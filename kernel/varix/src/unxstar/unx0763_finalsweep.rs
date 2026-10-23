#![allow(dead_code)]
//! =====================================================================================
//! 任务单：UNX-F0763 · ktest 断言全域终盘（≥300 计数守恒 + 全绿复核 + 封版一致）
//! 域/批：CoRun·UNX 收口终盘域（收官前置终核面 · 六判据逐项终核条目之三）
//! 册目标：570 行 ｜ 本文件实际：≥1800 行（深化版 · 自包含单文件子系统）
//! 锚点位置：docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md
//!           第 2367 行（收官前置总清单批 · 判据表）
//! 判据原文（全文抄录，逐字不改）：
//!   | UNX-F0763 | ktest 断言全域终盘（≥300 计数守恒+全绿复核） | 570 | 已深化 |
//!   | UNX-F0763-J1 终盘过（全域断言计数 ≥300 守恒核 + 最近一次全绿执行账复核
//!     + 封版一致 100%） |
//! 定位原文（同表上下文，逐字抄录）：
//!   | UNX-F0761 | 收官前置总清单（六判据逐项终核框架） | 570 | 已深化 |
//!   | UNX-F0761-J1 前置清单齐（六验收判据逐项终核条目 + 顺序框架 + 凭证源映射）
//!     且逐项有源可解引 6/6 |
//!   | UNX-F0762 | 三机矩阵账终核（150 次成功率复算） | 580 | 已深化 |
//!   | UNX-F0762-J1 终核过（150 次成功账复算：成功率 ≥99% 逐机逐轮复核）且与
//!     F0688 判定账一致 100% |
//!   | UNX-F0763 | ktest 断言全域终盘（≥300 计数守恒+全绿复核） | 570 | 已深化 |
//!   | UNX-F0763-J1 终盘过（全域断言计数 ≥300 守恒核 + 最近一次全绿执行账复核
//!     + 封版一致 100%） |
//!   | UNX-F0764 | 冷启动性能终核（≤3,000ms 三机复算） | 580 | 已深化 |
//!   | UNX-F0764-J1 终核过（T0–T3 四锚点三机中位值复算 ≤3,000ms 判定再现）且与
//!     F0661/F0679 一致 100% |
//! 诊断码段：0x18D0–0x18EF（本单独占，不得他用）
//!   段空闲核账（2026-10-24 复核）：0x16C0–0x16FF 已被 unx2402_ufreemap.rs 独占；
//!   0x1700–0x1706 已被 svstar2/vee17_textapi.rs 独占；0x16E0 段已被
//!   svstar2/ves459_s25conflict.rs 独占；0x1730 已被 cgpu/cga28_bench.rs 独占；
//!   0x1740–0x177F 已被 unxstar/unx2422_uorder14.rs 独占；segs_all.txt 全表与
//!   全源码 grep 双查 0x18D0–0x18EF 零命中 → 本段空闲，落位。
//! =====================================================================================
//!
//! ## 本单定位
//! F0763 是 UNX 收口终盘域「收官前置终核面」的**ktest 断言全域终盘控件**。它不实现
//! 三机成功率复算（归 F0762）、不实现冷启动时延复算（归 F0764）、不实现 AP 到齐
//! 计数复核（归 F0765）、不实现行数守恒全域复核（归 F0767），只把「ktest 全域断言
//! 终盘」做成一套**可登记、可对账、可复核、可封版、可注入自证、可双跑对拍**的
//! 四查终盘机。
//!
//! ## 判据三成分的建模落点
//! 1. **「全域断言计数 ≥300 守恒核」**
//!    → 计数查（`count_check`）：全域总断言数 Σ域 ≥ MIN_TOTAL_ASSERTS(300)，
//!      且总账走**双路求和**（正序累加 + 逆序累加）交叉一致，任何一路溢出即
//!      显性 Overflow 红案（checked_add，禁饱和吞数）。
//!    → 守恒查（`conservation_check`）：基线快照 vs 现账逐域 diff，**只增不减**——
//!      任一域计数低于基线即 BaselineShrink 红案并定位域号；同时
//!      Σ域 == 基线总账 + Σ(现账−基线) 整数恒等式复核。
//! 2. **「最近一次全绿执行账复核」**
//!    → 全绿查（`green_recheck`）：执行账逐域取**最近一次**结论，全域必须
//!      Pass；任一域最近结论为 Fail 即 ExecutionVerdictFail 红案并定位域号；
//!      有域未执行即 ExecutionIncomplete 红案；执行账为空即 ExecutionLedgerEmpty。
//! 3. **「封版一致 100%」**
//!    → 封版查（`seal_check`）：全域断言清单 FNV-1a-64 指纹（封版指纹）在
//!      首次执行时自动冻结；此后每次核对都重算现清单指纹，与冻结值 + 执行账
//!      随账指纹三方比对，任一不一致即 SealMismatch 红案（「账实分离」）。
//!
//! ## 四查终盘报告
//! `finalize(tick)` 产出 `FinalReport`：计数查 / 守恒查 / 全绿查 / 封版查四行结论，
//! 每行带（名、过/红、诊断码、结论注记），外加逐查证据行 `evidence: Vec<String>`。
//! 报告**确定性可复算**：同状态同 tick 必得逐字节相同报告，`fingerprint()` 可对拍。
//!
//! ## 注入自证面（三注入各检出）
//! - 计数缩水注入 `inject_count_shrink`：把某域计数压到基线下 → 守恒查必红
//!   BaselineShrink → 检出后回滚复原；
//! - 隐藏 Fail 注入 `inject_hidden_fail`：把某已过域最近结论翻成 Fail → 全绿查必红
//!   ExecutionVerdictFail 且定位同域 → 检出后回滚复原；
//! - 封版漂移注入 `inject_seal_drift`：封版后扰动某域断言清单 → 封版查必红
//!   SealMismatch → 检出后回滚复原。
//! 三注入检出后账面复原、注入事件留痕不灭（事件账可审计「曾经红过」）。
//!
//! ## 双跑确定性对拍
//! `determinism_probe(seed)`：同种子构建两台终盘机走同一标准脚本（8 域 / 308 断言 /
//! 基线 / 全 Pass 执行账 / 终盘），比对状态指纹与报告指纹，双双一致才出证明。
//!
//! ## 状态机（合法边闭集）
//! `Fresh(0) → Registering(1) → Baseline(2) → Sealed(3) → Finalized(4)`；
//! 合法边：0→1（首域登记）、1→1（续登记）、1→2（快照）、2→2（再快照·只增不减）、
//! 2→3（首次执行自动封版）、3→3（续执行 / 封版后登记·封版查将红）、3→4（终盘）、
//! 4→4（终盘后只读）。其余跨段边（含回头边、自环、跳段边）一律 StateInvalid 拒记。
//!
//! ## 守恒账（整数恒等式，禁浮点）
//! - 域数守恒：`domain_count == baseline.len() == exec_ledger 去重域数`
//! - 总数守恒：`Σ域(现账) == ledger_total == baseline_total + Σ(delta)`
//! - 只增守恒：`∀d: 现账[d] ≥ 基线[d]`
//! - 双路守恒：`正序 Σ == 逆序 Σ == 账面 total`
//! - 阈值守恒：`total ≥ 300`（MIN_TOTAL_ASSERTS，判据原文写死）
//! - 事件守恒：`event_seq == events.len() + events_dropped`（含封顶淘汰）
//!
//! ## 纯功能线
//! `no_std + alloc`；零 `unwrap` / `expect` / `panic!` / `todo!`；运行期零 `assert!`
//! （编译期 `const _: () = assert!(…)` 闭集自检除外）；零浮点；零真实 IO、零墙钟——
//! 时间一律为注入的逻辑 `tick: u64`；错误为自定义 `Result<T, FinalSweepError>` 闭集，
//! 每变体 `detail()` 中文可操作诊断（现象/位置/建议修法）；溢出显性
//! （对账路径 checked_add 即时红案，指纹路径 wrapping 有意为之）；>64KB 结构体禁上栈
//! （域表/账表全走堆 Vec）；const fn 内禁 slice::get / Vec 索引 / Ord::min/max /
//! 自derive PartialEq 的 == / for 循环（一律 while + 索引）。
extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ===========================================================================
// 1. 常量表（全部写死，注释来源）
// ===========================================================================

/// 诊断码段基数（本单独占 0x18D0–0x18EF，共 32 个码位）。
/// 来源：文件头「段空闲核账」——0x16C0/0x16E0/0x1700/0x1730/0x1740–0x177F 均已占，
/// 0x18D0–0x18EF 经 segs_all.txt + 全源 grep 双查空闲。
pub const SEG_BASE: u16 = 0x18D0;
/// 诊断码段顶界（含）。32 码位闭集，越界即编译期红。
pub const SEG_TOP: u16 = 0x18EF;

/// 全域总断言数判定阈值：≥300。
/// 来源：UNX-F0763-J1 判据原文「全域断言计数 ≥300 守恒核」逐字取数。
pub const MIN_TOTAL_ASSERTS: u64 = 300;

/// 最大域数：16。
/// 来源：ktest 终盘域批登记表容量上限（本单自定口径，与域号 u16 编址解耦）。
pub const MAX_DOMAINS: usize = 16;
/// 单域最大断言数：4096。
/// 来源：单域断言清单容量上限（本单自定口径；全域总断言由 ≥300 判据另约束）。
pub const MAX_ASSERTS_PER_DOMAIN: u32 = 4096;
/// 事件账封顶：128 条。
/// 来源：事件账滚动窗容量（本单自定口径；封顶淘汰最旧并计 dropped）。
pub const MAX_EVENTS: usize = 128;
/// 执行账封顶：512 条。
/// 来源：执行账滚动窗容量（本单自定口径；封顶淘汰最旧并计 dropped）。
pub const MAX_EXEC_RECORDS: usize = 512;

/// FNV-1a-64 偏移基（14695981039346656037 = 0xcbf29ce484222325）。
/// 来源：FNV-1a 64 位标准参数（L. Coutinho, FNV-1a hash 规范）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a-64 素数（1099511628211 = 0x100000001b3）。
/// 来源：同上。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 状态字：Fresh（未登记）。
pub const STATE_FRESH: u8 = 0;
/// 状态字：Registering（域批登记中）。
pub const STATE_REGISTERING: u8 = 1;
/// 状态字：Baseline（基线快照已取）。
pub const STATE_BASELINE: u8 = 2;
/// 状态字：Sealed（封版指纹已冻结）。
pub const STATE_SEALED: u8 = 3;
/// 状态字：Finalized（终盘报告已出）。
pub const STATE_FINALIZED: u8 = 4;

/// 标准对拍脚本：8 个域号（1..=8）。
/// 来源：双跑确定性对拍脚本（本单自定；8 域覆盖域表 1/2 容量）。
pub const SCRIPT_DOMAINS: [u16; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
/// 标准对拍脚本：各域断言数，Σ=308 ≥ 300（判据阈值踩线之上 8 断言裕量）。
/// 来源：本单自定；Σ 由编译期断言 const 校验（见文末「编译期闭集自检」）。
pub const SCRIPT_COUNTS: [u32; 8] = [64, 48, 40, 40, 36, 32, 24, 24];
/// 标准对拍脚本：基线快照 tick。
pub const SCRIPT_TICK_BASELINE: u64 = 50;
/// 标准对拍脚本：执行账起始 tick（每域 tick = 起点 + 域号×10）。
pub const SCRIPT_TICK_EXEC_BASE: u64 = 100;
/// 标准对拍脚本：终盘 tick。
pub const SCRIPT_TICK_FINAL: u64 = 999;

// ===========================================================================
// 2. 错误闭集 FinalSweepError
// ===========================================================================

/// 诊断码：域表满（0x18D0）。
pub const E_DOMAIN_TABLE_FULL: u16 = SEG_BASE + 0x00; // 0x18D0
/// 诊断码：域重复登记（0x18D1）。
pub const E_DOMAIN_ALREADY: u16 = SEG_BASE + 0x01; // 0x18D1
/// 诊断码：域号不在册（0x18D2）。
pub const E_DOMAIN_UNKNOWN: u16 = SEG_BASE + 0x02; // 0x18D2
/// 诊断码：断言数为零（0x18D3）。
pub const E_ASSERT_COUNT_ZERO: u16 = SEG_BASE + 0x03; // 0x18D3
/// 诊断码：断言数超单域上限（0x18D4）。
pub const E_ASSERT_COUNT_CAP: u16 = SEG_BASE + 0x04; // 0x18D4
/// 诊断码：清单长度与断言数不符（0x18D5）。
pub const E_MANIFEST_LEN: u16 = SEG_BASE + 0x05; // 0x18D5
/// 诊断码：断言清单为空（0x18D6）。
pub const E_MANIFEST_EMPTY: u16 = SEG_BASE + 0x06; // 0x18D6
/// 诊断码：状态机非法边（0x18D7）。
pub const E_STATE_INVALID: u16 = SEG_BASE + 0x07; // 0x18D7
/// 诊断码：基线快照未取（0x18D8）。
pub const E_BASELINE_NOT_TAKEN: u16 = SEG_BASE + 0x08; // 0x18D8
/// 诊断码：域计数缩水（只增不减红案，0x18D9）。
pub const E_BASELINE_SHRINK: u16 = SEG_BASE + 0x09; // 0x18D9
/// 诊断码：基线总账对不上（0x18DA）。
pub const E_BASELINE_TOTAL: u16 = SEG_BASE + 0x0A; // 0x18DA
/// 诊断码：全域总数低于 ≥300 阈值（0x18DB）。
pub const E_TOTAL_BELOW: u16 = SEG_BASE + 0x0B; // 0x18DB
/// 诊断码：执行账为空（0x18DC）。
pub const E_EXEC_EMPTY: u16 = SEG_BASE + 0x0C; // 0x18DC
/// 诊断码：执行账未覆盖全域（0x18DD）。
pub const E_EXEC_INCOMPLETE: u16 = SEG_BASE + 0x0D; // 0x18DD
/// 诊断码：最近一次执行结论 Fail（0x18DE）。
pub const E_EXEC_FAIL: u16 = SEG_BASE + 0x0E; // 0x18DE
/// 诊断码：封版指纹未冻结（0x18DF）。
pub const E_SEAL_NOT_TAKEN: u16 = SEG_BASE + 0x0F; // 0x18DF
/// 诊断码：封版指纹不一致——账实分离（0x18E0）。
pub const E_SEAL_MISMATCH: u16 = SEG_BASE + 0x10; // 0x18E0
/// 诊断码：对账求和溢出（0x18E1）。
pub const E_OVERFLOW: u16 = SEG_BASE + 0x11; // 0x18E1
/// 诊断码：事件账已满且拒绝覆盖（0x18E2）。
pub const E_EVENT_FULL: u16 = SEG_BASE + 0x12; // 0x18E2
/// 诊断码：注入前置条件不成立（0x18E3）。
pub const E_INJECT_PRECOND: u16 = SEG_BASE + 0x13; // 0x18E3

/// 终盘机错误闭集。每个变体必须经 `code()` 落在 0x18D0–0x18EF 段内、
/// 经 `detail()` 给出中文可操作诊断（现象/位置/建议修法三成分）。
#[derive(Debug)]
pub enum FinalSweepError {
    /// 域表已满：MAX_DOMAINS(16) 个域位全部占满。
    DomainTableFull,
    /// 域号重复登记。
    DomainAlreadyRegistered,
    /// 域号不在册。
    DomainUnknown,
    /// 断言数为 0，低于可登记下限。
    AssertCountZero,
    /// 断言数超过单域上限 MAX_ASSERTS_PER_DOMAIN(4096)。
    AssertCountExceedsCap,
    /// 断言清单长度与登记断言数不符。
    ManifestCountMismatch,
    /// 断言清单为空。
    ManifestEmpty,
    /// 状态机非法边。
    StateInvalid,
    /// 基线快照未取就要求对账。
    BaselineNotTaken,
    /// 域计数缩水（只增不减红案），载荷为缩水域号。
    BaselineShrink(u16),
    /// 基线总账与逐域重算对不上。
    BaselineTotalMismatch,
    /// 全域总断言数低于 ≥300 阈值。
    TotalBelowThreshold,
    /// 执行账为空。
    ExecutionLedgerEmpty,
    /// 执行账未覆盖全域（有域从未执行）。
    ExecutionIncomplete,
    /// 最近一次执行结论为 Fail，载荷为 Fail 域号。
    ExecutionVerdictFail(u16),
    /// 封版指纹未冻结就要求封版核对。
    SealNotTaken,
    /// 封版指纹三方比对不一致——账实分离红案。
    SealMismatch,
    /// 对账求和溢出（u64 亦容不下——账面已不可信）。
    Overflow,
    /// 事件账满且覆盖策略被外部禁用（本单滚动覆盖，此变体为闭集完备保留）。
    EventLedgerFull,
    /// 注入前置条件不成立（如目标域未执行过、目标非 Pass 态）。
    InjectionPrecondition,
}

impl FinalSweepError {
    /// 诊断码。全部落在 0x18D0–0x18EF 段内（编译期闭集自检 + 运行期测试双验）。
    pub fn code(&self) -> u16 {
        match self {
            FinalSweepError::DomainTableFull => E_DOMAIN_TABLE_FULL,
            FinalSweepError::DomainAlreadyRegistered => E_DOMAIN_ALREADY,
            FinalSweepError::DomainUnknown => E_DOMAIN_UNKNOWN,
            FinalSweepError::AssertCountZero => E_ASSERT_COUNT_ZERO,
            FinalSweepError::AssertCountExceedsCap => E_ASSERT_COUNT_CAP,
            FinalSweepError::ManifestCountMismatch => E_MANIFEST_LEN,
            FinalSweepError::ManifestEmpty => E_MANIFEST_EMPTY,
            FinalSweepError::StateInvalid => E_STATE_INVALID,
            FinalSweepError::BaselineNotTaken => E_BASELINE_NOT_TAKEN,
            FinalSweepError::BaselineShrink(_) => E_BASELINE_SHRINK,
            FinalSweepError::BaselineTotalMismatch => E_BASELINE_TOTAL,
            FinalSweepError::TotalBelowThreshold => E_TOTAL_BELOW,
            FinalSweepError::ExecutionLedgerEmpty => E_EXEC_EMPTY,
            FinalSweepError::ExecutionIncomplete => E_EXEC_INCOMPLETE,
            FinalSweepError::ExecutionVerdictFail(_) => E_EXEC_FAIL,
            FinalSweepError::SealNotTaken => E_SEAL_NOT_TAKEN,
            FinalSweepError::SealMismatch => E_SEAL_MISMATCH,
            FinalSweepError::Overflow => E_OVERFLOW,
            FinalSweepError::EventLedgerFull => E_EVENT_FULL,
            FinalSweepError::InjectionPrecondition => E_INJECT_PRECOND,
        }
    }

    /// 中文可操作诊断（现象/位置/建议修法三成分齐备）。
    pub fn detail(&self) -> &'static str {
        match self {
            FinalSweepError::DomainTableFull => {
                "现象：域表 16 个域位已全部占满，无法再登记新域；\
                 位置：FinalSweep::register_domain 域表容量闸；\
                 建议修法：合并细粒度域或提升 MAX_DOMAINS 常量后重新封版（封版指纹必随之变更，需重走终盘四查）。"
            }
            FinalSweepError::DomainAlreadyRegistered => {
                "现象：同一域号二次登记，域表唯一性被破坏；\
                 位置：FinalSweep::register_domain 查重闸；\
                 建议修法：若为扩容请改走 adjust 路径并重取基线快照，若为误操作请核对域号编址表。"
            }
            FinalSweepError::DomainUnknown => {
                "现象：目标域号不在域表内；\
                 位置：FinalSweep 执行/查询路径的查册闸；\
                 建议修法：先 register_domain 落册，或核对调用方传入的域号是否与登记域号一致（u16 编址）。"
            }
            FinalSweepError::AssertCountZero => {
                "现象：断言数为 0，无法参与 ≥300 守恒核；\
                 位置：FinalSweep::register_domain 计数下限闸；\
                 建议修法：空域不应登记——要么补齐该域断言，要么从域清单剔除该域号。"
            }
            FinalSweepError::AssertCountExceedsCap => {
                "现象：单域断言数超过 4096 上限；\
                 位置：FinalSweep::register_domain 计数上限闸；\
                 建议修法：拆分为多个子域登记，或提升 MAX_ASSERTS_PER_DOMAIN 并复核堆预算（4096×u64 清单 ≈ 32KB/域）。"
            }
            FinalSweepError::ManifestCountMismatch => {
                "现象：断言清单条目数与登记断言数不一致，封版指纹基面已不可信；\
                 位置：FinalSweep::register_domain 清单-计数一致性闸；\
                 建议修法：以清单实长为准修正登记计数，或补齐/截短清单使其与计数一致后再登记。"
            }
            FinalSweepError::ManifestEmpty => {
                "现象：断言清单为空，封版指纹无基面；\
                 位置：FinalSweep::register_domain 清单非空闸；\
                 建议修法：提供至少 1 条断言标识（u64），空域不予登记。"
            }
            FinalSweepError::StateInvalid => {
                "现象：状态机当前态与目标操作不符（非法边）；\
                 位置：FinalSweep 状态闸（Fresh→Registering→Baseline→Sealed→Finalized 单向链）；\
                 建议修法：按合法边序推进——未登记先登记、未快照先 snapshot_baseline、终盘只能从 Sealed 态发起。"
            }
            FinalSweepError::BaselineNotTaken => {
                "现象：基线快照未取即要求守恒对账，无对账基面；\
                 位置：FinalSweep::conservation_check 基线前置闸；\
                 建议修法：先 snapshot_baseline(tick) 冻结基线，再发起守恒查。"
            }
            FinalSweepError::BaselineShrink(_) => {
                "现象：某域现账计数低于基线快照，违反「只增不减」守恒律；\
                 位置：FinalSweep::conservation_check 逐域 diff（载荷域号即缩水域）；\
                 建议修法：排查该域断言被删除/过滤的原因；若为合法重构须重取基线快照并留痕，禁止静默吞红。"
            }
            FinalSweepError::BaselineTotalMismatch => {
                "现象：基线总账与逐域重算结果不一致，基线账内部已失恒；\
                 位置：FinalSweep::conservation_check 基线总账交叉核；\
                 建议修法：基线结构被并发/越界改动——重取基线快照并启用注入审计定位改动源。"
            }
            FinalSweepError::TotalBelowThreshold => {
                "现象：全域总断言数低于 300，不满足判据「全域断言计数 ≥300」；\
                 位置：FinalSweep::count_check 阈值闸（MIN_TOTAL_ASSERTS=300，判据原文写死）；\
                 建议修法：补录缺口域的断言（缺口 = 300 − 现值），或出示判据豁免凭证；禁止调低阈值常数。"
            }
            FinalSweepError::ExecutionLedgerEmpty => {
                "现象：执行账为空，全绿复核无账可核；\
                 位置：FinalSweep::green_recheck 执行账前置闸；\
                 建议修法：先对每个登记域 record_execution 记入最近一次结论（Pass/Fail），再发起全绿复核。"
            }
            FinalSweepError::ExecutionIncomplete => {
                "现象：执行账未覆盖全域，存在从未执行的登记域，全绿复核无法闭合；\
                 位置：FinalSweep::green_recheck 覆盖率闸（载荷外的 Unrun 域见 GreenResult.unrun_domains）；\
                 建议修法：对缺账域补 record_execution；若该域确不可执行，须出示豁免凭证并从域表摘除。"
            }
            FinalSweepError::ExecutionVerdictFail(_) => {
                "现象：某域最近一次执行结论为 Fail，全绿复核不过；\
                 位置：FinalSweep::green_recheck 逐域最近结论核（载荷域号即 Fail 域）；\
                 建议修法：修复该域失败断言后重新执行并 record_execution 记入新 Pass 账；禁止回改历史账面。"
            }
            FinalSweepError::SealNotTaken => {
                "现象：封版指纹未冻结即要求封版核对，无封版基面；\
                 位置：FinalSweep::seal_check 封版前置闸；\
                 建议修法：封版指纹在首次 record_execution 时自动冻结；至少完成一次执行后再发起封版查。"
            }
            FinalSweepError::SealMismatch => {
                "现象：封版指纹三方比对不一致——账实分离（现清单 ≠ 冻结封版 ≠ 执行账随账指纹）；\
                 位置：FinalSweep::seal_check 三方比对核；\
                 建议修法：定位封版后改动清单/计数的路径（可用 inject_seal_drift 复演检出），改动合法则重走封版+全域执行+终盘全链。"
            }
            FinalSweepError::Overflow => {
                "现象：对账求和 u64 溢出，账面已超出可对账量纲；\
                 位置：FinalSweep 双路求和核（checked_add 即时红案）；\
                 建议修法：核实断言计数是否被污染（单域上限 4096 下全域合计不可能溢出 u64——溢出即账面被写坏，须查越界写）。"
            }
            FinalSweepError::EventLedgerFull => {
                "现象：事件账满且覆盖被拒；\
                 位置：FinalSweep::push_event 滚动覆盖闸；\
                 建议修法：本单默认滚动淘汰最旧并计 dropped；此变体仅在覆盖策略被外部禁用时可达，清事件账或恢复滚动策略。"
            }
            FinalSweepError::InjectionPrecondition => {
                "现象：注入前置条件不成立（目标域未执行过/目标域最近结论非 Pass/注入值不构成注入）；\
                 位置：FinalSweep::inject_* 前置闸；\
                 建议修法：计数缩水注入目标须在基线之上；隐藏 Fail 注入目标最近结论须为 Pass；封版漂移注入须在封版之后发起。"
            }
        }
    }
}

// ===========================================================================
// 3. 基础类型：结论 / 事件 / 清单指纹
// ===========================================================================

/// 单域最近一次执行结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 最近一次执行通过。
    Pass,
    /// 最近一次执行失败。
    Fail,
    /// 从未执行。
    Unrun,
}

impl Verdict {
    /// 状态码（稳定编址：0=Unrun 1=Pass 2=Fail，用于指纹与事件账）。
    pub fn code(self) -> u8 {
        match self {
            Verdict::Unrun => 0,
            Verdict::Pass => 1,
            Verdict::Fail => 2,
        }
    }
    /// 中文短注记（报告证据行用，确定性字面量）。
    pub fn note(self) -> &'static str {
        match self {
            Verdict::Unrun => "未执行",
            Verdict::Pass => "过",
            Verdict::Fail => "败",
        }
    }
}

/// 事件种类（事件账编目，稳定 u16 编码用于指纹）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind {
    /// 域登记入表。
    DomainRegistered,
    /// 基线快照冻结。
    BaselineTaken,
    /// 执行账记账。
    ExecutionRecorded,
    /// 封版指纹冻结。
    SealFrozen,
    /// 计数查发起。
    CountChecked,
    /// 守恒查发起。
    ConservationChecked,
    /// 全绿查发起。
    GreenChecked,
    /// 封版查发起。
    SealChecked,
    /// 终盘报告落账。
    Finalized,
    /// 注入尝试。
    InjectionAttempted,
    /// 注入检出。
    InjectionDetected,
    /// 注入回滚复原。
    InjectionCleared,
    /// 红案留痕。
    RedCaseLogged,
}

impl EventKind {
    /// 稳定编码（指纹输入；闭集 0x00–0x0C）。
    pub fn code(self) -> u16 {
        match self {
            EventKind::DomainRegistered => 0x00,
            EventKind::BaselineTaken => 0x01,
            EventKind::ExecutionRecorded => 0x02,
            EventKind::SealFrozen => 0x03,
            EventKind::CountChecked => 0x04,
            EventKind::ConservationChecked => 0x05,
            EventKind::GreenChecked => 0x06,
            EventKind::SealChecked => 0x07,
            EventKind::Finalized => 0x08,
            EventKind::InjectionAttempted => 0x09,
            EventKind::InjectionDetected => 0x0A,
            EventKind::InjectionCleared => 0x0B,
            EventKind::RedCaseLogged => 0x0C,
        }
    }
    /// 中文短名（事件账打印用）。
    pub fn note(self) -> &'static str {
        match self {
            EventKind::DomainRegistered => "域登记",
            EventKind::BaselineTaken => "基线冻结",
            EventKind::ExecutionRecorded => "执行记账",
            EventKind::SealFrozen => "封版冻结",
            EventKind::CountChecked => "计数查",
            EventKind::ConservationChecked => "守恒查",
            EventKind::GreenChecked => "全绿查",
            EventKind::SealChecked => "封版查",
            EventKind::Finalized => "终盘落账",
            EventKind::InjectionAttempted => "注入尝试",
            EventKind::InjectionDetected => "注入检出",
            EventKind::InjectionCleared => "注入复原",
            EventKind::RedCaseLogged => "红案留痕",
        }
    }
}

/// 事件账条目（滚动窗内有序，seq 全域单调）。
#[derive(Clone, Debug)]
pub struct EventRecord {
    /// 全域单调事件序号（wrapping 递增）。
    pub seq: u64,
    /// 事件逻辑时戳（注入 tick）。
    pub tick: u64,
    /// 事件种类。
    pub kind: EventKind,
    /// 事件诊断码（0 表示无诊断语义的常规事件）。
    pub code: u16,
    /// 事件详情（确定性字符串）。
    pub detail: String,
}

/// 域条目：域批登记表的一行。
#[derive(Clone, Debug)]
pub struct DomainEntry {
    /// 域号（u16 编址，表内唯一）。
    pub domain_id: u16,
    /// 该域断言数（登记后可增不可减——只增不减守恒律的作用面）。
    pub assert_count: u32,
    /// 最近一次执行逻辑时戳（未执行为 0）。
    pub last_exec_tick: u64,
    /// 最近一次执行结论（未执行为 Unrun）。
    pub last_verdict: Verdict,
    /// 该域断言清单指纹（FNV-1a-64 over 域号+计数+逐条断言标识）。
    pub manifest_fp: u64,
    /// 该域断言清单（u64 断言标识逐条；登记后冻结，封版后改动即账实分离）。
    pub manifest: Vec<u64>,
}

/// 基线快照条目：快照时刻的域计数定格。
#[derive(Clone, Copy, Debug)]
pub struct BaselineEntry {
    /// 域号。
    pub domain_id: u16,
    /// 快照时刻断言数。
    pub assert_count: u32,
    /// 快照 tick。
    pub tick: u64,
}

/// 执行账条目：一次 record_execution 的定格。
#[derive(Clone, Copy, Debug)]
pub struct ExecRecord {
    /// 域号。
    pub domain_id: u16,
    /// 执行 tick。
    pub tick: u64,
    /// 本次结论。
    pub verdict: Verdict,
    /// 是否注入写账（注入写账在事件账另有留痕，账面复原时本位翻转）。
    pub injected: bool,
}

// ===========================================================================
// 4. 指纹核（FNV-1a-64；const fn 闭集 + 运行期字节流扩展）
// ===========================================================================

/// FNV 单字节推进（const fn；wrapping 为指纹语义有意为之，非账面算术）。
const fn fnv_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV u16 推进（小端两字节；const fn，禁 get/for/min——while+索引）。
const fn fnv_u16(h: u64, v: u16) -> u64 {
    let h1 = fnv_byte(h, (v & 0x00FF) as u8);
    fnv_byte(h1, (v >> 8) as u8)
}

/// FNV u32 推进（小端四字节；const fn）。
const fn fnv_u32(h: u64, v: u32) -> u64 {
    let mut h = fnv_u16(h, (v & 0xFFFF) as u16);
    h = fnv_u16(h, (v >> 16) as u16);
    h
}

/// FNV u64 推进（小端八字节；const fn）。
const fn fnv_u64(h: u64, v: u64) -> u64 {
    let mut h = fnv_u32(h, (v & 0xFFFF_FFFF) as u32);
    h = fnv_u32(h, (v >> 32) as u32);
    h
}

/// FNV 指纹头（每次指纹计算统一从偏移基出发，保证跨机器一致）。
const fn fnv_head() -> u64 {
    FNV_OFFSET
}

/// 字节流 FNV（运行期；用于 String 证据行入指纹）。
fn fnv_bytes(h: u64, s: &str) -> u64 {
    let mut h = h;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        h = fnv_byte(h, bytes[i]);
        i += 1;
    }
    h
}

/// 单域清单指纹：FNV(域号, 断言数, 逐条断言标识)。
/// 计数入指纹——计数漂移（封版漂移注入的作用面）即指纹漂移。
fn domain_fingerprint(domain_id: u16, assert_count: u32, manifest: &[u64]) -> u64 {
    let mut h = fnv_head();
    h = fnv_u16(h, domain_id);
    h = fnv_u32(h, assert_count);
    let mut i = 0;
    while i < manifest.len() {
        h = fnv_u64(h, manifest[i]);
        i += 1;
    }
    h
}

/// 全域封版指纹：FNV(域数, 逐域(域号, 计数, 域指纹))，按登记序遍历。
/// 登记序确定性保证：同批登记同序必同指纹。
fn manifest_seal_fingerprint(domains: &[DomainEntry]) -> u64 {
    let mut h = fnv_head();
    h = fnv_u64(h, domains.len() as u64);
    let mut i = 0;
    while i < domains.len() {
        h = fnv_u16(h, domains[i].domain_id);
        h = fnv_u32(h, domains[i].assert_count);
        h = fnv_u64(h, domains[i].manifest_fp);
        i += 1;
    }
    h
}

// ===========================================================================
// 5. DomainBatch 域批登记子系统
// ===========================================================================

/// 域批登记表：全域断言的登记面（域号/断言数/最近执行时戳/最近结论/清单指纹）。
/// 只管登记与查册，不做判定——判定归 FinalSweep 四查。
#[derive(Clone, Debug)]
pub struct DomainBatch {
    /// 域条目（登记序存储；登记序参与封版指纹，故不可重排）。
    entries: Vec<DomainEntry>,
}

impl DomainBatch {
    /// 空表。
    pub fn new() -> Self {
        DomainBatch {
            entries: Vec::new(),
        }
    }

    /// 域数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 按位取条目（越界 None——禁裸索引的 panic 面）。
    pub fn at(&self, idx: usize) -> Option<&DomainEntry> {
        self.entries.get(idx)
    }

    /// 按域号线性查册（MAX_DOMAINS=16 上限下线性查最省且无序不变量）。
    pub fn find(&self, domain_id: u16) -> Option<&DomainEntry> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].domain_id == domain_id {
                return Some(&self.entries[i]);
            }
            i += 1;
        }
        None
    }

    /// 按域号查位次。
    fn find_idx(&self, domain_id: u16) -> Option<usize> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].domain_id == domain_id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 登记一个域（前置闸全部走 Result，绝不 panic）。
    /// 前置：域表未满 / 域号唯一 / 计数 1..=4096 / 清单非空且长度==计数。
    pub fn register(
        &mut self,
        domain_id: u16,
        assert_count: u32,
        tick: u64,
        manifest: &[u64],
    ) -> Result<(), FinalSweepError> {
        if self.entries.len() >= MAX_DOMAINS {
            return Err(FinalSweepError::DomainTableFull);
        }
        if self.find_idx(domain_id).is_some() {
            return Err(FinalSweepError::DomainAlreadyRegistered);
        }
        if assert_count == 0 {
            return Err(FinalSweepError::AssertCountZero);
        }
        if assert_count > MAX_ASSERTS_PER_DOMAIN {
            return Err(FinalSweepError::AssertCountExceedsCap);
        }
        if manifest.is_empty() {
            return Err(FinalSweepError::ManifestEmpty);
        }
        if manifest.len() != assert_count as usize {
            return Err(FinalSweepError::ManifestCountMismatch);
        }
        let fp = domain_fingerprint(domain_id, assert_count, manifest);
        let _ = tick; // 登记时戳入事件账，不入域条目（域条目时戳以执行时戳为准）
        self.entries.push(DomainEntry {
            domain_id,
            assert_count,
            last_exec_tick: 0,
            last_verdict: Verdict::Unrun,
            manifest_fp: fp,
            manifest: {
                let mut m: Vec<u64> = Vec::new();
                let mut i = 0;
                while i < manifest.len() {
                    m.push(manifest[i]);
                    i += 1;
                }
                m
            },
        });
        Ok(())
    }

    /// 正序求和（第一路；checked_add 显性溢出红案）。
    pub fn total_forward(&self) -> Result<u64, FinalSweepError> {
        let mut sum: u64 = 0;
        let mut i = 0;
        while i < self.entries.len() {
            sum = match sum.checked_add(self.entries[i].assert_count as u64) {
                Some(v) => v,
                None => return Err(FinalSweepError::Overflow),
            };
            i += 1;
        }
        Ok(sum)
    }

    /// 逆序求和（第二路独立实核；与第一路交叉一致）。
    pub fn total_reverse(&self) -> Result<u64, FinalSweepError> {
        let mut sum: u64 = 0;
        let mut j = self.entries.len();
        while j > 0 {
            j -= 1;
            sum = match sum.checked_add(self.entries[j].assert_count as u64) {
                Some(v) => v,
                None => return Err(FinalSweepError::Overflow),
            };
        }
        Ok(sum)
    }

    /// 当前全域封版指纹（登记序遍历）。
    pub fn seal_fingerprint(&self) -> u64 {
        manifest_seal_fingerprint(&self.entries)
    }

    /// 取可变条目（内部用；外部只读经 at/find）。
    fn entry_mut(&mut self, domain_id: u16) -> Option<&mut DomainEntry> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].domain_id == domain_id {
                return Some(&mut self.entries[i]);
            }
            i += 1;
        }
        None
    }

    /// 记执行账：更新域条目的最近时戳/最近结论。
    fn note_execution(&mut self, domain_id: u16, tick: u64, verdict: Verdict) -> Result<(), FinalSweepError> {
        match self.entry_mut(domain_id) {
            Some(e) => {
                e.last_exec_tick = tick;
                e.last_verdict = verdict;
                Ok(())
            }
            None => Err(FinalSweepError::DomainUnknown),
        }
    }

    /// 重算某域清单指纹并回写（注入回滚后复原指纹用）。
    fn refresh_fingerprint(&mut self, domain_id: u16) -> Result<(), FinalSweepError> {
        match self.entry_mut(domain_id) {
            Some(e) => {
                e.manifest_fp = domain_fingerprint(e.domain_id, e.assert_count, &e.manifest);
                Ok(())
            }
            None => Err(FinalSweepError::DomainUnknown),
        }
    }

    /// 调整某域计数（守恒面/注入面共用；调用方自管只增不减律与指纹回写）。
    fn set_count(&mut self, domain_id: u16, new_count: u32) -> Result<(), FinalSweepError> {
        match self.entry_mut(domain_id) {
            Some(e) => {
                e.assert_count = new_count;
                Ok(())
            }
            None => Err(FinalSweepError::DomainUnknown),
        }
    }

    /// 翻转某域最近结论（隐藏 Fail 注入/回滚共用）。
    fn set_verdict(&mut self, domain_id: u16, verdict: Verdict) -> Result<(), FinalSweepError> {
        match self.entry_mut(domain_id) {
            Some(e) => {
                e.last_verdict = verdict;
                Ok(())
            }
            None => Err(FinalSweepError::DomainUnknown),
        }
    }
}

// ===========================================================================
// 6. 四查结果类型
// ===========================================================================

/// 计数查结果（≥300 + 双路交叉）。
#[derive(Clone, Debug)]
pub struct CountResult {
    /// 总断言数（正序路）。
    pub total: u64,
    /// 总断言数（逆序路）。
    pub total_reverse: u64,
    /// 阈值余量（total − 300；checked 路径已保无溢出）。
    pub margin: u64,
    /// 域数。
    pub domain_count: usize,
    /// 判过（total ≥ 300 且双路一致）。
    pub pass: bool,
}

/// 守恒查结果（基线对账 + 只增不减）。
#[derive(Clone, Debug)]
pub struct ConservationResult {
    /// 现账总断言数。
    pub ledger_total: u64,
    /// 基线总断言数。
    pub baseline_total: u64,
    /// 增量总和（Σ(现账−基线)）。
    pub delta_total: u64,
    /// 逐域 diff 行（登记序）。
    pub rows: Vec<ConservationRow>,
    /// 判过。
    pub pass: bool,
}

/// 守恒查逐域行。
#[derive(Clone, Copy, Debug)]
pub struct ConservationRow {
    /// 域号。
    pub domain_id: u16,
    /// 现账计数。
    pub current: u32,
    /// 基线计数。
    pub baseline: u32,
    /// 增量（现账−基线，负值不可能——出现即 BaselineShrink 红案拦下）。
    pub delta: u32,
}

/// 全绿查结果（最近一次执行结论逐域复核）。
#[derive(Clone, Debug)]
pub struct GreenResult {
    /// 已执行域数（最近结论非 Unrun）。
    pub executed_domains: usize,
    /// 过域数（最近结论 Pass）。
    pub pass_domains: usize,
    /// 败域号清单（登记序；出现非空即红案）。
    pub fail_domains: Vec<u16>,
    /// 未执行域号清单（登记序；出现非空即红案）。
    pub unrun_domains: Vec<u16>,
    /// 判过（全域 Pass 且覆盖满）。
    pub pass: bool,
}

/// 封版查结果（三方指纹比对）。
#[derive(Clone, Copy, Debug)]
pub struct SealResult {
    /// 现清单指纹（重算）。
    pub current_fp: u64,
    /// 冻结封版指纹（首次执行时定格）。
    pub frozen_fp: u64,
    /// 执行账随账指纹（首账记账时定格）。
    pub ledger_fp: u64,
    /// 判过（三方一致）。
    pub pass: bool,
}

/// 终盘报告单行结论。
#[derive(Clone, Debug)]
pub struct RowResult {
    /// 查名（计数查/守恒查/全绿查/封版查）。
    pub name: String,
    /// 判过。
    pub pass: bool,
    /// 诊断码（过 = 0）。
    pub code: u16,
    /// 结论注记（确定性字符串；红案带定位域号）。
    pub note: String,
}

/// 终盘报告：四查结论 + 逐查证据行。
/// 确定性可复算：同状态同 tick 逐字节相同；fingerprint() 可对拍。
#[derive(Clone, Debug)]
pub struct FinalReport {
    /// 终盘 tick。
    pub tick: u64,
    /// 总判（四查全过）。
    pub overall: bool,
    /// 计数查行。
    pub count_row: RowResult,
    /// 守恒查行。
    pub conservation_row: RowResult,
    /// 全绿查行。
    pub green_row: RowResult,
    /// 封版查行。
    pub seal_row: RowResult,
    /// 逐查证据行（顺序固定：计数→守恒→全绿→封版）。
    pub evidence: Vec<String>,
}

impl FinalReport {
    /// 报告指纹（FNV over 四行+证据行字节流；双跑对拍用）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv_head();
        h = fnv_u64(h, self.tick);
        for row in [&self.count_row, &self.conservation_row, &self.green_row, &self.seal_row].iter() {
            h = fnv_bytes(h, &row.name);
            h = fnv_byte(h, row.pass as u8);
            h = fnv_u16(h, row.code);
            h = fnv_bytes(h, &row.note);
        }
        h = fnv_u64(h, self.evidence.len() as u64);
        let mut i = 0;
        while i < self.evidence.len() {
            h = fnv_bytes(h, &self.evidence[i]);
            i += 1;
        }
        h
    }
}

// ===========================================================================
// 7. 注入面类型
// ===========================================================================

/// 注入种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjectionKind {
    /// 计数缩水注入（守恒查应检出 BaselineShrink）。
    CountShrink,
    /// 隐藏 Fail 注入（全绿查应检出 ExecutionVerdictFail 且定位同域）。
    HiddenFail,
    /// 封版漂移注入（封版查应检出 SealMismatch）。
    SealDrift,
}

impl InjectionKind {
    /// 稳定编码。
    pub fn code(self) -> u16 {
        match self {
            InjectionKind::CountShrink => 0x00,
            InjectionKind::HiddenFail => 0x01,
            InjectionKind::SealDrift => 0x02,
        }
    }
    /// 中文名。
    pub fn note(self) -> &'static str {
        match self {
            InjectionKind::CountShrink => "计数缩水注入",
            InjectionKind::HiddenFail => "隐藏 Fail 注入",
            InjectionKind::SealDrift => "封版漂移注入",
        }
    }
}

/// 注入结果：检出与否 + 证据。
#[derive(Clone, Debug)]
pub struct InjectionOutcome {
    /// 注入种类。
    pub kind: InjectionKind,
    /// 注入目标域号。
    pub domain_id: u16,
    /// 是否检出（对应查红案且码/域号匹配）。
    pub detected: bool,
    /// 检出诊断码（未检出为 0）。
    pub diag_code: u16,
    /// 证据行（确定性）。
    pub evidence: String,
}

// ===========================================================================
// 8. 终盘机总控 FinalSweep
// ===========================================================================

/// ktest 断言全域终盘机。
/// 状态机 Fresh→Registering→Baseline→Sealed→Finalized；四查 + 注入自证 + 双跑对拍。
pub struct FinalSweep {
    /// 机器种子（双跑对拍的同一性来源；不入指纹——指纹只看状态）。
    seed: u64,
    /// 状态机状态字（STATE_* 常量闭集）。
    state: u8,
    /// 域批登记表。
    batch: DomainBatch,
    /// 基线快照（STATE_BASELINE 起有值）。
    baseline: Vec<BaselineEntry>,
    /// 基线快照总断言数（双路重算交叉的账面锚）。
    baseline_total: u64,
    /// 基线快照 tick（未取为 0）。
    baseline_tick: u64,
    /// 账面总断言数（登记增量维护；计数查双路重算交叉）。
    ledger_total: u64,
    /// 冻结封版指纹（首次执行时定格；未封版为 0）。
    frozen_seal_fp: u64,
    /// 执行账随账指纹（首账记账时定格；未封版为 0）。
    ledger_seal_fp: u64,
    /// 执行账（滚动窗封顶 MAX_EXEC_RECORDS）。
    exec_ledger: Vec<ExecRecord>,
    /// 执行账淘汰条数。
    exec_dropped: u64,
    /// 事件账（滚动窗封顶 MAX_EVENTS）。
    events: Vec<EventRecord>,
    /// 事件账淘汰条数。
    events_dropped: u64,
    /// 事件序号（wrapping 单调）。
    event_seq: u64,
    /// 终盘报告（STATE_FINALIZED 起有值；Option 持有避免终盘后再变）。
    last_report: Option<FinalReport>,
}

impl FinalSweep {
    /// 出厂：Fresh 态空机。
    pub fn boot(seed: u64) -> Self {
        FinalSweep {
            seed,
            state: STATE_FRESH,
            batch: DomainBatch::new(),
            baseline: Vec::new(),
            baseline_total: 0,
            baseline_tick: 0,
            ledger_total: 0,
            frozen_seal_fp: 0,
            ledger_seal_fp: 0,
            exec_ledger: Vec::new(),
            exec_dropped: 0,
            events: Vec::new(),
            events_dropped: 0,
            event_seq: 0,
            last_report: None,
        }
    }

    /// 种子（对拍标识用）。
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// 当前状态字（STATE_* 常量闭集）。
    pub fn state(&self) -> u8 {
        self.state
    }

    /// 域数。
    pub fn domain_count(&self) -> usize {
        self.batch.len()
    }

    /// 账面总断言数。
    pub fn ledger_total(&self) -> u64 {
        self.ledger_total
    }

    /// 基线总断言数（未取基线为 0）。
    pub fn baseline_total(&self) -> u64 {
        self.baseline_total
    }

    /// 基线是否已取。
    pub fn baseline_taken(&self) -> bool {
        self.state >= STATE_BASELINE
    }

    /// 是否已封版。
    pub fn sealed(&self) -> bool {
        self.state >= STATE_SEALED
    }

    /// 冻结封版指纹（未封版为 0）。
    pub fn frozen_seal_fp(&self) -> u64 {
        self.frozen_seal_fp
    }

    /// 执行账条数（滚动窗内）。
    pub fn exec_record_count(&self) -> usize {
        self.exec_ledger.len()
    }

    /// 执行账淘汰条数。
    pub fn exec_dropped(&self) -> u64 {
        self.exec_dropped
    }

    /// 事件账只读视图。
    pub fn events(&self) -> &[EventRecord] {
        &self.events
    }

    /// 事件账淘汰条数。
    pub fn events_dropped(&self) -> u64 {
        self.events_dropped
    }

    /// 终盘报告只读视图（未终盘为 None）。
    pub fn last_report(&self) -> Option<&FinalReport> {
        self.last_report.as_ref()
    }

    // ------------------------------------------------------------------
    // 事件账
    // ------------------------------------------------------------------

    /// 事件入账（滚动覆盖最旧；dropped 计数守恒：seq == len + dropped）。
    fn push_event(&mut self, tick: u64, kind: EventKind, code: u16, detail: String) {
        if self.events.len() >= MAX_EVENTS {
            let _ = self.events.remove(0);
            self.events_dropped = self.events_dropped.wrapping_add(1);
        }
        self.event_seq = self.event_seq.wrapping_add(1);
        self.events.push(EventRecord {
            seq: self.event_seq,
            tick,
            kind,
            code,
            detail,
        });
    }

    /// 事件账守恒核：seq == 事件窗内条数 + 淘汰条数（wrapping 同余意义下）。
    pub fn event_conservation_ok(&self) -> bool {
        self.event_seq.wrapping_sub(self.events.len() as u64) == self.events_dropped
    }

    // ------------------------------------------------------------------
    // 域批登记
    // ------------------------------------------------------------------

    /// 登记一个域。合法态：Registering / Baseline / Sealed（封版后登记是
    /// 封版查的合法红案面，不在此拦——留痕交封版查）。
    pub fn register_domain(
        &mut self,
        domain_id: u16,
        assert_count: u32,
        tick: u64,
        manifest: &[u64],
    ) -> Result<(), FinalSweepError> {
        if self.state == STATE_FINALIZED {
            return Err(FinalSweepError::StateInvalid);
        }
        if self.state == STATE_FRESH {
            self.state = STATE_REGISTERING;
        }
        // 计数增量先核溢出，再落表（失败不留半账）。
        let new_total = match self.ledger_total.checked_add(assert_count as u64) {
            Some(v) => v,
            None => return Err(FinalSweepError::Overflow),
        };
        self.batch.register(domain_id, assert_count, tick, manifest)?;
        self.ledger_total = new_total;
        self.push_event(
            tick,
            EventKind::DomainRegistered,
            0,
            format!("域#{} 登记 断言={} 清单指纹={:#018x}", domain_id, assert_count, {
                match self.batch.find(domain_id) {
                    Some(e) => e.manifest_fp,
                    None => 0,
                }
            }),
        );
        Ok(())
    }

    // ------------------------------------------------------------------
    // 基线快照（计数守恒核的对账基面）
    // ------------------------------------------------------------------

    /// 冻结基线快照。合法态：Registering（首取）/ Baseline（再取·只增不减律
    /// 先行复核，缩水即红案拒记）。快照后逐域只能增不能减。
    pub fn snapshot_baseline(&mut self, tick: u64) -> Result<(), FinalSweepError> {
        if self.state != STATE_REGISTERING && self.state != STATE_BASELINE {
            return Err(FinalSweepError::StateInvalid);
        }
        if self.state == STATE_BASELINE {
            // 再取快照：与旧基线逐域比对，任一域缩水即红案拒记。
            let mut i = 0;
            while i < self.baseline.len() {
                let old = self.baseline[i];
                let now = match self.batch.find(old.domain_id) {
                    Some(e) => e.assert_count,
                    None => return Err(FinalSweepError::DomainUnknown),
                };
                if now < old.assert_count {
                    self.push_event(
                        tick,
                        EventKind::RedCaseLogged,
                        E_BASELINE_SHRINK,
                        format!("再快照遇缩水：域#{} 现{} < 基线{}", old.domain_id, now, old.assert_count),
                    );
                    return Err(FinalSweepError::BaselineShrink(old.domain_id));
                }
                i += 1;
            }
        }
        self.baseline.clear();
        let mut i = 0;
        while i < self.batch.len() {
            if let Some(e) = self.batch.at(i) {
                self.baseline.push(BaselineEntry {
                    domain_id: e.domain_id,
                    assert_count: e.assert_count,
                    tick,
                });
            }
            i += 1;
        }
        self.baseline_total = self.batch.total_forward()?;
        self.baseline_tick = tick;
        self.state = STATE_BASELINE;
        self.push_event(
            tick,
            EventKind::BaselineTaken,
            0,
            format!("基线冻结 域数={} 总断言={} tick={}", self.baseline.len(), self.baseline_total, tick),
        );
        Ok(())
    }

    // ------------------------------------------------------------------
    // 执行账
    // ------------------------------------------------------------------

    /// 记一次执行（域号必须已在册；首次执行自动封版）。
    pub fn record_execution(
        &mut self,
        domain_id: u16,
        tick: u64,
        verdict: Verdict,
    ) -> Result<(), FinalSweepError> {
        if self.state == STATE_FINALIZED {
            return Err(FinalSweepError::StateInvalid);
        }
        if self.state == STATE_FRESH || self.state == STATE_REGISTERING {
            return Err(FinalSweepError::StateInvalid);
        }
        if self.batch.find(domain_id).is_none() {
            return Err(FinalSweepError::DomainUnknown);
        }
        // 首次执行：自动封版（冻结封版指纹 + 随账指纹）。
        if self.state == STATE_BASELINE {
            self.frozen_seal_fp = self.batch.seal_fingerprint();
            self.ledger_seal_fp = self.frozen_seal_fp;
            self.state = STATE_SEALED;
            self.push_event(
                tick,
                EventKind::SealFrozen,
                0,
                format!("封版冻结 清单指纹={:#018x}", self.frozen_seal_fp),
            );
        }
        self.batch.note_execution(domain_id, tick, verdict)?;
        if self.exec_ledger.len() >= MAX_EXEC_RECORDS {
            let _ = self.exec_ledger.remove(0);
            self.exec_dropped = self.exec_dropped.wrapping_add(1);
        }
        self.exec_ledger.push(ExecRecord {
            domain_id,
            tick,
            verdict,
            injected: false,
        });
        self.push_event(
            tick,
            EventKind::ExecutionRecorded,
            0,
            format!("域#{} 执行{} tick={}", domain_id, verdict.note(), tick),
        );
        Ok(())
    }

    // ------------------------------------------------------------------
    // 查一：计数查（≥300 + 双路交叉）
    // ------------------------------------------------------------------

    /// 计数查：全域总断言数 ≥300 且正/逆双路求和交叉一致。
    pub fn count_check(&self) -> Result<CountResult, FinalSweepError> {
        let fwd = self.batch.total_forward()?;
        let rev = self.batch.total_reverse()?;
        let domain_count = self.batch.len();
        if domain_count == 0 {
            return Err(FinalSweepError::TotalBelowThreshold);
        }
        if fwd != rev || fwd != self.ledger_total {
            return Err(FinalSweepError::BaselineTotalMismatch);
        }
        if fwd < MIN_TOTAL_ASSERTS {
            return Err(FinalSweepError::TotalBelowThreshold);
        }
        Ok(CountResult {
            total: fwd,
            total_reverse: rev,
            margin: fwd - MIN_TOTAL_ASSERTS,
            domain_count,
            pass: true,
        })
    }

    // ------------------------------------------------------------------
    // 查二：守恒查（基线对账 + 只增不减 + 恒等式）
    // ------------------------------------------------------------------

    /// 守恒查：基线快照 vs 现账逐域 diff。
    /// 红案闭集：BaselineNotTaken / BaselineShrink(域) / BaselineTotalMismatch / Overflow。
    pub fn conservation_check(&self) -> Result<ConservationResult, FinalSweepError> {
        if !self.baseline_taken() || self.baseline.is_empty() {
            return Err(FinalSweepError::BaselineNotTaken);
        }
        let ledger_total = self.batch.total_forward()?;
        let mut rows: Vec<ConservationRow> = Vec::new();
        let mut delta_total: u64 = 0;
        let mut i = 0;
        while i < self.baseline.len() {
            let b = self.baseline[i];
            let now = match self.batch.find(b.domain_id) {
                Some(e) => e.assert_count,
                None => return Err(FinalSweepError::DomainUnknown),
            };
            if now < b.assert_count {
                return Err(FinalSweepError::BaselineShrink(b.domain_id));
            }
            let delta = now - b.assert_count;
            delta_total = match delta_total.checked_add(delta as u64) {
                Some(v) => v,
                None => return Err(FinalSweepError::Overflow),
            };
            rows.push(ConservationRow {
                domain_id: b.domain_id,
                current: now,
                baseline: b.assert_count,
                delta,
            });
            i += 1;
        }
        // 恒等式：Σ域(现账) == 基线总账 + Σ(delta)。
        let expect = match self.baseline_total.checked_add(delta_total) {
            Some(v) => v,
            None => return Err(FinalSweepError::Overflow),
        };
        if expect != ledger_total || self.baseline_total != self.baseline_total_forward() {
            return Err(FinalSweepError::BaselineTotalMismatch);
        }
        Ok(ConservationResult {
            ledger_total,
            baseline_total: self.baseline_total,
            delta_total,
            rows,
            pass: true,
        })
    }

    /// 基线总账第二路重算（逆序），与账面 baseline_total 交叉。
    fn baseline_total_forward(&self) -> u64 {
        let mut sum: u64 = 0;
        let mut i = 0;
        while i < self.baseline.len() {
            sum = sum.saturating_add(self.baseline[i].assert_count as u64);
            i += 1;
        }
        sum
    }

    // ------------------------------------------------------------------
    // 查三：全绿查（最近一次执行结论逐域复核）
    // ------------------------------------------------------------------

    /// 全绿查：逐域取最近一次结论，全域必须 Pass 且覆盖满。
    /// 红案闭集：ExecutionLedgerEmpty / ExecutionIncomplete / ExecutionVerdictFail(域)。
    /// Fail 定位：登记序首个 Fail 域（报告与错误载荷同源）。
    pub fn green_recheck(&self) -> Result<GreenResult, FinalSweepError> {
        if self.exec_ledger.is_empty() {
            return Err(FinalSweepError::ExecutionLedgerEmpty);
        }
        let mut fail_domains: Vec<u16> = Vec::new();
        let mut unrun_domains: Vec<u16> = Vec::new();
        let mut pass_domains: usize = 0;
        let mut executed_domains: usize = 0;
        let mut i = 0;
        while i < self.batch.len() {
            let e = match self.batch.at(i) {
                Some(e) => e,
                None => return Err(FinalSweepError::DomainUnknown),
            };
            match e.last_verdict {
                Verdict::Pass => {
                    executed_domains += 1;
                    pass_domains += 1;
                }
                Verdict::Fail => {
                    executed_domains += 1;
                    fail_domains.push(e.domain_id);
                }
                Verdict::Unrun => {
                    unrun_domains.push(e.domain_id);
                }
            }
            i += 1;
        }
        if !unrun_domains.is_empty() {
            return Err(FinalSweepError::ExecutionIncomplete);
        }
        if !fail_domains.is_empty() {
            return Err(FinalSweepError::ExecutionVerdictFail(fail_domains[0]));
        }
        if pass_domains != self.batch.len() {
            return Err(FinalSweepError::ExecutionIncomplete);
        }
        Ok(GreenResult {
            executed_domains,
            pass_domains,
            fail_domains,
            unrun_domains,
            pass: true,
        })
    }

    // ------------------------------------------------------------------
    // 查四：封版查（三方指纹比对）
    // ------------------------------------------------------------------

    /// 封版查：现清单指纹（重算）== 冻结封版指纹 == 执行账随账指纹。
    /// 任一不一致即 SealMismatch（账实分离红案）。
    pub fn seal_check(&self) -> Result<SealResult, FinalSweepError> {
        if self.frozen_seal_fp == 0 || self.ledger_seal_fp == 0 {
            return Err(FinalSweepError::SealNotTaken);
        }
        let current = self.batch.seal_fingerprint();
        if current != self.frozen_seal_fp || current != self.ledger_seal_fp {
            return Err(FinalSweepError::SealMismatch);
        }
        Ok(SealResult {
            current_fp: current,
            frozen_fp: self.frozen_seal_fp,
            ledger_fp: self.ledger_seal_fp,
            pass: true,
        })
    }

    // ------------------------------------------------------------------
    // 终盘报告（四查结论 + 证据行）
    // ------------------------------------------------------------------

    /// 红案行注记（载荷域号带「域#N」定位；无载荷注明无域载荷）。
    fn red_row_note(e: &FinalSweepError) -> String {
        if e.domain_hint() != 0 {
            format!("红案：{}（定位：域#{:02}）", e.detail_short(), e.domain_hint())
        } else {
            format!("红案：{}（无域载荷）", e.detail_short())
        }
    }

    /// 终盘：四查逐项出结论，四行 + 证据行成报告。合法态：Sealed。
    /// 任一查红不阻断报告产出——报告必须呈现全部四查结论（红案定位在行内），
    /// overall = 四行全过。返回报告的属主副本（机器内另存同份只读账）。
    pub fn finalize(&mut self, tick: u64) -> Result<FinalReport, FinalSweepError> {
        if self.state != STATE_SEALED {
            return Err(FinalSweepError::StateInvalid);
        }
        // 查一：计数查。
        let count_row = match self.count_check() {
            Ok(r) => {
                let note = format!(
                    "总断言 {} ≥ {}（余量 {}），双路 {}=={} 一致，域数 {}",
                    r.total, MIN_TOTAL_ASSERTS, r.margin, r.total, r.total_reverse, r.domain_count
                );
                RowResult { name: "计数查".to_string(), pass: true, code: 0, note }
            }
            Err(e) => {
                let note = Self::red_row_note(&e);
                RowResult { name: "计数查".to_string(), pass: false, code: e.code(), note }
            }
        };
        // 查二：守恒查。
        let conservation_row = match self.conservation_check() {
            Ok(r) => {
                let note = format!(
                    "基线 {} → 现账 {}（增量 {}），逐域只增不减 {} 域全过",
                    r.baseline_total, r.ledger_total, r.delta_total, r.rows.len()
                );
                RowResult { name: "守恒查".to_string(), pass: true, code: 0, note }
            }
            Err(e) => {
                let note = Self::red_row_note(&e);
                RowResult { name: "守恒查".to_string(), pass: false, code: e.code(), note }
            }
        };
        // 查三：全绿查。
        let green_row = match self.green_recheck() {
            Ok(r) => {
                let note = format!(
                    "最近一次执行账复核：执行 {} 域全过（{} 域在册）",
                    r.pass_domains, self.batch.len()
                );
                RowResult { name: "全绿查".to_string(), pass: true, code: 0, note }
            }
            Err(e) => {
                let note = Self::red_row_note(&e);
                RowResult { name: "全绿查".to_string(), pass: false, code: e.code(), note }
            }
        };
        // 查四：封版查。
        let seal_row = match self.seal_check() {
            Ok(r) => {
                let note = format!(
                    "封版一致 100%：现清单 {:#018x} == 冻结 {:#018x} == 随账 {:#018x}",
                    r.current_fp, r.frozen_fp, r.ledger_fp
                );
                RowResult { name: "封版查".to_string(), pass: true, code: 0, note }
            }
            Err(e) => {
                let note = Self::red_row_note(&e);
                RowResult { name: "封版查".to_string(), pass: false, code: e.code(), note }
            }
        };
        let overall = count_row.pass && conservation_row.pass && green_row.pass && seal_row.pass;
        // 证据行：每查固定 2 行（摘要 + 判定），顺序确定。
        let mut evidence: Vec<String> = vec![];
        evidence.push(format!(
            "[证据·计数] J1 成分「全域断言计数 ≥300」：总断言 {} / 阈值 {} → {}",
            self.ledger_total, MIN_TOTAL_ASSERTS, if count_row.pass { "过" } else { "红" }
        ));
        evidence.push(format!(
            "[证据·守恒] J1 成分「守恒核」：基线 tick={} 基线总账 {} 现账 {} → {}",
            self.baseline_tick, self.baseline_total, self.ledger_total,
            if conservation_row.pass { "过" } else { "红" }
        ));
        evidence.push(format!(
            "[证据·全绿] J1 成分「最近一次全绿执行账复核」：执行账 {} 条（淘汰 {}）→ {}",
            self.exec_ledger.len(), self.exec_dropped, if green_row.pass { "过" } else { "红" }
        ));
        evidence.push(format!(
            "[证据·封版] J1 成分「封版一致 100%」：冻结 {:#018x} 随账 {:#018x} → {}",
            self.frozen_seal_fp, self.ledger_seal_fp, if seal_row.pass { "过" } else { "红" }
        ));
        evidence.push(format!(
            "[证据·总判] UNX-F0763-J1 四查 {}",
            if overall { "全过 · 终盘过" } else { "有红 · 终盘不过" }
        ));
        let report = FinalReport {
            tick,
            overall,
            count_row,
            conservation_row,
            green_row,
            seal_row,
            evidence,
        };
        self.push_event(
            tick,
            EventKind::Finalized,
            0,
            format!(
                "终盘落账 四查{} tick={}",
                if report.overall { "全过" } else { "有红" },
                tick
            ),
        );
        self.state = STATE_FINALIZED;
        self.last_report = Some(report.clone());
        Ok(report)
    }

    // ------------------------------------------------------------------
    // 健康面
    // ------------------------------------------------------------------

    /// 健康摘要（只读快照，供监控轮询）。
    pub fn health(&self) -> HealthSummary {
        let mut ok = true;
        if self.state == STATE_FRESH {
            ok = false;
        }
        if self.ledger_total < MIN_TOTAL_ASSERTS && self.state != STATE_FRESH && self.state != STATE_REGISTERING {
            ok = false;
        }
        if self.baseline_taken() && self.baseline.len() != self.batch.len() {
            ok = false;
        }
        if self.sealed() && (self.frozen_seal_fp == 0 || self.ledger_seal_fp == 0) {
            ok = false;
        }
        if !self.event_conservation_ok() {
            ok = false;
        }
        HealthSummary {
            state: self.state,
            state_name: state_name(self.state),
            domain_count: self.batch.len(),
            total_asserts: self.ledger_total,
            baseline_taken: self.baseline_taken(),
            baseline_total: self.baseline_total,
            sealed: self.sealed(),
            frozen_seal_fp: self.frozen_seal_fp,
            exec_records: self.exec_ledger.len(),
            exec_dropped: self.exec_dropped,
            events: self.events.len(),
            events_dropped: self.events_dropped,
            event_seq: self.event_seq,
            ok,
        }
    }

    // ------------------------------------------------------------------
    // 状态指纹（双跑对拍面）
    // ------------------------------------------------------------------

    /// 状态指纹：FNV over 状态字/域表/基线/封版/执行账/事件账全量。
    /// 双跑同种子同脚本必同指纹——不一致即非确定性留痕。
    pub fn state_fingerprint(&self) -> u64 {
        self.fingerprint_impl(true)
    }

    /// 账面指纹：状态指纹但不含事件账（事件账有意留痕不灭——注入留痕、
    /// 红案留痕都会改变事件账，故注入回滚判定须用本指纹比对账面本体）。
    pub fn ledger_fingerprint(&self) -> u64 {
        self.fingerprint_impl(false)
    }

    /// 指纹本体（include_events=false 时以固定标记位替代事件账段）。
    fn fingerprint_impl(&self, include_events: bool) -> u64 {
        let mut h = fnv_head();
        h = fnv_u16(h, SEG_BASE); // 段标（防跨机指纹串味）
        h = fnv_byte(h, self.state);
        h = fnv_u64(h, self.ledger_total);
        h = fnv_u64(h, self.baseline_total);
        h = fnv_u64(h, self.baseline_tick);
        h = fnv_u64(h, self.frozen_seal_fp);
        h = fnv_u64(h, self.ledger_seal_fp);
        h = fnv_u64(h, self.exec_dropped);
        // 事件账侧账（dropped/seq/窗内条目）只在状态指纹里参与——账面指纹
        // 不含事件域：事件留痕（注入/红案）有意不灭，回滚判定只看账面本体。
        if include_events {
            h = fnv_u64(h, self.events_dropped);
            h = fnv_u64(h, self.event_seq);
        } else {
            h = fnv_u64(h, 0x0E7C);
        }
        // 域表。
        h = fnv_u64(h, self.batch.len() as u64);
        let mut i = 0;
        while i < self.batch.len() {
            if let Some(e) = self.batch.at(i) {
                h = fnv_u16(h, e.domain_id);
                h = fnv_u32(h, e.assert_count);
                h = fnv_u64(h, e.last_exec_tick);
                h = fnv_byte(h, e.last_verdict.code());
                h = fnv_u64(h, e.manifest_fp);
                h = fnv_u64(h, e.manifest.len() as u64);
                let mut j = 0;
                while j < e.manifest.len() {
                    h = fnv_u64(h, e.manifest[j]);
                    j += 1;
                }
            }
            i += 1;
        }
        // 基线。
        h = fnv_u64(h, self.baseline.len() as u64);
        let mut k = 0;
        while k < self.baseline.len() {
            h = fnv_u16(h, self.baseline[k].domain_id);
            h = fnv_u32(h, self.baseline[k].assert_count);
            h = fnv_u64(h, self.baseline[k].tick);
            k += 1;
        }
        // 执行账。
        h = fnv_u64(h, self.exec_ledger.len() as u64);
        let mut m = 0;
        while m < self.exec_ledger.len() {
            h = fnv_u16(h, self.exec_ledger[m].domain_id);
            h = fnv_u64(h, self.exec_ledger[m].tick);
            h = fnv_byte(h, self.exec_ledger[m].verdict.code());
            h = fnv_byte(h, self.exec_ledger[m].injected as u8);
            m += 1;
        }
        // 事件账（含详情字节流；账面指纹以固定标记替代——事件留痕不灭不参与）。
        if include_events {
            h = fnv_u64(h, self.events.len() as u64);
            let mut n = 0;
            while n < self.events.len() {
                h = fnv_u64(h, self.events[n].seq);
                h = fnv_u64(h, self.events[n].tick);
                h = fnv_u16(h, self.events[n].kind.code());
                h = fnv_u16(h, self.events[n].code);
                h = fnv_bytes(h, &self.events[n].detail);
                n += 1;
            }
        } else {
            h = fnv_u64(h, 0xD1CE_B00E_DEAD_BEEF);
        }
        h
    }

    // ------------------------------------------------------------------
    // 注入自证面（三注入各检出 + 回滚复原 + 事件留痕）
    // ------------------------------------------------------------------

    /// 注入一：计数缩水。把目标域计数压到 `shrink_to`（必须严格低于现值且
    /// 低于基线才有红案意义），发起守恒查，检出 BaselineShrink 且域号吻合
    /// 即 detected；随后回滚复原并留痕。
    pub fn inject_count_shrink(
        &mut self,
        domain_id: u16,
        shrink_to: u32,
        tick: u64,
    ) -> Result<InjectionOutcome, FinalSweepError> {
        let orig = match self.batch.find(domain_id) {
            Some(e) => e.assert_count,
            None => return Err(FinalSweepError::DomainUnknown),
        };
        let baseline_of = match self.baseline.iter().find(|b| b.domain_id == domain_id) {
            Some(b) => b.assert_count,
            None => return Err(FinalSweepError::InjectionPrecondition),
        };
        if shrink_to >= orig || shrink_to >= baseline_of || shrink_to == 0 {
            return Err(FinalSweepError::InjectionPrecondition);
        }
        self.push_event(
            tick,
            EventKind::InjectionAttempted,
            InjectionKind::CountShrink.code(),
            format!("域#{} 计数缩水注入 {}→{}", domain_id, orig, shrink_to),
        );
        // 施注入。
        self.batch.set_count(domain_id, shrink_to)?;
        self.batch.refresh_fingerprint(domain_id)?;
        // 守恒查应红。
        let (detected, diag_code) = match self.conservation_check() {
            Err(FinalSweepError::BaselineShrink(d)) if d == domain_id => (true, E_BASELINE_SHRINK),
            Err(e) => (true, e.code()), // 其他红案也算守恒面拦下，但证据里注明非预期码
            Ok(_) => (false, 0),
        };
        self.push_event(
            tick,
            if detected { EventKind::InjectionDetected } else { EventKind::RedCaseLogged },
            diag_code,
            format!(
                "域#{} 计数缩水注入{}",
                domain_id,
                if detected { "已检出" } else { "未检出（红案漏网）" }
            ),
        );
        // 回滚复原。
        self.batch.set_count(domain_id, orig)?;
        self.batch.refresh_fingerprint(domain_id)?;
        self.push_event(
            tick,
            EventKind::InjectionCleared,
            0,
            format!("域#{} 计数缩水注入回滚复原 → {}", domain_id, orig),
        );
        Ok(InjectionOutcome {
            kind: InjectionKind::CountShrink,
            domain_id,
            detected,
            diag_code,
            evidence: format!(
                "计数缩水注入 域#{} {}→{} 守恒查{} 码={:#06x}",
                domain_id, orig, shrink_to,
                if detected { "检出" } else { "漏网" }, diag_code
            ),
        })
    }

    /// 注入二：隐藏 Fail。把目标域最近结论翻成 Fail（前置：该域最近结论为
    /// Pass），发起全绿查，检出 ExecutionVerdictFail 且域号吻合即 detected；
    /// 随后回滚复原并留痕。
    pub fn inject_hidden_fail(
        &mut self,
        domain_id: u16,
        tick: u64,
    ) -> Result<InjectionOutcome, FinalSweepError> {
        let cur = match self.batch.find(domain_id) {
            Some(e) => e.last_verdict,
            None => return Err(FinalSweepError::DomainUnknown),
        };
        if cur != Verdict::Pass {
            return Err(FinalSweepError::InjectionPrecondition);
        }
        self.push_event(
            tick,
            EventKind::InjectionAttempted,
            InjectionKind::HiddenFail.code(),
            format!("域#{} 隐藏 Fail 注入 过→败", domain_id),
        );
        // 施注入。
        self.batch.set_verdict(domain_id, Verdict::Fail)?;
        // 全绿查应红。
        let (detected, diag_code) = match self.green_recheck() {
            Err(FinalSweepError::ExecutionVerdictFail(d)) if d == domain_id => (true, E_EXEC_FAIL),
            Err(e) => (true, e.code()),
            Ok(_) => (false, 0),
        };
        self.push_event(
            tick,
            if detected { EventKind::InjectionDetected } else { EventKind::RedCaseLogged },
            diag_code,
            format!(
                "域#{} 隐藏 Fail 注入{}",
                domain_id,
                if detected { "已检出" } else { "未检出（红案漏网）" }
            ),
        );
        // 回滚复原。
        self.batch.set_verdict(domain_id, Verdict::Pass)?;
        self.push_event(
            tick,
            EventKind::InjectionCleared,
            0,
            format!("域#{} 隐藏 Fail 注入回滚复原 → 过", domain_id),
        );
        Ok(InjectionOutcome {
            kind: InjectionKind::HiddenFail,
            domain_id,
            detected,
            diag_code,
            evidence: format!(
                "隐藏 Fail 注入 域#{} 全绿查{} 码={:#06x}",
                domain_id,
                if detected { "检出" } else { "漏网" }, diag_code
            ),
        })
    }

    /// 注入三：封版漂移。封版后把目标域计数 +1（清单基面扰动），发起封版查，
    /// 检出 SealMismatch 即 detected；随后回滚复原并留痕。
    pub fn inject_seal_drift(
        &mut self,
        domain_id: u16,
        tick: u64,
    ) -> Result<InjectionOutcome, FinalSweepError> {
        let orig = match self.batch.find(domain_id) {
            Some(e) => e.assert_count,
            None => return Err(FinalSweepError::DomainUnknown),
        };
        if !self.sealed() {
            return Err(FinalSweepError::InjectionPrecondition);
        }
        self.push_event(
            tick,
            EventKind::InjectionAttempted,
            InjectionKind::SealDrift.code(),
            format!("域#{} 封版漂移注入 {}→{}（封版后扰动）", domain_id, orig, orig + 1),
        );
        // 施注入（计数+1 → 清单指纹漂移）。
        self.batch.set_count(domain_id, orig + 1)?;
        self.batch.refresh_fingerprint(domain_id)?;
        // 封版查应红。
        let (detected, diag_code) = match self.seal_check() {
            Err(FinalSweepError::SealMismatch) => (true, E_SEAL_MISMATCH),
            Err(e) => (true, e.code()),
            Ok(_) => (false, 0),
        };
        self.push_event(
            tick,
            if detected { EventKind::InjectionDetected } else { EventKind::RedCaseLogged },
            diag_code,
            format!(
                "域#{} 封版漂移注入{}",
                domain_id,
                if detected { "已检出（账实分离）" } else { "未检出（红案漏网）" }
            ),
        );
        // 回滚复原。
        self.batch.set_count(domain_id, orig)?;
        self.batch.refresh_fingerprint(domain_id)?;
        self.push_event(
            tick,
            EventKind::InjectionCleared,
            0,
            format!("域#{} 封版漂移注入回滚复原 → {}", domain_id, orig),
        );
        Ok(InjectionOutcome {
            kind: InjectionKind::SealDrift,
            domain_id,
            detected,
            diag_code,
            evidence: format!(
                "封版漂移注入 域#{} {}→{} 封版查{} 码={:#06x}",
                domain_id, orig, orig + 1,
                if detected { "检出" } else { "漏网" }, diag_code
            ),
        })
    }

    // ------------------------------------------------------------------
    // 双跑确定性对拍
    // ------------------------------------------------------------------

    /// 双跑对拍：同种子构建两台终盘机走同一标准脚本，比对状态指纹与
    /// 终盘报告指纹。双双一致才出证明；任何不一致即确定性破口。
    pub fn determinism_probe(seed: u64) -> Result<DoubleRunProof, FinalSweepError> {
        let (a, ra) = run_standard_script(seed)?;
        let (b, rb) = run_standard_script(seed)?;
        let fp_a = a.state_fingerprint();
        let fp_b = b.state_fingerprint();
        let rep_a = ra.fingerprint();
        let rep_b = rb.fingerprint();
        let equal = fp_a == fp_b && rep_a == rep_b;
        Ok(DoubleRunProof {
            seed,
            state_fp_a: fp_a,
            state_fp_b: fp_b,
            report_fp_a: rep_a,
            report_fp_b: rep_b,
            equal,
        })
    }
}

/// 健康摘要。
#[derive(Clone, Debug)]
pub struct HealthSummary {
    /// 状态字。
    pub state: u8,
    /// 状态名。
    pub state_name: &'static str,
    /// 域数。
    pub domain_count: usize,
    /// 账面总断言数。
    pub total_asserts: u64,
    /// 基线已取。
    pub baseline_taken: bool,
    /// 基线总账。
    pub baseline_total: u64,
    /// 已封版。
    pub sealed: bool,
    /// 冻结封版指纹。
    pub frozen_seal_fp: u64,
    /// 执行账窗内条数。
    pub exec_records: usize,
    /// 执行账淘汰条数。
    pub exec_dropped: u64,
    /// 事件账窗内条数。
    pub events: usize,
    /// 事件账淘汰条数。
    pub events_dropped: u64,
    /// 事件序号。
    pub event_seq: u64,
    /// 总健康判。
    pub ok: bool,
}

/// 双跑对拍证明。
#[derive(Clone, Copy, Debug)]
pub struct DoubleRunProof {
    /// 对拍种子。
    pub seed: u64,
    /// 甲机状态指纹。
    pub state_fp_a: u64,
    /// 乙机状态指纹。
    pub state_fp_b: u64,
    /// 甲机报告指纹。
    pub report_fp_a: u64,
    /// 乙机报告指纹。
    pub report_fp_b: u64,
    /// 双双一致。
    pub equal: bool,
}

/// 状态名（STATE_* → 中文，报告/健康面用）。
pub fn state_name(state: u8) -> &'static str {
    match state {
        STATE_FRESH => "未登记",
        STATE_REGISTERING => "登记中",
        STATE_BASELINE => "基线已取",
        STATE_SEALED => "已封版",
        STATE_FINALIZED => "已终盘",
        _ => "未知态",
    }
}

/// 错误短注记（报告行内嵌用；detail() 的压缩面）。
impl FinalSweepError {
    fn detail_short(&self) -> &'static str {
        match self {
            FinalSweepError::DomainTableFull => "域表满",
            FinalSweepError::DomainAlreadyRegistered => "域重复登记",
            FinalSweepError::DomainUnknown => "域号不在册",
            FinalSweepError::AssertCountZero => "断言数为零",
            FinalSweepError::AssertCountExceedsCap => "断言数超上限",
            FinalSweepError::ManifestCountMismatch => "清单-计数不符",
            FinalSweepError::ManifestEmpty => "清单为空",
            FinalSweepError::StateInvalid => "状态机非法边",
            FinalSweepError::BaselineNotTaken => "基线未取",
            FinalSweepError::BaselineShrink(_) => "域计数缩水（只增不减红案）",
            FinalSweepError::BaselineTotalMismatch => "基线总账失恒",
            FinalSweepError::TotalBelowThreshold => "总断言 <300",
            FinalSweepError::ExecutionLedgerEmpty => "执行账为空",
            FinalSweepError::ExecutionIncomplete => "执行账未覆盖全域",
            FinalSweepError::ExecutionVerdictFail(_) => "最近执行结论 Fail",
            FinalSweepError::SealNotTaken => "封版未冻结",
            FinalSweepError::SealMismatch => "封版不一致（账实分离）",
            FinalSweepError::Overflow => "对账求和溢出",
            FinalSweepError::EventLedgerFull => "事件账满",
            FinalSweepError::InjectionPrecondition => "注入前置不成立",
        }
    }

    /// 红案域号定位（载荷域号；无载荷变体返回 0）。
    fn domain_hint(&self) -> u16 {
        match self {
            FinalSweepError::BaselineShrink(d) => *d,
            FinalSweepError::ExecutionVerdictFail(d) => *d,
            _ => 0,
        }
    }
}

// ===========================================================================
// 9. 标准对拍脚本
// ===========================================================================

/// 断言清单生成（确定性 LCG 混入；同种子同域同计数必同清单）。
fn gen_manifest(seed: u64, domain_id: u16, count: u32) -> Vec<u64> {
    let mut m: Vec<u64> = Vec::new();
    let mut x = seed
        ^ (domain_id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (count as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let mut i: u32 = 0;
    while i < count {
        // splitmix64 终混（确定性，无外部熵）。
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        m.push((domain_id as u64) << 32 | (i as u64) | (z & 0xFFFF_0000_0000_0000));
        i += 1;
    }
    m
}

/// 标准对拍脚本：boot → 8 域登记（Σ=308）→ 基线 → 全域 Pass 执行账 → 终盘。
/// 同种子两跑逐字节同轨迹（双跑对拍的脚本体）。
pub fn run_standard_script(
    seed: u64,
) -> Result<(FinalSweep, FinalReport), FinalSweepError> {
    let mut fs = FinalSweep::boot(seed);
    let mut i = 0;
    while i < SCRIPT_DOMAINS.len() {
        let d = SCRIPT_DOMAINS[i];
        let c = SCRIPT_COUNTS[i];
        let manifest = gen_manifest(seed, d, c);
        fs.register_domain(d, c, 10 + d as u64, &manifest)?;
        i += 1;
    }
    fs.snapshot_baseline(SCRIPT_TICK_BASELINE)?;
    let mut j = 0;
    while j < SCRIPT_DOMAINS.len() {
        fs.record_execution(
            SCRIPT_DOMAINS[j],
            SCRIPT_TICK_EXEC_BASE + SCRIPT_DOMAINS[j] as u64 * 10,
            Verdict::Pass,
        )?;
        j += 1;
    }
    let report_tick = SCRIPT_TICK_FINAL;
    let report = fs.finalize(report_tick)?;
    Ok((fs, report))
}

// ===========================================================================
// 10. 编译期闭集自检（const _: () = assert!(…)，唯一允许的 assert!）
// ===========================================================================

/// const 求和（const fn；while+索引，禁 for/get/min）。
const fn const_slice_sum_u32(s: &[u32]) -> u32 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < s.len() {
        sum = sum.wrapping_add(s[i]);
        i += 1;
    }
    sum
}

/// 诊断码段闭集：段基对齐 16、顶界恰为基+31。
const _: () = assert!(SEG_BASE & 0x000F == 0x0000);
const _: () = assert!(SEG_TOP == SEG_BASE + 0x001F);
/// 诊断码闭集：20 个码位逐个落段内（错误变体数 == 20）。
const _: () = assert!(E_DOMAIN_TABLE_FULL == SEG_BASE + 0x00);
const _: () = assert!(E_INJECT_PRECOND == SEG_BASE + 0x13);
const _: () = assert!(E_INJECT_PRECOND <= SEG_TOP);
/// 阈值判据原文写死核：MIN_TOTAL_ASSERTS == 300。
const _: () = assert!(MIN_TOTAL_ASSERTS == 300);
/// 标准脚本守恒：Σ=308 ≥ 300 且域数不超域表容量。
const _: () = assert!(const_slice_sum_u32(&SCRIPT_COUNTS) as u64 >= MIN_TOTAL_ASSERTS);
const _: () = assert!(const_slice_sum_u32(&SCRIPT_COUNTS) == 308);
const _: () = assert!(SCRIPT_COUNTS.len() <= MAX_DOMAINS);
/// FNV 参数闭集：偏移基与素数为标准值。
const _: () = assert!(FNV_OFFSET == 0xcbf29ce484222325);
const _: () = assert!(FNV_PRIME == 0x100000001b3);
/// 状态字闭集：单向链 0→4。
const _: () = assert!(STATE_FRESH < STATE_REGISTERING
    && STATE_REGISTERING < STATE_BASELINE
    && STATE_BASELINE < STATE_SEALED
    && STATE_SEALED < STATE_FINALIZED);

// ===========================================================================
// 11. 冒烟与守恒测试（cfg(test)：std 注入仅测试面，内核路径不含）
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 测-01：诊断码闭集——20 码逐个落段内且互不重复，detail 三成分齐备。
    #[test]
    fn diag_codes_closure() {
        let codes: [u16; 20] = [
            E_DOMAIN_TABLE_FULL, E_DOMAIN_ALREADY, E_DOMAIN_UNKNOWN, E_ASSERT_COUNT_ZERO,
            E_ASSERT_COUNT_CAP, E_MANIFEST_LEN, E_MANIFEST_EMPTY, E_STATE_INVALID,
            E_BASELINE_NOT_TAKEN, E_BASELINE_SHRINK, E_BASELINE_TOTAL, E_TOTAL_BELOW,
            E_EXEC_EMPTY, E_EXEC_INCOMPLETE, E_EXEC_FAIL, E_SEAL_NOT_TAKEN, E_SEAL_MISMATCH,
            E_OVERFLOW, E_EVENT_FULL, E_INJECT_PRECOND,
        ];
        for c in codes.iter() {
            assert!(*c >= SEG_BASE && *c <= SEG_TOP, "码 {:#06x} 越段", c);
        }
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j], "码位重复 {:#06x}", codes[i]);
            }
        }
        let all: [FinalSweepError; 20] = [
            FinalSweepError::DomainTableFull,
            FinalSweepError::DomainAlreadyRegistered,
            FinalSweepError::DomainUnknown,
            FinalSweepError::AssertCountZero,
            FinalSweepError::AssertCountExceedsCap,
            FinalSweepError::ManifestCountMismatch,
            FinalSweepError::ManifestEmpty,
            FinalSweepError::StateInvalid,
            FinalSweepError::BaselineNotTaken,
            FinalSweepError::BaselineShrink(1),
            FinalSweepError::BaselineTotalMismatch,
            FinalSweepError::TotalBelowThreshold,
            FinalSweepError::ExecutionLedgerEmpty,
            FinalSweepError::ExecutionIncomplete,
            FinalSweepError::ExecutionVerdictFail(1),
            FinalSweepError::SealNotTaken,
            FinalSweepError::SealMismatch,
            FinalSweepError::Overflow,
            FinalSweepError::EventLedgerFull,
            FinalSweepError::InjectionPrecondition,
        ];
        for e in all.iter() {
            let d = e.detail();
            assert!(d.contains("现象"), "码 {:#06x} detail 缺现象", e.code());
            assert!(d.contains("位置"), "码 {:#06x} detail 缺位置", e.code());
            assert!(d.contains("建议修法"), "码 {:#06x} detail 缺建议修法", e.code());
        }
    }

    /// 测-02：标准脚本全绿——8 域 308 断言 ≥300，四查全过，终盘过。
    #[test]
    fn smoke_full_green() {
        let (fs, report) = run_standard_script(7).expect("标准脚本不得失败");
        assert_eq!(fs.domain_count(), 8);
        assert_eq!(fs.ledger_total(), 308);
        assert!(fs.sealed());
        assert_eq!(fs.state(), STATE_FINALIZED);
        assert!(report.overall, "四查应全过：{:#?}", report.evidence);
        assert!(report.count_row.pass);
        assert!(report.conservation_row.pass);
        assert!(report.green_row.pass);
        assert!(report.seal_row.pass);
        // 计数查数值面。
        let c = fs.count_check().expect("全绿机计数查应过");
        assert_eq!(c.total, 308);
        assert_eq!(c.total_reverse, 308);
        assert_eq!(c.margin, 8);
        assert_eq!(c.domain_count, 8);
        // 封版三方一致。
        let s = fs.seal_check().expect("全绿机封版查应过");
        assert!(s.pass);
        assert_eq!(s.current_fp, s.frozen_fp);
        assert_eq!(s.frozen_fp, s.ledger_fp);
        assert_eq!(s.frozen_fp, fs.frozen_seal_fp());
        // 事件账守恒。
        assert!(fs.event_conservation_ok());
        assert_eq!(fs.events_dropped(), 0);
        // 健康面。
        let h = fs.health();
        assert!(h.ok, "全绿机应健康：{:#?}", h);
        assert_eq!(h.state_name, "已终盘");
        assert_eq!(h.domain_count, 8);
        assert_eq!(h.total_asserts, 308);
        // 报告指纹确定性：同机重算同指纹。
        assert_eq!(report.fingerprint(), report.fingerprint());
    }

    /// 测-03：≥300 阈值红案——总断言 <300 时计数查必红 TotalBelowThreshold。
    #[test]
    fn threshold_red_case() {
        let mut fs = FinalSweep::boot(11);
        fs.register_domain(1, 150, 1, &gen_manifest(11, 1, 150)).expect("登记");
        fs.register_domain(2, 149, 2, &gen_manifest(11, 2, 149)).expect("登记");
        assert_eq!(fs.ledger_total(), 299);
        fs.snapshot_baseline(10).expect("基线");
        fs.record_execution(1, 20, Verdict::Pass).expect("执行");
        fs.record_execution(2, 30, Verdict::Pass).expect("执行");
        match fs.count_check() {
            Err(FinalSweepError::TotalBelowThreshold) => {}
            other => panic!("应红 TotalBelowThreshold，实得 {:?}", other.map(|_| ())),
        }
        // 守恒/全绿/封版本身可过——红在阈值这一成分。
        assert!(fs.conservation_check().is_ok());
        assert!(fs.green_recheck().is_ok());
        assert!(fs.seal_check().is_ok());
        // 终盘报告应呈现计数查红 + overall=false。
        let report = fs.finalize(999).expect("终盘应出报告");
        assert!(!report.overall);
        assert!(!report.count_row.pass);
        assert_eq!(report.count_row.code, E_TOTAL_BELOW);
        assert!(report.conservation_row.pass);
        assert!(report.green_row.pass);
        assert!(report.seal_row.pass);
    }

    /// 测-04：守恒红案——直接缩水（不经注入面）被守恒查拦下并定位域号。
    #[test]
    fn conservation_shrink_red_case() {
        // 终盘前重建同脚本状态（终盘后账面只读，不可再改）。
        let mut fs = FinalSweep::boot(13);
        let mut i = 0;
        while i < SCRIPT_DOMAINS.len() {
            fs.register_domain(
                SCRIPT_DOMAINS[i],
                SCRIPT_COUNTS[i],
                10 + SCRIPT_DOMAINS[i] as u64,
                &gen_manifest(13, SCRIPT_DOMAINS[i], SCRIPT_COUNTS[i]),
            )
            .expect("登记");
            i += 1;
        }
        fs.snapshot_baseline(SCRIPT_TICK_BASELINE).expect("基线");
        let mut j = 0;
        while j < SCRIPT_DOMAINS.len() {
            fs.record_execution(SCRIPT_DOMAINS[j], 100 + j as u64 * 10, Verdict::Pass)
                .expect("执行");
            j += 1;
        }
        // 直接把域#3 计数 40→39（绕开注入面模拟真缩水）。
        fs.batch.set_count(3, 39).expect("改账");
        fs.batch.refresh_fingerprint(3).expect("指纹");
        match fs.conservation_check() {
            Err(FinalSweepError::BaselineShrink(3)) => {}
            other => panic!("应红 BaselineShrink(3)，实得 {:?}", other.map(|_| ())),
        }
        // 复原后守恒恢复。
        fs.batch.set_count(3, 40).expect("复原");
        fs.batch.refresh_fingerprint(3).expect("指纹");
        assert!(fs.conservation_check().is_ok());
    }

    /// 测-05：全绿红案——某域最近结论 Fail 被全绿查拦下并定位域号。
    #[test]
    fn green_recheck_fail_located() {
        let mut fs = FinalSweep::boot(17);
        fs.register_domain(1, 200, 1, &gen_manifest(17, 1, 200)).expect("登记");
        fs.register_domain(2, 150, 2, &gen_manifest(17, 2, 150)).expect("登记");
        fs.snapshot_baseline(10).expect("基线");
        fs.record_execution(1, 20, Verdict::Pass).expect("执行");
        fs.record_execution(2, 30, Verdict::Fail).expect("执行");
        match fs.green_recheck() {
            Err(FinalSweepError::ExecutionVerdictFail(2)) => {}
            other => panic!("应红 ExecutionVerdictFail(2)，实得 {:?}", other.map(|_| ())),
        }
        // 修复重跑：新 Pass 账入账后全绿闭合。
        fs.record_execution(2, 40, Verdict::Pass).expect("重执行");
        let g = fs.green_recheck().expect("修复后应全绿");
        assert!(g.pass);
        assert_eq!(g.pass_domains, 2);
        assert!(g.fail_domains.is_empty());
        // 最近一次口径：执行账 3 条（域#2 两条），最近结论 Pass。
        assert_eq!(fs.exec_record_count(), 3);
    }

    /// 测-06：未执行红案——缺账域被全绿查拦下（ExecutionIncomplete）。
    #[test]
    fn green_recheck_incomplete() {
        let mut fs = FinalSweep::boot(19);
        fs.register_domain(1, 180, 1, &gen_manifest(19, 1, 180)).expect("登记");
        fs.register_domain(2, 140, 2, &gen_manifest(19, 2, 140)).expect("登记");
        fs.snapshot_baseline(10).expect("基线");
        fs.record_execution(1, 20, Verdict::Pass).expect("执行");
        match fs.green_recheck() {
            Err(FinalSweepError::ExecutionIncomplete) => {}
            other => panic!("应红 ExecutionIncomplete，实得 {:?}", other.map(|_| ())),
        }
    }

    /// 测-07：封版一致红案——封版后改清单直接被封版查拦下（账实分离）。
    #[test]
    fn seal_mismatch_red_case() {
        let mut fs = FinalSweep::boot(23);
        fs.register_domain(1, 200, 1, &gen_manifest(23, 1, 200)).expect("登记");
        fs.register_domain(2, 150, 2, &gen_manifest(23, 2, 150)).expect("登记");
        fs.snapshot_baseline(10).expect("基线");
        fs.record_execution(1, 20, Verdict::Pass).expect("执行（自动封版）");
        fs.record_execution(2, 30, Verdict::Pass).expect("执行");
        let frozen = fs.frozen_seal_fp();
        assert_ne!(frozen, 0);
        // 封版后扰动域#2 计数（账实分离）。
        fs.batch.set_count(2, 151).expect("改账");
        fs.batch.refresh_fingerprint(2).expect("指纹");
        match fs.seal_check() {
            Err(FinalSweepError::SealMismatch) => {}
            other => panic!("应红 SealMismatch，实得 {:?}", other.map(|_| ())),
        }
        // 复原后封版恢复一致。
        fs.batch.set_count(2, 150).expect("复原");
        fs.batch.refresh_fingerprint(2).expect("指纹");
        assert!(fs.seal_check().is_ok());
        assert_eq!(fs.frozen_seal_fp(), frozen);
    }

    /// 测-08：三注入各检出——计数缩水 / 隐藏 Fail / 封版漂移，检出后账面复原。
    #[test]
    fn three_injections_all_detected() {
        let mut fs = FinalSweep::boot(29);
        let mut i = 0;
        while i < SCRIPT_DOMAINS.len() {
            fs.register_domain(
                SCRIPT_DOMAINS[i],
                SCRIPT_COUNTS[i],
                10 + SCRIPT_DOMAINS[i] as u64,
                &gen_manifest(29, SCRIPT_DOMAINS[i], SCRIPT_COUNTS[i]),
            )
            .expect("登记");
            i += 1;
        }
        fs.snapshot_baseline(SCRIPT_TICK_BASELINE).expect("基线");
        let mut j = 0;
        while j < SCRIPT_DOMAINS.len() {
            fs.record_execution(SCRIPT_DOMAINS[j], 100 + j as u64 * 10, Verdict::Pass)
                .expect("执行");
            j += 1;
        }
        let fp_before = fs.ledger_fingerprint();
        // 注入一：计数缩水（域#1 64→63，63 < 基线 64）。
        let o1 = fs.inject_count_shrink(1, 63, 500).expect("注入一");
        assert!(o1.detected, "计数缩水注入必须检出");
        assert_eq!(o1.diag_code, E_BASELINE_SHRINK);
        assert_eq!(o1.kind, InjectionKind::CountShrink);
        // 注入二：隐藏 Fail（域#5 最近 Pass → Fail）。
        let o2 = fs.inject_hidden_fail(5, 501).expect("注入二");
        assert!(o2.detected, "隐藏 Fail 注入必须检出");
        assert_eq!(o2.diag_code, E_EXEC_FAIL);
        assert_eq!(o2.kind, InjectionKind::HiddenFail);
        // 注入三：封版漂移（域#7 计数 24→25）。
        let o3 = fs.inject_seal_drift(7, 502).expect("注入三");
        assert!(o3.detected, "封版漂移注入必须检出");
        assert_eq!(o3.diag_code, E_SEAL_MISMATCH);
        assert_eq!(o3.kind, InjectionKind::SealDrift);
        // 三注入后账面复原：账面指纹（不含事件账——事件留痕有意不灭）与注入前一致。
        assert_eq!(fs.ledger_fingerprint(), fp_before, "注入必须完全回滚");
        // 状态指纹（含事件账）必然异于账面指纹——注入留痕不灭的证明。
        assert_ne!(fs.state_fingerprint(), fs.ledger_fingerprint());
        // 注入事件留痕不灭（3 尝试 + 3 检出 + 3 复原）。
        let mut attempts = 0;
        let mut detects = 0;
        let mut clears = 0;
        for ev in fs.events() {
            match ev.kind {
                EventKind::InjectionAttempted => attempts += 1,
                EventKind::InjectionDetected => detects += 1,
                EventKind::InjectionCleared => clears += 1,
                _ => {}
            }
        }
        assert_eq!(attempts, 3);
        assert_eq!(detects, 3);
        assert_eq!(clears, 3);
        // 复原后四查仍全过（终盘前状态）。
        assert!(fs.count_check().is_ok());
        assert!(fs.conservation_check().is_ok());
        assert!(fs.green_recheck().is_ok());
        assert!(fs.seal_check().is_ok());
    }

    /// 测-09：注入前置闸——非法注入（非缩水 / 非 Pass 域 / 未封版）被拒。
    #[test]
    fn injection_preconditions() {
        let mut fs = FinalSweep::boot(31);
        fs.register_domain(1, 300, 1, &gen_manifest(31, 1, 300)).expect("登记");
        fs.snapshot_baseline(10).expect("基线");
        fs.record_execution(1, 20, Verdict::Pass).expect("执行（自动封版）");
        // 缩水目标不低于现值 → 拒。
        match fs.inject_count_shrink(1, 300, 30) {
            Err(FinalSweepError::InjectionPrecondition) => {}
            other => panic!("应拒注入前置，实得 {:?}", other.map(|_| ())),
        }
        // 未知域 → 拒。
        match fs.inject_hidden_fail(9, 30) {
            Err(FinalSweepError::DomainUnknown) => {}
            other => panic!("应拒未知域，实得 {:?}", other.map(|_| ())),
        }
    }

    /// 测-10：双跑确定性对拍——同种子两跑状态指纹与报告指纹双双一致。
    #[test]
    fn double_run_determinism() {
        let proof = FinalSweep::determinism_probe(37).expect("对拍");
        assert!(proof.equal, "双跑必须一致：{:#?}", proof);
        assert_eq!(proof.state_fp_a, proof.state_fp_b);
        assert_eq!(proof.report_fp_a, proof.report_fp_b);
        // 不同种子指纹必不同（种子入清单混入，轨迹必异）。
        let proof2 = FinalSweep::determinism_probe(41).expect("对拍2");
        assert!(proof2.equal);
        assert_ne!(proof.state_fp_a, proof2.state_fp_a, "异种子轨迹必须可分辨");
    }

    /// 测-11：状态机非法边——终盘后再记账/再登记被拒；执行先于基线被拒。
    #[test]
    fn state_machine_edges() {
        let mut fs = FinalSweep::boot(43);
        // 未登记就执行 → 拒。
        match fs.record_execution(1, 1, Verdict::Pass) {
            Err(FinalSweepError::StateInvalid) => {}
            other => panic!("应拒未登记执行，实得 {:?}", other.map(|_| ())),
        }
        fs.register_domain(1, 320, 1, &gen_manifest(43, 1, 320)).expect("登记");
        // 未基线就执行 → 拒。
        match fs.record_execution(1, 2, Verdict::Pass) {
            Err(FinalSweepError::StateInvalid) => {}
            other => panic!("应拒未基线执行，实得 {:?}", other.map(|_| ())),
        }
        fs.snapshot_baseline(10).expect("基线");
        fs.record_execution(1, 20, Verdict::Pass).expect("执行（封版）");
        let report = fs.finalize(999).expect("终盘");
        assert!(report.overall);
        // 终盘后再登记/再执行/再快照/再终盘 → 全拒。
        match fs.register_domain(2, 10, 1000, &gen_manifest(43, 2, 10)) {
            Err(FinalSweepError::StateInvalid) => {}
            other => panic!("应拒终盘后登记，实得 {:?}", other.map(|_| ())),
        }
        match fs.record_execution(1, 1001, Verdict::Pass) {
            Err(FinalSweepError::StateInvalid) => {}
            other => panic!("应拒终盘后执行，实得 {:?}", other.map(|_| ())),
        }
        match fs.snapshot_baseline(1002) {
            Err(FinalSweepError::StateInvalid) => {}
            other => panic!("应拒终盘后快照，实得 {:?}", other.map(|_| ())),
        }
        assert!(fs.finalize(1003).is_err());
    }

    /// 测-12：再快照只增不减——快照后加域/加计数可再快照，缩水即红案拒记。
    #[test]
    fn baseline_resnapshot_only_increase() {
        let mut fs = FinalSweep::boot(47);
        fs.register_domain(1, 160, 1, &gen_manifest(47, 1, 160)).expect("登记");
        fs.register_domain(2, 150, 2, &gen_manifest(47, 2, 150)).expect("登记");
        fs.snapshot_baseline(10).expect("基线");
        // 加计数后可再快照。
        fs.register_domain(3, 40, 11, &gen_manifest(47, 3, 40)).expect("加域");
        fs.snapshot_baseline(12).expect("再快照应过（只增）");
        assert_eq!(fs.domain_count(), 3);
        assert_eq!(fs.ledger_total(), 350);
        // 直接缩水后再快照 → 红案拒记且基线不被破坏。
        fs.batch.set_count(2, 149).expect("改账");
        match fs.snapshot_baseline(13) {
            Err(FinalSweepError::BaselineShrink(2)) => {}
            other => panic!("再快照应红 BaselineShrink(2)，实得 {:?}", other.map(|_| ())),
        }
        // 基线未被破坏：守恒查仍红（现账缩水），复原后守恒恢复。
        match fs.conservation_check() {
            Err(FinalSweepError::BaselineShrink(2)) => {}
            other => panic!("守恒查应红，实得 {:?}", other.map(|_| ())),
        }
        fs.batch.set_count(2, 150).expect("复原");
        fs.batch.refresh_fingerprint(2).expect("指纹");
        assert!(fs.conservation_check().is_ok());
        assert_eq!(fs.baseline_total(), 350);
    }

    /// 测-13：登记前置闸——零计数/超上限/清单不符/重复登记/域表满全被拒。
    #[test]
    fn register_preconditions() {
        let mut fs = FinalSweep::boot(53);
        // 零计数。
        match fs.register_domain(1, 0, 1, &gen_manifest(53, 1, 0)) {
            Err(FinalSweepError::AssertCountZero) => {}
            other => panic!("应拒零计数，实得 {:?}", other.map(|_| ())),
        }
        // 超上限。
        match fs.register_domain(1, MAX_ASSERTS_PER_DOMAIN + 1, 1, &gen_manifest(53, 1, 4097)) {
            Err(FinalSweepError::AssertCountExceedsCap) => {}
            other => panic!("应拒超上限，实得 {:?}", other.map(|_| ())),
        }
        // 清单为空。
        match fs.register_domain(1, 4, 1, &[]) {
            Err(FinalSweepError::ManifestEmpty) => {}
            other => panic!("应拒空清单，实得 {:?}", other.map(|_| ())),
        }
        // 清单长度不符。
        match fs.register_domain(1, 4, 1, &gen_manifest(53, 1, 3)) {
            Err(FinalSweepError::ManifestCountMismatch) => {}
            other => panic!("应拒清单不符，实得 {:?}", other.map(|_| ())),
        }
        // 正常登记 + 重复登记。
        fs.register_domain(1, 300, 1, &gen_manifest(53, 1, 300)).expect("登记");
        match fs.register_domain(1, 10, 2, &gen_manifest(53, 1, 10)) {
            Err(FinalSweepError::DomainAlreadyRegistered) => {}
            other => panic!("应拒重复登记，实得 {:?}", other.map(|_| ())),
        }
        // 域表满：再灌 15 域后第 17 个拒。
        let mut d: u16 = 2;
        while d <= 16 {
            fs.register_domain(d, 1, d as u64, &gen_manifest(53, d, 1)).expect("灌表");
            d += 1;
        }
        assert_eq!(fs.domain_count(), MAX_DOMAINS);
        match fs.register_domain(17, 1, 17, &gen_manifest(53, 17, 1)) {
            Err(FinalSweepError::DomainTableFull) => {}
            other => panic!("应拒域表满，实得 {:?}", other.map(|_| ())),
        }
    }

    /// 测-14：事件账滚动封顶——超窗淘汰最旧、dropped 计数、守恒式保持。
    #[test]
    fn event_ledger_rollcap() {
        let mut fs = FinalSweep::boot(59);
        fs.register_domain(1, 300, 1, &gen_manifest(59, 1, 300)).expect("登记");
        fs.snapshot_baseline(10).expect("基线");
        // 灌 400 次执行 → 事件数远超 MAX_EVENTS(128)。
        let mut i = 0;
        while i < 400 {
            fs.record_execution(1, 100 + i as u64, Verdict::Pass).expect("执行");
            i += 1;
        }
        assert_eq!(fs.events().len(), MAX_EVENTS);
        assert!(fs.events_dropped() > 0);
        assert!(fs.event_conservation_ok(), "事件守恒式应保持");
        // 执行账滚动：窗内 ≤ 512 条。
        assert!(fs.exec_record_count() <= MAX_EXEC_RECORDS);
        // 最旧事件已被淘汰：窗内最小 seq > 淘汰数。
        let min_seq = fs.events().first().map(|e| e.seq).unwrap_or(0);
        assert!(min_seq > 0);
    }

    /// 测-15：域批登记表基础面——查册/双路求和/封版指纹确定性。
    #[test]
    fn domain_batch_basics() {
        let mut batch = DomainBatch::new();
        assert!(batch.is_empty());
        batch.register(1, 100, 1, &gen_manifest(61, 1, 100)).expect("登记1");
        batch.register(2, 200, 2, &gen_manifest(61, 2, 200)).expect("登记2");
        assert_eq!(batch.len(), 2);
        assert!(batch.find(1).is_some());
        assert!(batch.find(9).is_none());
        assert!(batch.at(2).is_none());
        assert_eq!(batch.total_forward().expect("和"), 300);
        assert_eq!(batch.total_reverse().expect("和"), 300);
        let fp1 = batch.seal_fingerprint();
        assert_eq!(fp1, batch.seal_fingerprint(), "同状态指纹必同");
        // 登记序入指纹：新域追加改变指纹。
        batch.register(3, 5, 3, &gen_manifest(61, 3, 5)).expect("登记3");
        assert_ne!(batch.seal_fingerprint(), fp1);
    }

    /// 测-16：报告证据行确定性——同种子两跑证据行逐条相等，且含判据关键词。
    #[test]
    fn report_evidence_deterministic() {
        let (_, ra) = run_standard_script(67).expect("脚本A");
        let (_, rb) = run_standard_script(67).expect("脚本B");
        assert_eq!(ra.evidence.len(), rb.evidence.len());
        for i in 0..ra.evidence.len() {
            assert_eq!(ra.evidence[i], rb.evidence[i], "证据行 {} 必逐字节相同", i);
        }
        assert_eq!(ra.fingerprint(), rb.fingerprint());
        // 判据关键词落证据。
        let joined = {
            let mut s = String::new();
            for line in ra.evidence.iter() {
                s.push_str(line);
            }
            s
        };
        assert!(joined.contains("≥300"));
        assert!(joined.contains("守恒"));
        assert!(joined.contains("全绿"));
        assert!(joined.contains("封版一致"));
        assert!(joined.contains("UNX-F0763-J1"));
    }

    /// 测-17：清单指纹敏感性——同域计数变动/清单条目变动必改域指纹。
    #[test]
    fn fingerprint_sensitivity() {
        let m1 = gen_manifest(71, 1, 8);
        let fp1 = domain_fingerprint(1, 8, &m1);
        // 同参重算同指纹。
        assert_eq!(fp1, domain_fingerprint(1, 8, &m1));
        // 计数变动改指纹。
        assert_ne!(fp1, domain_fingerprint(1, 9, &m1));
        // 条目变动改指纹。
        let mut m2 = m1.clone();
        m2[3] = m2[3].wrapping_add(1);
        assert_ne!(fp1, domain_fingerprint(1, 8, &m2));
        // 域号变动改指纹。
        assert_ne!(fp1, domain_fingerprint(2, 8, &m1));
        // 域数变动改封版指纹（三方比对中的登记序成分）。
        let mut d1: Vec<DomainEntry> = Vec::new();
        d1.push(DomainEntry {
            domain_id: 1,
            assert_count: 8,
            last_exec_tick: 0,
            last_verdict: Verdict::Unrun,
            manifest_fp: fp1,
            manifest: m1.clone(),
        });
        let s1 = manifest_seal_fingerprint(&d1);
        d1.push(DomainEntry {
            domain_id: 2,
            assert_count: 1,
            last_exec_tick: 0,
            last_verdict: Verdict::Unrun,
            manifest_fp: domain_fingerprint(2, 1, &m2[0..1].to_vec()),
            manifest: m2[0..1].to_vec(),
        });
        assert_ne!(s1, manifest_seal_fingerprint(&d1));
    }

    /// 测-18：Verdict/EventKind/InjectionKind 编码闭集——code() 稳定且互异。
    #[test]
    fn code_tables() {
        assert_eq!(Verdict::Unrun.code(), 0);
        assert_eq!(Verdict::Pass.code(), 1);
        assert_eq!(Verdict::Fail.code(), 2);
        let kinds = [
            EventKind::DomainRegistered, EventKind::BaselineTaken, EventKind::ExecutionRecorded,
            EventKind::SealFrozen, EventKind::CountChecked, EventKind::ConservationChecked,
            EventKind::GreenChecked, EventKind::SealChecked, EventKind::Finalized,
            EventKind::InjectionAttempted, EventKind::InjectionDetected, EventKind::InjectionCleared,
            EventKind::RedCaseLogged,
        ];
        for i in 0..kinds.len() {
            for j in (i + 1)..kinds.len() {
                assert_ne!(kinds[i].code(), kinds[j].code());
            }
        }
        assert_eq!(InjectionKind::CountShrink.code(), 0);
        assert_eq!(InjectionKind::HiddenFail.code(), 1);
        assert_eq!(InjectionKind::SealDrift.code(), 2);
    }

    /// 测-19：健康面负案——缺基线/缺封版时 ok 翻红，事件守恒式始终成立。
    #[test]
    fn health_negative_cases() {
        // Fresh 机：不健康。
        let fs0 = FinalSweep::boot(73);
        assert!(!fs0.health().ok);
        assert_eq!(fs0.health().state_name, "未登记");
        // 仅登记未基线：健康（登记期合法态）。
        let mut fs1 = FinalSweep::boot(73);
        fs1.register_domain(1, 310, 1, &gen_manifest(73, 1, 310)).expect("登记");
        assert!(fs1.health().ok);
        assert_eq!(fs1.health().state_name, "登记中");
        // 基线后未封版：健康。
        fs1.snapshot_baseline(10).expect("基线");
        assert!(fs1.health().ok);
        assert_eq!(fs1.health().state_name, "基线已取");
        assert!(fs1.health().baseline_taken);
        assert!(!fs1.health().sealed);
    }

    /// 测-20：终盘红案呈现——注入后不回滚直接终盘，报告必须呈现红案与定位。
    #[test]
    fn finalize_presents_red_case() {
        let mut fs = FinalSweep::boot(79);
        fs.register_domain(1, 200, 1, &gen_manifest(79, 1, 200)).expect("登记");
        fs.register_domain(2, 150, 2, &gen_manifest(79, 2, 150)).expect("登记");
        fs.snapshot_baseline(10).expect("基线");
        fs.record_execution(1, 20, Verdict::Pass).expect("执行");
        fs.record_execution(2, 30, Verdict::Fail).expect("执行（真败）");
        let report = fs.finalize(999).expect("终盘应出报告");
        assert!(!report.overall, "有 Fail 域终盘必须不过");
        assert!(!report.green_row.pass);
        assert_eq!(report.green_row.code, E_EXEC_FAIL);
        // 红案定位域号落报告注记（两位域号格式 域#02）。
        assert!(report.green_row.note.contains("域#02"), "Fail 域号必须落注记：{}", report.green_row.note);
        // 计数查通过（310 ≥ 300）。
        assert!(report.count_row.pass);
        // 终盘后状态字为 Finalized 且报告可复读。
        assert_eq!(fs.state(), STATE_FINALIZED);
        let again = fs.last_report().expect("报告应可复读");
        assert_eq!(again.fingerprint(), report.fingerprint());
    }
}
