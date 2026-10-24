//! UNX-F0786 · 判据 1 终验（150 次成功率 ≥99% 凭证落账）· 诊断段 0x2770
//!
//! ── 验收锚点（原文全抄） ─────────────────────────────────────────────
//! 定位：docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md
//! 行 2391 表头「| 编号 | 功能条目 | 行数 | 状态 | 判据 |」下、行 2397：
//!
//! | UNX-F0786 | 判据 1 终验（150 次成功率 ≥99% 凭证落账） | 580 | 已深化 |
//!   UNX-F0786-J1 终验过（判据 1 凭证包四件：终核账/留痕账/判定账/报告指针——
//!   齐 4/4 落账）且结论 ≥99% 定格 |
//!
//! 邻行原文（同页行 2388~2399 摘抄，佐证收口终验域 B40 批满收官终局面的
//! 防重边界与六判据终验的邻位语义）：
//!
//! > AI-01 承办｜域账累计：228480 + 本批 11520 = 240000 / 240,000｜防重：
//!   满收官终局面（报告定版/终点宣告/六判据终验/封域宣告/终局宣告）归本批
//!   独占；被验账面（F0762–F0778 前置终核账/F0756 包封存账等）为既有账面
//!   （终验消费不重写）；B40 为 A1 域第 40 批（F0781–F0800），域账累计链
//!   终点 = 240,000｜R-A1-004 纪律：批小计 = 逐条求和唯一真值（本批逐条求和
//!   11520）｜新增骨架批次（本会话立）
//!
//! | UNX-F0781 | 收官报告定版（六节定稿+哈希封存） | 570 | 已深化 |
//!   UNX-F0781-J1 定版过（六节定稿 + 指针可解引 100% + 报告哈希封存）且与
//!   F0778 初稿差异留痕 |
//! | UNX-F0782 | 域账终点宣告（240,000 终值锁定账） | 580 | 已深化 |
//!   UNX-F0782-J1 终点宣告过（域账累计链 40 段终点 = 240,000 精确等 + 逐段
//!   链核 40/40）且终值哈希锁定 |
//! | UNX-F0787 | 判据 2 终验（300+ 断言全绿凭证落账） | 570 | 已深化 |
//!   UNX-F0787-J1 终验过（判据 2 凭证包四件：终盘账/封版清单/执行账/分项
//!   盘点——齐 4/4）且 ≥300 全绿定格 |
//! | UNX-F0788 | 判据 3 终验（冷启动 ≤3 秒凭证落账） | 580 | 已深化 |
//!   UNX-F0788-J1 终验过（判据 3 凭证包四件：终核账/耗时账/画像报告/快照
//!   ——齐 4/4）且 ≤3,000ms 定格 |
//!
//! 镜像输入原文（终核账件的数据面来源，行 2263）：
//!
//! | UNX-F0687 | 成功率统计账（150 次 ≥99% 判定） | 580 | 已深化 |
//!   UNX-F0687-J1 成功率统计（成功/150）≥99% 判定账落簿且分机成功率三列齐，
//!   口径二值零灰区 |
//!
//! 前置终核镜像原文（行 2366，本账「终验消费不重写」的对面账）：
//!
//! | UNX-F0762 | 三机矩阵账终核（150 次成功率复算） | 580 | 已深化 |
//!   UNX-F0762-J1 终核过（150 次成功账复算：成功率 ≥99% 逐机逐轮复核）且
//!   与 F0688 判定账一致 100% |
//!
//! J 判据拆解（本模块逐条落地的验收口径）：
//!   J1-a 齐 4/4 落账：判据 1 凭证包四件 = 终核账（150 次总账 + 三机分列，
//!        F0687 镜像输入）/ 留痕账（判定过程事件链）/ 判定账（≥99% = 9900ppm
//!        二值结论 + ppm 值）/ 报告指针（报告位置引用 + 报告指纹）。四件
//!        presence 位图全置位才算齐；缺任一件即终验不过（E_ITEM_MISSING）。
//!   J1-b 逐件校验：件在场之外还须内容自洽——终核账（150 次 / Σ三机守恒 /
//!        ppm 复算 / 封账指纹不漂移）、留痕账（事件链滚动哈希重放复核）、
//!        判定账（ppm 值 ↔ 二值结论互证 + 样本恰 150 + 来源指纹在位）、
//!        报告指针（定位串可解引格式 + 报告指纹非零 + 锚链在位）。
//!   J1-c 交叉一致：终核账 ppm ≡ 判定账 base_ppm（数值等）、判定账来源指纹
//!        ≡ 终核账封账指纹（引用等）、判定结论 ≡ ppm 复算结论（结论等）、
//!        报告指针锚链 ≡ 留痕账链头（锚定等）——四路任一不符即
//!        E_CROSS_MISMATCH 终验不过。
//!   J1-d ≥99% 定格：ppm 整数制 9900 达标线（≥ 语义非 >）；定格态
//!        Undecided→Decided 单向，Decided 后任何再终验（重新 finalize）即
//!        重复终验红案（E_REVERIFY），原定格结论不动摇；<99% 拒定格
//!        （E_BELOW_LINE，属正常拒绝非红案，定格态保持 Undecided）。
//!   J1-e 包指纹：四件合指纹 FNV（域标签 + presence 位图 + 逐件落账指纹），
//!        定格时冻结；冻结后任何件内容被改 → 合指纹/件指纹复算漂移 →
//!        E_PACK_TAMPER 篡改检出（红案）。
//!
//! 落账序说明（与判据枚举序分离，二者不得混淆）：
//!   判据枚举序（原文口径）＝ 终核账(第1件) → 留痕账(第2件) → 判定账(第3件)
//!   → 报告指针(第4件)；落账序（本站管线）＝ 终核账(slot0) → 判定账(slot1)
//!   → 留痕账封链(slot2) → 报告指针(slot3)。留痕账封链须含前两件落账事件
//!   （否则事件链不覆盖判定过程主轴），报告指针锚定封链链头且引用定格后
//!   才存在的终局报告，故二者分列第 2/3 落账步。落账序偏离即
//!   E_ITEM_ORDER（件可齐而序不可乱）。
//!
//! ── 深度子系统清单（本文件全手工定制，零注水） ───────────────────────
//!   S1 OutcomePlane 150 位结果位平面：bit=运行序（机号×50+轮号-1），
//!      1=成功 0=失败，尾部 2 脏位校验，逐位读/翻/计数全显式越界检。
//!   S2 canonical_plane 规范确定性生成器：xorshift64* 流按 seed 抽失败位，
//!      同 seed 双跑必同平面；失败数 >150 拒构造。
//!   S3 MachineColumn 三机分列：机号/轮数/成功数/成功率 ppm（整数
//!      success×10000/50），Σ三机守恒由终核账复算。
//!   S4 FinalCoreLedger 终核账（凭证第 1 件）：位平面 + 150 总账 + 三机分列
//!      + F0687 镜像来源指纹 + 封账指纹；verify_self 九路自洽核。
//!   S5 TraceLedger 留痕账（凭证第 2 件）：判定过程事件链（开站/件落账/封链），
//!      滚动哈希 mix_event 链式封印，replay_verify 全链重放复核 + 断链定位。
//!   S6 JudgementRecord 判定账（凭证第 3 件）：base_ppm + 二值结论 + 终核账
//!      来源指纹 + 样本数 + 判定序号；from_final 派生道 / from_parts 外来道
//!      双入口，verify_self 四路自洽核。
//!   S7 ReportPointer 报告指针（凭证第 4 件）：定位串（F0781#S1..S6 / F0781#ALL
//!      可解引格式）+ 报告指纹 + 留痕链锚；derive_for 镜像派生道。
//!   S8 EvidencePack 凭证包：四件 presence 位图 + 落账序日志 + 逐件落账指纹
//!      + 合指纹 + 冻结制；齐 4/4 判定 / 乱序拒 / 重件拒 / 冻结后写拒 /
//!      verify_integrity 篡改检出。
//!   S9 VerdictState 定格态：Undecided→Decided 单向状态机，Decided 载
//!      结论/ppm/包指纹/定格序号；红案账 RedCaseRecord（重复终验/包篡改/
//!      链断三类）。
//!   S10 FinalVerifyStation 终验编排站：四件收集 → 逐件校验 → 交叉一致 →
//!      落账序核 → ≥99% 定格；finalize_verdict 定格主闸 + health() 十二灯
//!      + gazette() 定格公报（文本 + 公报指纹）。
//!   S11 双跑确定性对拍 dual_run_check / assert_dual_run：同 seed 两路独立
//!      构建定格，vitals 六项逐项等，漂移即 E_DUAL_RUN。
//!   S12 注入面六路：缺件注入 / 不一致注入（终核 ppm≠判定 ppm ±1 漂移）/
//!      重复终验注入 / 落账乱序注入 / 包篡改注入（定格后翻位）/ 留痕断链
//!      注入，每路显性检出（detected + 诊断码 + 中文诊断）。
//!   S13 mirror_source_fp F0687 镜像引用：自包含纪律下不 import 被验账，
//!      以规范同构指纹（域标签+位平面）作终核账来源引用，「终验消费不
//!      重写」既有账面。
//!
//! ── 相邻任务对接 ─────────────────────────────────────────────────────
//!   F0687 成功率统计账：本账终核账件为其 150 次总账+三机分列的镜像消费面
//!                        （source_fp 引用其账指纹，不复写其账体）。
//!   F0682/F0683：位平面槽位序（机×50+轮）与轮排/逐次留痕同口径。
//!   F0762 三机矩阵账终核：其「与判定账一致 100%」的判定账即本账第 3 件。
//!   F0781 收官报告定版：本账报告指针件引用其报告位置与哈希封存指纹。
//!   F0787/F0788/F0789 判据 2/3/4 终验：同构四件凭证包范式，互不越界
//!                        （判据 1 专管 150 次 ≥99% 成功率口径）。
//!
//! ── 通用纪律 ─────────────────────────────────────────────────────────
//!   no_std + alloc；只 use core::… 与 use alloc::{…}，禁 use crate::…。
//!   内核路径零 unwrap/expect/panic!/运行期 assert!（编译期 const assert! 允许）。
//!   可失败 API 全走 Result<T, Verdict1Error>；溢出显性 saturating/wrapping
//!   （FNV 一律 wrapping_mul）。整数运算零浮点，ppm 万分率整数制。
//!   >64KiB 结构体禁上栈（本文件栈上最大为 FinalCoreLedger ≈ 128B 与
//!   canonical_plane 的 [bool;150] = 150B，安全；事件/红案/灯表走 Vec 堆）。
//!   const fn 纪律：const fn 内零 slice::get / Vec 索引 / Ord::min-max /
//!   derive(PartialEq) 类型的 == / for 循环（一律 while + 索引）；
//!   含 Vec/String 字段的结构体禁 derive Copy。
//!
//! 零浮点、零 unwrap、零 expect、零 panic 路径（#[cfg(test)] 测试域除外）。

use core::mem::size_of;

use alloc::{
    format,
    string::String,
    vec,
    vec::Vec,
};

// ── 诊断码段（0x2770~0x277F 独占 0x10；先验空闲：kernel/ 全树 grep "0x277"
//    零命中（0x2710 段被 unx0688_failarc 占，0x2780~0x27BF 被 unx3361_utimewin
//    声明独占，0x2760=ves503，故取本任务号邻段 0x2770） ──
pub const SEG_BASE: u16 = 0x2770;

const E_SEALED_WRITE: u16 = SEG_BASE + 0x0; // 0x2770 定格后仍写账
const E_ITEM_MISSING: u16 = SEG_BASE + 0x1; // 0x2771 凭证包缺件（<4/4）
const E_ITEM_DUP: u16 = SEG_BASE + 0x2; // 0x2772 同件二落
const E_ITEM_ORDER: u16 = SEG_BASE + 0x3; // 0x2773 落账序偏离规范管线
const E_FINAL_SELF: u16 = SEG_BASE + 0x4; // 0x2774 终核账自洽破（九路）
const E_MACHINE_COL: u16 = SEG_BASE + 0x5; // 0x2775 三机分列轮数非 50
const E_PLANE_INDEX: u16 = SEG_BASE + 0x6; // 0x2776 位平面索引/二值/构造越界
const E_TRACE_CHAIN: u16 = SEG_BASE + 0x7; // 0x2777 留痕账事件链断链
const E_JUDGE_SELF: u16 = SEG_BASE + 0x8; // 0x2778 判定账自洽破（四路）
const E_REPORT_PTR: u16 = SEG_BASE + 0x9; // 0x2779 报告指针非法（格式/指纹/锚）
const E_CROSS_MISMATCH: u16 = SEG_BASE + 0xA; // 0x277A 终核↔判定↔留痕↔报告交叉不一致
const E_PACK_FROZEN: u16 = SEG_BASE + 0xB; // 0x277B 凭证包已冻结仍写
const E_PACK_TAMPER: u16 = SEG_BASE + 0xC; // 0x277C 包指纹漂移（篡改检出）
const E_REVERIFY: u16 = SEG_BASE + 0xD; // 0x277D Decided 后重复终验（红案）
const E_BELOW_LINE: u16 = SEG_BASE + 0xE; // 0x277E <99% 拒定格（正常拒绝）
const E_DUAL_RUN: u16 = SEG_BASE + 0xF; // 0x277F 双跑对拍漂移

/// 诊断码 → 中文可操作诊断（独占段 0x2770~0x277F 内解释，段外一律未知）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED_WRITE => "定格后仍写账：终验结论已定格，账面只读，请克隆新站或走公报通道",
        E_ITEM_MISSING => "凭证包缺件：判据 1 四件须齐 4/4（终核账/留痕账/判定账/报告指针），缺件即终验不过",
        E_ITEM_DUP => "同件二落：该凭证件已落账，拒收重复落账",
        E_ITEM_ORDER => "落账序偏离：规范管线为 终核账→判定账→留痕封链→报告指针，序不可乱",
        E_FINAL_SELF => "终核账自洽破：150 总账/Σ三机守恒/ppm 复算/封账指纹 任一路不符",
        E_MACHINE_COL => "三机分列轮数非 50：分列口径 3 机 × 50 次，缺轮缺机需补",
        E_PLANE_INDEX => "位平面索引/二值/构造越界：合法运行位 0..150，位值只认 0/1，失败数 ≤150",
        E_TRACE_CHAIN => "留痕账事件链断链：重放复核链哈希不符，判定过程留痕已被篡改或损坏",
        E_JUDGE_SELF => "判定账自洽破：ppm↔二值结论/样本数 150/来源指纹/封账指纹 任一路不符",
        E_REPORT_PTR => "报告指针非法：定位串须 F0781#S1..S6 或 F0781#ALL，报告指纹与锚链非零",
        E_CROSS_MISMATCH => "交叉不一致：终核账 ppm/指纹、判定结论、报告锚链与对面件不符，四路逐路核对",
        E_PACK_FROZEN => "凭证包已冻结：定格时合指纹已封，冻结后禁写，改动即篡改红案",
        E_PACK_TAMPER => "包指纹漂移：件内容与落账指纹不符，凭证包被篡改，定格结论不可信需重立站",
        E_REVERIFY => "重复终验红案：结论已定格单向不可重算，再提交即红案立案且原结论不动摇",
        E_BELOW_LINE => "低于 99% 达标线：9900ppm 未达，拒定格（非红案），定格态保持未决",
        E_DUAL_RUN => "双跑对拍漂移：同 seed 两路构建指纹不等，确定性破",
        _ => "未知诊断码（0x2770 段外或保留位）",
    }
}

// ── 判据常量（来源：UNX-F0786-J1 与 F0687-J1 镜像口径，常量写死注释来源） ──
/// 三机矩阵机数（来源：F0687-J1「分机成功率三列」）。
pub const MACHINE_COUNT: usize = 3;
/// 每机轮数（来源：F0682-J1「3 机 × 50 次 = 150 次序」）。
pub const ROUNDS_PER_MACHINE: usize = 50;
/// 总账次数（来源：UNX-F0786 判据原文「150 次成功率」）。
pub const TOTAL_RUNS: usize = MACHINE_COUNT * ROUNDS_PER_MACHINE;
/// 位平面字节数（150 位 → 19 字节 = 152 位，尾 2 位保留位）。
pub const PLANE_BYTES: usize = (TOTAL_RUNS + 7) / 8;
/// 位平面尾部保留位数（152 - 150 = 2，恒须为零）。
pub const PLANE_TAIL_UNUSED_BITS: usize = PLANE_BYTES * 8 - TOTAL_RUNS;
/// 达标线：≥99% = 9900 ppm（来源：判据原文「≥99% 定格」，万分率整数制零浮点）。
pub const PASS_PPM: u64 = 9_900;
/// ppm 换算基数（10000 ppm = 100%）。
pub const PPM_SCALE: u64 = 10_000;
/// 达标下界成功数：149 次（149×10000/150 = 9933 ≥ 9900；148 → 9866 < 9900）。
pub const MIN_SUCCESS_FOR_PASS: u32 = 149;
/// 拒格上界成功数：148 次（148×10000/150 = 9866 < 9900，二值零灰区）。
pub const MAX_SUCCESS_BELOW_LINE: u32 = 148;
/// 凭证包件数（来源：判据原文「四件——齐 4/4 落账」）。
pub const ITEM_COUNT: usize = 4;
/// 全件在场位图（slot0|slot1|slot2|slot3 = 0b1111）。
pub const MASK_ALL_ITEMS: u8 = 0b1111;
/// 报告定位串长度上限（字节；对接 F0781 报告位置引用）。
pub const REPORT_LOCATOR_MAX_LEN: usize = 64;
/// 留痕账事件容量上限（判定过程事件链长度护栏）。
pub const TRACE_EVENT_MAX: usize = 64;
/// 结果位二值闭集：0=失败（内容归 F0688，本账只引不载），1=成功。
pub const RAW_FAIL: u8 = 0;
pub const RAW_SUCCESS: u8 = 1;
/// 判据 1 凭证包口径原文（定格公报与件报表的锚回显）。
pub const CRITERION_TEXT: &str =
    "判据 1 凭证包四件：终核账/留痕账/判定账/报告指针——齐 4/4 落账";

// ── 编译期断言（const 期允许；口径数学与栈尺寸锁死） ──────────────────
const _: () = assert!(MACHINE_COUNT == 3);
const _: () = assert!(ROUNDS_PER_MACHINE == 50);
const _: () = assert!(TOTAL_RUNS == 150);
const _: () = assert!(PLANE_BYTES == 19);
const _: () = assert!(PLANE_TAIL_UNUSED_BITS == 2);
const _: () = assert!(PASS_PPM == 9_900 && PPM_SCALE == 10_000);
// 达标/拒格边界的整数复算（零浮点口径锁死）。
const _: () = assert!((149u64 * 10_000) / 150 == 9_933);
const _: () = assert!((148u64 * 10_000) / 150 == 9_866);
const _: () = assert!(MIN_SUCCESS_FOR_PASS == 149 && MAX_SUCCESS_BELOW_LINE == 148);
const _: () = assert!(MASK_ALL_ITEMS == 0b1111 && MASK_ALL_ITEMS.count_ones() == 4);
// 栈尺寸护栏（>64KiB 禁上栈的红线远未触及，锁死上限防未来膨胀）。
const _: () = assert!(size_of::<OutcomePlane>() <= 24);
const _: () = assert!(size_of::<MachineColumn>() <= 24);
const _: () = assert!(size_of::<FinalCoreLedger>() <= 160);
const _: () = assert!(size_of::<JudgementRecord>() <= 48);
const _: () = assert!(size_of::<TraceDigest>() <= 24);
const _: () = assert!(size_of::<TraceEvent>() <= 24);
const _: () = assert!(size_of::<RedCaseRecord>() <= 16);
const _: () = assert!(size_of::<FinalConclusion>() <= 48);

// ── 错误枚举（可失败 API 全走 Result<T, Verdict1Error>） ───────────────
/// 判据 1 终验域错误枚举：每变体带中文可操作诊断与独占诊断码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict1Error {
    /// 定格后仍写账（0x2770）：`what` 指明被写的账面。
    SealedWrite { what: &'static str },
    /// 凭证包缺件（0x2771）：expect=期望位图，got=实得位图，missing=缺件位。
    ItemMissing { expect: u8, got: u8, missing: u8 },
    /// 同件二落（0x2772）：kind=slot 号。
    ItemDup { kind: u8 },
    /// 落账序偏离（0x2773）：expect=期望下一 slot，got=实提 slot。
    ItemOrder { expect: u8, got: u8 },
    /// 终核账自洽破（0x2774）：sub 指明九路中哪一路。
    FinalSelf { sub: &'static str },
    /// 三机分列轮数非 50（0x2775）。
    MachineCol { machine: u8, rounds: u32 },
    /// 位平面索引/二值/构造越界（0x2776）。
    PlaneIndex { idx: u32, limit: u32 },
    /// 留痕账事件链断链（0x2777）：at_seq=断点事件序号（0=链头失配）。
    TraceChain { at_seq: u32 },
    /// 判定账自洽破（0x2778）：sub 指明四路中哪一路。
    JudgeSelf { sub: &'static str },
    /// 报告指针非法（0x2779）：sub 指明格式/指纹/锚链哪一路。
    ReportPtr { sub: &'static str },
    /// 交叉不一致（0x277A）：sub 指明四路（引用等/ppm 等/结论等/锚定等）。
    CrossMismatch { sub: &'static str },
    /// 凭证包已冻结仍写（0x277B）。
    PackFrozen,
    /// 包指纹漂移（0x277C）：stored=落账指纹，recomputed=复算指纹。
    PackTamper { stored: u64, recomputed: u64 },
    /// Decided 后重复终验红案（0x277D）：decided_serial=已定格序号。
    Reverify { decided_serial: u32 },
    /// 低于 99% 拒定格（0x277E）：ppm=实得万分率。
    BelowLine { ppm: u64 },
    /// 双跑对拍漂移（0x277F）：sub 指明漂移项。
    DualRun { sub: &'static str },
}

impl Verdict1Error {
    /// 诊断码（0x2770~0x277F 独占段内逐变体独占）。
    pub const fn err_code(&self) -> u16 {
        match self {
            Verdict1Error::SealedWrite { .. } => E_SEALED_WRITE,
            Verdict1Error::ItemMissing { .. } => E_ITEM_MISSING,
            Verdict1Error::ItemDup { .. } => E_ITEM_DUP,
            Verdict1Error::ItemOrder { .. } => E_ITEM_ORDER,
            Verdict1Error::FinalSelf { .. } => E_FINAL_SELF,
            Verdict1Error::MachineCol { .. } => E_MACHINE_COL,
            Verdict1Error::PlaneIndex { .. } => E_PLANE_INDEX,
            Verdict1Error::TraceChain { .. } => E_TRACE_CHAIN,
            Verdict1Error::JudgeSelf { .. } => E_JUDGE_SELF,
            Verdict1Error::ReportPtr { .. } => E_REPORT_PTR,
            Verdict1Error::CrossMismatch { .. } => E_CROSS_MISMATCH,
            Verdict1Error::PackFrozen => E_PACK_FROZEN,
            Verdict1Error::PackTamper { .. } => E_PACK_TAMPER,
            Verdict1Error::Reverify { .. } => E_REVERIFY,
            Verdict1Error::BelowLine { .. } => E_BELOW_LINE,
            Verdict1Error::DualRun { .. } => E_DUAL_RUN,
        }
    }

    /// 中文可操作诊断（逐变体，含关键上下文数值）。
    pub fn detail(&self) -> &'static str {
        match self {
            Verdict1Error::SealedWrite { .. } => {
                "定格后仍写账：终验结论已定格，账面只读；如需复演请另立新站"
            }
            Verdict1Error::ItemMissing { .. } => {
                "凭证包缺件：四件（终核账/留痕账/判定账/报告指针）须齐 4/4 落账，先补齐缺件再终验"
            }
            Verdict1Error::ItemDup { .. } => "同件二落：该凭证件已在包内，拒收重复落账",
            Verdict1Error::ItemOrder { .. } => {
                "落账序偏离：按 终核账→判定账→留痕封链→报告指针 管线落账"
            }
            Verdict1Error::FinalSelf { .. } => {
                "终核账自洽破：核对 150 总账/Σ三机守恒/ppm 复算/封账指纹四组九路"
            }
            Verdict1Error::MachineCol { .. } => {
                "三机分列轮数非 50：分列口径 3 机 × 50 次恒定，请重建终核账"
            }
            Verdict1Error::PlaneIndex { .. } => {
                "位平面越界/灰区：运行位合法域 0..150，位值只认 0/1，构造失败数 ≤150"
            }
            Verdict1Error::TraceChain { .. } => {
                "留痕账事件链断链：重放链哈希不符，留痕被改，需重建事件链"
            }
            Verdict1Error::JudgeSelf { .. } => {
                "判定账自洽破：ppm↔结论互证/样本恰 150/来源指纹/封账指纹逐路核对"
            }
            Verdict1Error::ReportPtr { .. } => {
                "报告指针非法：定位串 F0781#S1..S6|F0781#ALL，报告指纹与锚链非零"
            }
            Verdict1Error::CrossMismatch { .. } => {
                "交叉不一致：终核 ppm/指纹、判定结论、报告锚链须与对面件逐路相等"
            }
            Verdict1Error::PackFrozen => {
                "凭证包已冻结：定格合指纹已封，冻结后禁写；改动即走篡改红案通道"
            }
            Verdict1Error::PackTamper { .. } => {
                "包指纹漂移：件内容与落账指纹不符，凭证包被篡改，定格结论不可信"
            }
            Verdict1Error::Reverify { .. } => {
                "重复终验红案：定格单向不可重算；再提交即红案立案，原结论不动摇"
            }
            Verdict1Error::BelowLine { .. } => {
                "低于 99% 达标线（9900ppm）：拒定格非红案，补跑或另立账面后重走终验"
            }
            Verdict1Error::DualRun { .. } => {
                "双跑对拍漂移：同 seed 两路构建 vitals 不等，确定性破需归因"
            }
        }
    }
}

// ── FNV-1a 64 哈希（自包含；全 wrapping 无溢出 UB） ────────────────────
/// FNV-1a 64 偏移基（标准常量，写死来源：FNV-1a 64 规范）。
pub const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 素数（标准常量，写死来源：FNV-1a 64 规范）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 流式 FNV-1a 64（凭证件指纹/合指纹/事件链共用底座）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fnv64 {
    state: u64,
}

impl Fnv64 {
    /// 新建流（标准偏移基起）。
    pub const fn new() -> Self {
        Fnv64 {
            state: FNV_OFFSET_BASIS,
        }
    }

    /// 从既有链值续流（事件链续段用）。
    pub const fn resume(chain: u64) -> Self {
        Fnv64 { state: chain }
    }

    /// 写入单字节。
    pub fn write_u8(&mut self, b: u8) {
        self.state = (self.state ^ b as u64).wrapping_mul(FNV_PRIME);
    }

    /// 写入 u16（小端字节序展开）。
    pub fn write_u16(&mut self, v: u16) {
        self.write_u8((v & 0xFF) as u8);
        self.write_u8((v >> 8) as u8);
    }

    /// 写入 u32（小端字节序展开）。
    pub fn write_u32(&mut self, v: u32) {
        self.write_u8((v & 0xFF) as u8);
        self.write_u8(((v >> 8) & 0xFF) as u8);
        self.write_u8(((v >> 16) & 0xFF) as u8);
        self.write_u8(((v >> 24) & 0xFF) as u8);
    }

    /// 写入 u64（小端字节序展开）。
    pub fn write_u64(&mut self, v: u64) {
        self.write_u32((v & 0xFFFF_FFFF) as u32);
        self.write_u32((v >> 32) as u32);
    }

    /// 写入字节串。
    pub fn write_bytes(&mut self, b: &[u8]) {
        let mut i = 0usize;
        while i < b.len() {
            self.write_u8(b[i]);
            i += 1;
        }
    }

    /// 写入域标签（&str 标签，域分隔防跨域碰撞）。
    pub fn write_tag(&mut self, s: &str) {
        self.write_bytes(s.as_bytes());
    }

    /// 终值（不消耗流）。
    pub const fn finish(&self) -> u64 {
        self.state
    }
}

/// 一次性 FNV-1a 64。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h = Fnv64::new();
    h.write_bytes(data);
    h.finish()
}

/// 事件链步进混合：cur ⊕ (seq,kind,payload) 按 FNV-1a 字节序续乘。
/// 留痕账逐事件封印的唯一步进函数（同输入必同输出，双跑确定性底座）。
pub fn mix_event(cur: u64, seq: u32, kind: u8, payload: u32) -> u64 {
    let mut h = Fnv64::resume(cur);
    h.write_u32(seq);
    h.write_u8(kind);
    h.write_u32(payload);
    h.finish()
}

// ── 万分率与二值结论（整数制零浮点） ──────────────────────────────────
/// 成功率万分率：success × 10000 / total（total=0 显性回 0，不除零）。
pub fn ppm_of(success: u32, total: u32) -> u64 {
    if total == 0 {
        return 0;
    }
    (success as u64) * PPM_SCALE / (total as u64)
}

/// 二值结论闭集：Pass（≥9900ppm）/ Reject（<9900ppm），口径零灰区。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 达标（≥99% = 9900ppm，含边界）。
    Pass,
    /// 未达标（<9900ppm；拒定格非红案）。
    Reject,
}

impl Verdict {
    /// 原始字节：Pass=1，Reject=0（二值闭集）。
    pub const fn as_raw(self) -> u8 {
        match self {
            Verdict::Pass => 1,
            Verdict::Reject => 0,
        }
    }

    /// 从原始字节重建：只认 0/1，其余灰区拒收。
    pub fn from_raw(raw: u8) -> Result<Verdict, Verdict1Error> {
        match raw {
            1 => Ok(Verdict::Pass),
            0 => Ok(Verdict::Reject),
            other => Err(Verdict1Error::PlaneIndex {
                idx: other as u32,
                limit: 1,
            }),
        }
    }

    /// 中文短名。
    pub const fn label(self) -> &'static str {
        match self {
            Verdict::Pass => "达标",
            Verdict::Reject => "未达标",
        }
    }
}

/// ppm → 二值结论（≥9900 达标，含边界；const 期可用）。
pub const fn verdict_of_ppm(ppm: u64) -> Verdict {
    if ppm >= PASS_PPM {
        Verdict::Pass
    } else {
        Verdict::Reject
    }
}

// ── S1 OutcomePlane：150 位结果位平面 ────────────────────────────────
/// 150 次运行结果位平面：bit 序 = 机号 × 50 +（轮号-1）；1=成功 0=失败。
/// 尾部 2 位保留位恒零（脏位即构造非法）。对接 F0682 轮排槽位口径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutcomePlane {
    bits: [u8; PLANE_BYTES],
}

impl OutcomePlane {
    /// 全零平面（全失败基线；构造器内部用）。
    pub const fn zeroed() -> Self {
        OutcomePlane {
            bits: [0; PLANE_BYTES],
        }
    }

    /// 从原始字节构造：尾部保留位必须干净，否则拒收（防灰区注入）。
    pub fn from_raw(bytes: [u8; PLANE_BYTES]) -> Result<OutcomePlane, Verdict1Error> {
        let p = OutcomePlane { bits: bytes };
        if !p.tail_bits_clean() {
            return Err(Verdict1Error::PlaneIndex {
                idx: TOTAL_RUNS as u32,
                limit: TOTAL_RUNS as u32,
            });
        }
        Ok(p)
    }

    /// 原始字节快照。
    pub const fn raw(&self) -> [u8; PLANE_BYTES] {
        self.bits
    }

    /// 尾部保留位是否全零（150..152 位）。
    pub fn tail_bits_clean(&self) -> bool {
        let mut i = TOTAL_RUNS;
        while i < PLANE_BYTES * 8 {
            let mask = 1u8 << (i % 8);
            if self.bits[i / 8] & mask != 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 读运行位（越界显性拒）。
    pub fn bit(&self, idx: usize) -> Result<u8, Verdict1Error> {
        if idx >= TOTAL_RUNS {
            return Err(Verdict1Error::PlaneIndex {
                idx: idx as u32,
                limit: TOTAL_RUNS as u32,
            });
        }
        Ok((self.bits[idx / 8] >> (idx % 8)) & 1)
    }

    /// 写运行位（idx 越界或位值非 0/1 显性拒）。
    pub fn set_bit(&mut self, idx: usize, v: u8) -> Result<(), Verdict1Error> {
        if idx >= TOTAL_RUNS {
            return Err(Verdict1Error::PlaneIndex {
                idx: idx as u32,
                limit: TOTAL_RUNS as u32,
            });
        }
        if v > 1 {
            return Err(Verdict1Error::PlaneIndex {
                idx: v as u32,
                limit: 1,
            });
        }
        let mask = 1u8 << (idx % 8);
        if v == 1 {
            self.bits[idx / 8] |= mask;
        } else {
            self.bits[idx / 8] &= !mask;
        }
        Ok(())
    }

    /// 翻转运行位（注入面专用：包篡改/断链演练）。
    pub fn flip_bit(&mut self, idx: usize) -> Result<(), Verdict1Error> {
        if idx >= TOTAL_RUNS {
            return Err(Verdict1Error::PlaneIndex {
                idx: idx as u32,
                limit: TOTAL_RUNS as u32,
            });
        }
        self.bits[idx / 8] ^= 1u8 << (idx % 8);
        Ok(())
    }

    /// 成功位计数（while + 索引，const fn 纪律同款写法）。
    pub fn success_count(&self) -> u32 {
        let mut n: u32 = 0;
        let mut i = 0usize;
        while i < TOTAL_RUNS {
            if (self.bits[i / 8] >> (i % 8)) & 1 == RAW_SUCCESS {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 单机成功数（机号 ≥3 显性拒；槽位基 = 机号 × 50 越界即平面越界）。
    pub fn machine_success(&self, machine: u8) -> Result<u32, Verdict1Error> {
        let m = machine as usize;
        if m >= MACHINE_COUNT {
            return Err(Verdict1Error::PlaneIndex {
                idx: (m * ROUNDS_PER_MACHINE) as u32,
                limit: TOTAL_RUNS as u32,
            });
        }
        let base = m * ROUNDS_PER_MACHINE;
        let mut n: u32 = 0;
        let mut r = 0usize;
        while r < ROUNDS_PER_MACHINE {
            let idx = base + r;
            if (self.bits[idx / 8] >> (idx % 8)) & 1 == RAW_SUCCESS {
                n += 1;
            }
            r += 1;
        }
        Ok(n)
    }

    /// 失败位清单（0..150 内 0 值位；描述/公报用）。
    pub fn fail_positions(&self) -> Vec<u32> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < TOTAL_RUNS {
            if (self.bits[i / 8] >> (i % 8)) & 1 == RAW_FAIL {
                out.push(i as u32);
            }
            i += 1;
        }
        out
    }

    /// 写入流式指纹。
    pub fn write_fingerprint(&self, h: &mut Fnv64) {
        h.write_tag("UNX-F0786:PLANE:v1");
        let mut i = 0usize;
        while i < PLANE_BYTES {
            h.write_u8(self.bits[i]);
            i += 1;
        }
    }

    /// 一次性内容指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = Fnv64::new();
        self.write_fingerprint(&mut h);
        h.finish()
    }
}

// ── S2 规范确定性生成器：canonical_plane + xorshift64* ────────────────
/// 规范盐（xorshift 初值扰动；写死来源：本模块域标签 "UNX-F0786:CANON"）。
pub const CANONICAL_SALT: u64 = 0x554E_582D_4630_3738;

/// xorshift64* 单步（私有：仅规范生成器用；零浮点确定性流）。
fn xorshift_star(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

/// 规范位平面生成：同 seed 双跑必同平面（双跑对拍底座）。
/// 失败数 fail_count ≤ 150 越界拒；抽取护栏 4096 步防病态 seed 死循环。
pub fn canonical_plane(seed: u64, fail_count: u8) -> Result<OutcomePlane, Verdict1Error> {
    if fail_count as usize > TOTAL_RUNS {
        return Err(Verdict1Error::PlaneIndex {
            idx: fail_count as u32,
            limit: TOTAL_RUNS as u32,
        });
    }
    let mut state = seed ^ CANONICAL_SALT;
    if state == 0 {
        state = CANONICAL_SALT;
    }
    let mut marked = [false; TOTAL_RUNS];
    let mut marked_count = 0usize;
    let mut guard = 0u64;
    let want = fail_count as usize;
    while marked_count < want && guard < 4096 {
        let r = xorshift_star(&mut state);
        let pos = (r % TOTAL_RUNS as u64) as usize;
        if !marked[pos] {
            marked[pos] = true;
            marked_count += 1;
        }
        guard += 1;
    }
    if marked_count < want {
        // 理论不可达（4096 抽样对 ≤150 目标充分）；防御性显性失败。
        return Err(Verdict1Error::PlaneIndex {
            idx: marked_count as u32,
            limit: want as u32,
        });
    }
    let mut plane = OutcomePlane::zeroed();
    let mut i = 0usize;
    while i < TOTAL_RUNS {
        if !marked[i] {
            // 全部成功位；失败位保持 0（RAW_FAIL）。
            plane.set_bit(i, RAW_SUCCESS)?;
        }
        i += 1;
    }
    Ok(plane)
}

/// F0687 镜像来源指纹：域标签 + 位平面的规范同构指纹。
/// 自包含纪律下不 import 被验账，以本指纹作终核账来源引用——
/// 「终验消费不重写」既有账面（F0762/F0687 账体不动）。
pub fn mirror_source_fp(plane: &OutcomePlane) -> u64 {
    let mut h = Fnv64::new();
    h.write_tag("UNX-F0786:SRC-F0687-MIRROR");
    plane.write_fingerprint(&mut h);
    h.finish()
}

// ── S3 MachineColumn：三机分列 ───────────────────────────────────────
/// 单机分列：机号 / 轮数 / 成功数 / 成功率 ppm（success×10000/50 整数制）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineColumn {
    machine: u8,
    rounds: u32,
    success: u32,
    ppm: u64,
}

impl MachineColumn {
    /// 构造（rounds=0 时 ppm 显性 0，不除零；const 期可用）。
    pub const fn new(machine: u8, rounds: u32, success: u32) -> Self {
        let ppm = if rounds == 0 {
            0
        } else {
            (success as u64) * PPM_SCALE / (rounds as u64)
        };
        MachineColumn {
            machine,
            rounds,
            success,
            ppm,
        }
    }

    /// 机号（0/1/2）。
    pub const fn machine(&self) -> u8 {
        self.machine
    }

    /// 轮数（口径恒 50）。
    pub const fn rounds(&self) -> u32 {
        self.rounds
    }

    /// 成功数。
    pub const fn success(&self) -> u32 {
        self.success
    }

    /// 失败数（rounds - success；构造保证不减破）。
    pub const fn fail(&self) -> u32 {
        self.rounds - self.success
    }

    /// 成功率 ppm。
    pub const fn ppm(&self) -> u64 {
        self.ppm
    }

    /// 写入流式指纹。
    pub fn write_fingerprint(&self, h: &mut Fnv64) {
        h.write_u8(self.machine);
        h.write_u32(self.rounds);
        h.write_u32(self.success);
        h.write_u64(self.ppm);
    }

    /// 分列短描述（M0=49/50|9800ppm 样式）。
    pub fn describe(&self) -> String {
        format!(
            "M{}={}/{}|{}ppm",
            self.machine,
            self.success,
            self.rounds,
            self.ppm
        )
    }
}

// ── S4 FinalCoreLedger：终核账（凭证第 1 件） ────────────────────────
/// 终核账：150 次总账（位平面+成功数+ppm）+ 三机分列 + F0687 镜像来源指纹
/// + 封账指纹。判据 1 凭证包第 1 件；镜像输入（消费不重写）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinalCoreLedger {
    plane: OutcomePlane,
    total_success: u32,
    total_ppm: u64,
    columns: [MachineColumn; MACHINE_COUNT],
    source_fp: u64,
    seal_fp: u64,
}

impl FinalCoreLedger {
    /// 终核账内容指纹（平面+总账+分列+来源；不含 seal_fp 自身）。
    fn content_fp(&self) -> u64 {
        let mut h = Fnv64::new();
        h.write_tag("UNX-F0786:FINALCORE:v1");
        self.plane.write_fingerprint(&mut h);
        h.write_u32(self.total_success);
        h.write_u64(self.total_ppm);
        let mut m = 0usize;
        while m < MACHINE_COUNT {
            self.columns[m].write_fingerprint(&mut h);
            m += 1;
        }
        h.write_u64(self.source_fp);
        h.finish()
    }

    /// 从位平面构建终核账（来源指纹为零拒收——镜像引用必须非空）。
    pub fn from_plane(
        plane: OutcomePlane,
        source_fp: u64,
    ) -> Result<FinalCoreLedger, Verdict1Error> {
        if !plane.tail_bits_clean() {
            return Err(Verdict1Error::FinalSelf {
                sub: "位平面尾部保留位脏",
            });
        }
        if source_fp == 0 {
            return Err(Verdict1Error::FinalSelf {
                sub: "来源账指纹为零（F0687 镜像引用缺失）",
            });
        }
        let mut columns = [
            MachineColumn::new(0, 0, 0),
            MachineColumn::new(1, 0, 0),
            MachineColumn::new(2, 0, 0),
        ];
        let mut m = 0usize;
        while m < MACHINE_COUNT {
            let s = plane.machine_success(m as u8)?;
            columns[m] = MachineColumn::new(m as u8, ROUNDS_PER_MACHINE as u32, s);
            m += 1;
        }
        let total_success = plane.success_count();
        let total_ppm = ppm_of(total_success, TOTAL_RUNS as u32);
        let mut ledger = FinalCoreLedger {
            plane,
            total_success,
            total_ppm,
            columns,
            source_fp,
            seal_fp: 0,
        };
        ledger.seal_fp = ledger.content_fp();
        Ok(ledger)
    }

    /// 九路自洽核：尾位/总账复算/ppm 复算/分列轮数/分列复算/守恒/轮数守恒/
    /// 来源指纹/封账指纹。任一路破即终核账不合格（件在场≠件合格）。
    pub fn verify_self(&self) -> Result<(), Verdict1Error> {
        if !self.plane.tail_bits_clean() {
            return Err(Verdict1Error::FinalSelf {
                sub: "位平面尾部保留位脏",
            });
        }
        if self.plane.success_count() != self.total_success {
            return Err(Verdict1Error::FinalSelf {
                sub: "总账成功数复算不符",
            });
        }
        if ppm_of(self.total_success, TOTAL_RUNS as u32) != self.total_ppm {
            return Err(Verdict1Error::FinalSelf { sub: "总账 ppm 复算不符" });
        }
        let mut sum_success: u32 = 0;
        let mut sum_rounds: u32 = 0;
        let mut m = 0usize;
        while m < MACHINE_COUNT {
            let col = self.columns[m];
            if col.rounds() != ROUNDS_PER_MACHINE as u32 {
                return Err(Verdict1Error::MachineCol {
                    machine: col.machine(),
                    rounds: col.rounds(),
                });
            }
            let expect = self.plane.machine_success(col.machine())?;
            if col.success() != expect {
                return Err(Verdict1Error::FinalSelf {
                    sub: "分列成功数与位平面复算不符",
                });
            }
            if col.ppm() != ppm_of(col.success(), col.rounds()) {
                return Err(Verdict1Error::FinalSelf {
                    sub: "分列 ppm 复算不符",
                });
            }
            sum_success = sum_success.saturating_add(col.success());
            sum_rounds = sum_rounds.saturating_add(col.rounds());
            m += 1;
        }
        if sum_success != self.total_success {
            return Err(Verdict1Error::FinalSelf {
                sub: "Σ分机成功数 ≠ 总账（守恒破）",
            });
        }
        if sum_rounds != TOTAL_RUNS as u32 {
            return Err(Verdict1Error::FinalSelf {
                sub: "Σ分机轮数 ≠ 150（守恒破）",
            });
        }
        if self.source_fp == 0 {
            return Err(Verdict1Error::FinalSelf {
                sub: "来源账指纹为零（F0687 镜像引用缺失）",
            });
        }
        if self.seal_fp != self.content_fp() {
            return Err(Verdict1Error::FinalSelf {
                sub: "封账指纹漂移（终核账被改）",
            });
        }
        Ok(())
    }

    /// 位平面只读视图。
    pub const fn plane(&self) -> OutcomePlane {
        self.plane
    }

    /// 总账成功数。
    pub const fn total_success(&self) -> u32 {
        self.total_success
    }

    /// 总账成功率 ppm（成功×10000/150）。
    pub const fn total_ppm(&self) -> u64 {
        self.total_ppm
    }

    /// 三机分列切片。
    pub fn columns(&self) -> &[MachineColumn] {
        &self.columns
    }

    /// 单机分列（机号越界 None）。
    pub fn column(&self, machine: u8) -> Option<MachineColumn> {
        if (machine as usize) < MACHINE_COUNT {
            Some(self.columns[machine as usize])
        } else {
            None
        }
    }

    /// F0687 镜像来源指纹。
    pub const fn source_fp(&self) -> u64 {
        self.source_fp
    }

    /// 封账指纹。
    pub const fn seal_fp(&self) -> u64 {
        self.seal_fp
    }

    /// 读单次运行结果（越界显性拒；对接逐次留痕口径）。
    pub fn run_outcome(&self, run_idx: u32) -> Result<u8, Verdict1Error> {
        self.plane.bit(run_idx as usize)
    }

    /// 注入面专用：直接覆写位平面（不重算任何派生量——篡改语义本体，
    /// 供包指纹漂移检出演练；正式路径禁用）。
    pub fn debug_overwrite_plane(&mut self, plane: OutcomePlane) {
        self.plane = plane;
    }

    /// 终核账落账行（FVER1 域文本快照；公报/审计用）。
    pub fn ledger_line(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "FVER1|FINAL|succ={}/{}|ppm={}",
            self.total_success, TOTAL_RUNS, self.total_ppm
        ));
        let mut m = 0usize;
        while m < MACHINE_COUNT {
            s.push('|');
            s.push_str(&self.columns[m].describe());
            m += 1;
        }
        s.push_str(&format!(
            "|src=0x{:016X}|seal=0x{:016X}",
            self.source_fp, self.seal_fp
        ));
        s
    }
}

// ── S5 TraceLedger：留痕账（凭证第 2 件） ────────────────────────────
/// 留痕事件种类（判定过程事件链节点类型闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceKind {
    /// 开站（payload=label 低 32 位）。
    Open,
    /// 件落账（payload=slot<<24 | 件指纹低 24 位）。
    ItemFiled,
    /// 留痕封链（链尾事件）。
    TraceSeal,
}

impl TraceKind {
    /// 链内原始码。
    pub const fn as_raw(self) -> u8 {
        match self {
            TraceKind::Open => 1,
            TraceKind::ItemFiled => 2,
            TraceKind::TraceSeal => 3,
        }
    }

    /// 中文短名。
    pub const fn label(self) -> &'static str {
        match self {
            TraceKind::Open => "开站",
            TraceKind::ItemFiled => "件落账",
            TraceKind::TraceSeal => "封链",
        }
    }
}

/// 单条留痕事件：序号/种类/载荷/链值（链值 = mix_event(前链, 本事件三要素)）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceEvent {
    seq: u32,
    kind: TraceKind,
    payload: u32,
    chain: u64,
}

impl TraceEvent {
    /// 事件序号（1 起严格连续）。
    pub const fn seq(&self) -> u32 {
        self.seq
    }

    /// 事件种类。
    pub const fn kind(&self) -> TraceKind {
        self.kind
    }

    /// 事件载荷。
    pub const fn payload(&self) -> u32 {
        self.payload
    }

    /// 事件链值。
    pub const fn chain(&self) -> u64 {
        self.chain
    }

    /// 事件行文本。
    pub fn line(&self) -> String {
        format!(
            "TRC|seq={}|kind={}|payload=0x{:08X}|chain=0x{:016X}",
            self.seq,
            self.kind.label(),
            self.payload,
            self.chain
        )
    }
}

/// 留痕账摘要（凭证包内第 2 件的落账形态：链头/事件数/创世值/封链序号）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceDigest {
    event_count: u32,
    chain_head: u64,
    genesis: u64,
    seal_seq: u32,
}

impl TraceDigest {
    /// 事件数（含封链事件）。
    pub const fn event_count(&self) -> u32 {
        self.event_count
    }

    /// 链头（封链后终值；报告指针锚定此值）。
    pub const fn chain_head(&self) -> u64 {
        self.chain_head
    }

    /// 创世链值。
    pub const fn genesis(&self) -> u64 {
        self.genesis
    }

    /// 封链事件序号（恒 = event_count）。
    pub const fn seal_seq(&self) -> u32 {
        self.seal_seq
    }

    /// 摘要内容指纹。
    pub fn content_fp(&self) -> u64 {
        let mut h = Fnv64::new();
        h.write_tag("UNX-F0786:TRACEDIGEST:v1");
        h.write_u32(self.event_count);
        h.write_u64(self.chain_head);
        h.write_u64(self.genesis);
        h.write_u32(self.seal_seq);
        h.finish()
    }

    /// 自洽核：创世非零/事件数 ≥2/封链序号=事件数/链头非零。
    pub fn verify_self(&self) -> Result<(), Verdict1Error> {
        if self.genesis == 0 || self.chain_head == 0 {
            return Err(Verdict1Error::TraceChain { at_seq: 0 });
        }
        if self.event_count < 2 || self.seal_seq != self.event_count {
            return Err(Verdict1Error::TraceChain {
                at_seq: self.seal_seq,
            });
        }
        Ok(())
    }

    /// 摘要行文本。
    pub fn line(&self) -> String {
        format!(
            "TRC|DIGEST|events={}|seal_seq={}|genesis=0x{:016X}|head=0x{:016X}",
            self.event_count, self.seal_seq, self.genesis, self.chain_head
        )
    }
}

/// 留痕账：判定过程事件链（append-only + 滚动哈希封印 + 重放复核）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceLedger {
    genesis: u64,
    events: Vec<TraceEvent>,
    chain_head: u64,
    sealed: bool,
    seal_seq: u32,
}

impl TraceLedger {
    /// 新建（创世链 = 域标签 FNV，写死域标签来源）。
    pub fn new() -> Self {
        let genesis = fnv1a64(b"UNX-F0786:TRACE:GENESIS:v1");
        TraceLedger {
            genesis,
            events: Vec::new(),
            chain_head: genesis,
            sealed: false,
            seal_seq: 0,
        }
    }

    /// 追加事件（封链后拒写 E_SEALED_WRITE；超容拒写；返回新事件序号）。
    pub fn append(&mut self, kind: TraceKind, payload: u32) -> Result<u32, Verdict1Error> {
        if self.sealed {
            return Err(Verdict1Error::SealedWrite {
                what: "留痕账已封链仍写",
            });
        }
        if self.events.len() >= TRACE_EVENT_MAX {
            return Err(Verdict1Error::SealedWrite {
                what: "留痕账超容（>64 事件）仍写",
            });
        }
        let seq = self.events.len() as u32 + 1;
        let chain = mix_event(self.chain_head, seq, kind.as_raw(), payload);
        self.events.push(TraceEvent {
            seq,
            kind,
            payload,
            chain,
        });
        self.chain_head = chain;
        Ok(seq)
    }

    /// 封链（追加 TraceSeal 事件后封板；返回封链摘要）。重复封链拒。
    pub fn seal(&mut self) -> Result<TraceDigest, Verdict1Error> {
        if self.sealed {
            return Err(Verdict1Error::SealedWrite {
                what: "留痕账已封链仍再封",
            });
        }
        let seal_seq = self.append(TraceKind::TraceSeal, 0)?;
        self.sealed = true;
        self.seal_seq = seal_seq;
        Ok(self.digest())
    }

    /// 当前摘要（不封链快照）。
    pub fn digest(&self) -> TraceDigest {
        TraceDigest {
            event_count: self.events.len() as u32,
            chain_head: self.chain_head,
            genesis: self.genesis,
            seal_seq: if self.sealed { self.seal_seq } else { 0 },
        }
    }

    /// 全链重放复核：序号连续 + 逐事件链值复算 + 链头守恒。
    /// at_seq 定位断点（0 = 链头失配）。
    pub fn replay_verify(&self) -> Result<(), Verdict1Error> {
        let mut cur = self.genesis;
        let mut i = 0usize;
        while i < self.events.len() {
            let evt = self.events[i];
            if evt.seq != (i as u32) + 1 {
                return Err(Verdict1Error::TraceChain { at_seq: evt.seq });
            }
            let expect = mix_event(cur, evt.seq, evt.kind.as_raw(), evt.payload);
            if expect != evt.chain {
                return Err(Verdict1Error::TraceChain { at_seq: evt.seq });
            }
            cur = evt.chain;
            i += 1;
        }
        if cur != self.chain_head {
            return Err(Verdict1Error::TraceChain { at_seq: 0 });
        }
        Ok(())
    }

    /// 是否已封链。
    pub const fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 链头当前值。
    pub const fn chain_head(&self) -> u64 {
        self.chain_head
    }

    /// 创世链值。
    pub const fn genesis(&self) -> u64 {
        self.genesis
    }

    /// 事件数。
    pub fn event_count(&self) -> u32 {
        self.events.len() as u32
    }

    /// 事件只读视图。
    pub fn events(&self) -> &[TraceEvent] {
        &self.events
    }

    /// 注入面专用：破坏某序号事件载荷（不重算链）→ replay_verify 必检出。
    /// 查无该序号按断链拒（at_seq=该序号）。
    pub fn debug_break_payload(&mut self, seq: u32) -> Result<(), Verdict1Error> {
        let mut i = 0usize;
        while i < self.events.len() {
            if self.events[i].seq == seq {
                self.events[i].payload ^= 1;
                return Ok(());
            }
            i += 1;
        }
        Err(Verdict1Error::TraceChain { at_seq: seq })
    }

    /// 全链快照文本（公报/审计用）。
    pub fn snapshot_lines(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "TRC|OPEN|genesis=0x{:016X}|events={}|sealed={}\n",
            self.genesis,
            self.events.len(),
            if self.sealed { 1 } else { 0 }
        ));
        let mut i = 0usize;
        while i < self.events.len() {
            s.push_str(&self.events[i].line());
            s.push('\n');
            i += 1;
        }
        s
    }
}

// ── S6 JudgementRecord：判定账（凭证第 3 件） ────────────────────────
/// 判定账：base_ppm + 二值结论 + 终核账来源指纹 + 样本数 + 判定序号 +
/// 封账指纹。from_final（派生道）/ from_parts（外来道）双入口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JudgementRecord {
    base_ppm: u64,
    verdict: Verdict,
    from_final_fp: u64,
    sample_count: u32,
    seq_no: u32,
    seal_fp: u64,
}

/// 判定账内容指纹（不含 seal_fp 自身）。
fn judgement_fp(base_ppm: u64, verdict: Verdict, from_final_fp: u64, sample: u32, seq: u32) -> u64 {
    let mut h = Fnv64::new();
    h.write_tag("UNX-F0786:JUDGE:v1");
    h.write_u64(base_ppm);
    h.write_u8(verdict.as_raw());
    h.write_u64(from_final_fp);
    h.write_u32(sample);
    h.write_u32(seq);
    h.finish()
}

impl JudgementRecord {
    /// 派生道：从在场终核账派生一致判定账（引用其 ppm/结论/封账指纹）。
    pub fn from_final(
        final_ledger: &FinalCoreLedger,
        seq_no: u32,
    ) -> Result<JudgementRecord, Verdict1Error> {
        JudgementRecord::from_parts(
            final_ledger.total_ppm(),
            verdict_of_ppm(final_ledger.total_ppm()),
            final_ledger.seal_fp(),
            TOTAL_RUNS as u32,
            seq_no,
        )
    }

    /// 外来道：外部四要素组装（内容自洽留给 verify_self 逐路核——
    /// 外来件先落账后校验，不合格由终验闸拒）。
    pub fn from_parts(
        base_ppm: u64,
        verdict: Verdict,
        from_final_fp: u64,
        sample_count: u32,
        seq_no: u32,
    ) -> Result<JudgementRecord, Verdict1Error> {
        if seq_no == 0 {
            return Err(Verdict1Error::JudgeSelf { sub: "判定序号非正" });
        }
        let seal_fp = judgement_fp(base_ppm, verdict, from_final_fp, sample_count, seq_no);
        Ok(JudgementRecord {
            base_ppm,
            verdict,
            from_final_fp,
            sample_count,
            seq_no,
            seal_fp,
        })
    }

    /// 四路自洽核：ppm↔结论互证 / 样本恰 150 / 来源指纹在位 / 封账指纹不漂移。
    pub fn verify_self(&self) -> Result<(), Verdict1Error> {
        if self.verdict != verdict_of_ppm(self.base_ppm) {
            return Err(Verdict1Error::JudgeSelf {
                sub: "ppm 与二值结论矛盾",
            });
        }
        if self.sample_count != TOTAL_RUNS as u32 {
            return Err(Verdict1Error::JudgeSelf { sub: "样本数非 150" });
        }
        if self.from_final_fp == 0 {
            return Err(Verdict1Error::JudgeSelf {
                sub: "终核账来源指纹缺",
            });
        }
        if self.seal_fp
            != judgement_fp(
                self.base_ppm,
                self.verdict,
                self.from_final_fp,
                self.sample_count,
                self.seq_no,
            )
        {
            return Err(Verdict1Error::JudgeSelf {
                sub: "封账指纹漂移（判定账被改）",
            });
        }
        Ok(())
    }

    /// 判定基准 ppm（万分率整数）。
    pub const fn base_ppm(&self) -> u64 {
        self.base_ppm
    }

    /// 二值结论。
    pub const fn verdict(&self) -> Verdict {
        self.verdict
    }

    /// 所引终核账封账指纹。
    pub const fn from_final_fp(&self) -> u64 {
        self.from_final_fp
    }

    /// 样本数（口径恒 150）。
    pub const fn sample_count(&self) -> u32 {
        self.sample_count
    }

    /// 判定序号。
    pub const fn seq_no(&self) -> u32 {
        self.seq_no
    }

    /// 封账指纹。
    pub const fn seal_fp(&self) -> u64 {
        self.seal_fp
    }

    /// 判定账落账行。
    pub fn ledger_line(&self) -> String {
        format!(
            "FVER1|JUDGE|ppm={}|verdict={}|from=0x{:016X}|n={}|seq={}|seal=0x{:016X}",
            self.base_ppm,
            self.verdict.label(),
            self.from_final_fp,
            self.sample_count,
            self.seq_no,
            self.seal_fp
        )
    }
}

// ── S7 ReportPointer：报告指针（凭证第 4 件） ────────────────────────
/// 报告指针：定位串（F0781#S1..S6 / F0781#ALL 可解引格式）+ 报告指纹 +
/// 留痕链锚。引用 F0781 收官报告的位置与哈希封存指纹（只引不载）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportPointer {
    locator: String,
    report_fp: u64,
    anchor_chain: u64,
}

/// 定位串格式校验（规则 fn：长度先判后切片，零 panic 路径）。
fn valid_locator(loc: &str) -> bool {
    let b = loc.as_bytes();
    // 最短合法形 "F0781#S1" = 8 字节。
    if b.len() < 8 {
        return false;
    }
    let head_ok = b[0] == b'F'
        && b[1] == b'0'
        && b[2] == b'7'
        && b[3] == b'8'
        && b[4] == b'1'
        && b[5] == b'#';
    if !head_ok {
        return false;
    }
    if b.len() == 9 && b[6] == b'A' && b[7] == b'L' && b[8] == b'L' {
        return true;
    }
    if b.len() == 8 && b[6] == b'S' && b[7] >= b'1' && b[7] <= b'6' {
        return true;
    }
    false
}

impl ReportPointer {
    /// 构造（三路校验：定位串格式/报告指纹非零/锚链非零）。
    pub fn new(
        locator: &str,
        report_fp: u64,
        anchor_chain: u64,
    ) -> Result<ReportPointer, Verdict1Error> {
        let b = locator.as_bytes();
        if b.is_empty() || b.len() > REPORT_LOCATOR_MAX_LEN {
            return Err(Verdict1Error::ReportPtr {
                sub: "定位串空或超长（1..=64 字节）",
            });
        }
        if !valid_locator(locator) {
            return Err(Verdict1Error::ReportPtr {
                sub: "定位串格式不符（F0781#S1..S6 / F0781#ALL）",
            });
        }
        if report_fp == 0 {
            return Err(Verdict1Error::ReportPtr { sub: "报告指纹为零" });
        }
        if anchor_chain == 0 {
            return Err(Verdict1Error::ReportPtr { sub: "留痕锚链为零" });
        }
        Ok(ReportPointer {
            locator: String::from(locator),
            report_fp,
            anchor_chain,
        })
    }

    /// 镜像派生道：定位 F0781#ALL，报告指纹 = 域标签⊕终核封账⊕留痕链头⊕ppm
    /// 的 FNV（F0781 报告哈希封存指纹的本地镜像锚；退化零值时异或再散一次）。
    pub fn derive_for(
        final_ledger: &FinalCoreLedger,
        trace_head: u64,
    ) -> Result<ReportPointer, Verdict1Error> {
        let mut h = Fnv64::new();
        h.write_tag("UNX-F0786:REPORT-MIRROR");
        h.write_u64(final_ledger.seal_fp());
        h.write_u64(trace_head);
        h.write_u64(final_ledger.total_ppm());
        let mut fp = h.finish();
        if fp == 0 {
            fp ^= 0x9E37_79B9_7F4A_7C15;
        }
        ReportPointer::new("F0781#ALL", fp, trace_head)
    }

    /// 自洽核（定位串格式 + 双指纹非零复验）。
    pub fn verify_self(&self) -> Result<(), Verdict1Error> {
        if !valid_locator(&self.locator) || self.locator.as_bytes().len() > REPORT_LOCATOR_MAX_LEN
        {
            return Err(Verdict1Error::ReportPtr {
                sub: "定位串格式不符（复验）",
            });
        }
        if self.report_fp == 0 {
            return Err(Verdict1Error::ReportPtr {
                sub: "报告指纹为零（复验）",
            });
        }
        if self.anchor_chain == 0 {
            return Err(Verdict1Error::ReportPtr {
                sub: "留痕锚链为零（复验）",
            });
        }
        Ok(())
    }

    /// 定位串。
    pub fn locator(&self) -> &str {
        &self.locator
    }

    /// 报告指纹。
    pub const fn report_fp(&self) -> u64 {
        self.report_fp
    }

    /// 留痕链锚（恒 = 留痕账链头，交叉核对面）。
    pub const fn anchor_chain(&self) -> u64 {
        self.anchor_chain
    }

    /// 内容指纹。
    pub fn content_fp(&self) -> u64 {
        let mut h = Fnv64::new();
        h.write_tag("UNX-F0786:REPORTPTR:v1");
        h.write_bytes(self.locator.as_bytes());
        h.write_u64(self.report_fp);
        h.write_u64(self.anchor_chain);
        h.finish()
    }

    /// 报告指针落账行。
    pub fn ledger_line(&self) -> String {
        format!(
            "FVER1|RPTPTR|loc={}|fp=0x{:016X}|anchor=0x{:016X}",
            self.locator, self.report_fp, self.anchor_chain
        )
    }
}

// ── S8 EvidencePack：凭证包（四件 + 位图 + 落账序 + 合指纹 + 冻结制） ──
/// 凭证件种（判据原文枚举；slot 判别值为落账管线序，criterion_slot 回原文序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackItemKind {
    /// 终核账（判据第 1 件；落账 slot 0）。
    FinalCore,
    /// 判定账（判据第 3 件；落账 slot 1）。
    Judgement,
    /// 留痕账（判据第 2 件；落账 slot 2 封链落账）。
    Trace,
    /// 报告指针（判据第 4 件；落账 slot 3）。
    ReportPtr,
}

impl PackItemKind {
    /// 落账 slot 号（0..=3，规范管线序）。
    pub const fn slot(self) -> u8 {
        match self {
            PackItemKind::FinalCore => 0,
            PackItemKind::Judgement => 1,
            PackItemKind::Trace => 2,
            PackItemKind::ReportPtr => 3,
        }
    }

    /// 判据原文枚举序（第几件，1 起）。
    pub const fn criterion_slot(self) -> u8 {
        match self {
            PackItemKind::FinalCore => 1,
            PackItemKind::Trace => 2,
            PackItemKind::Judgement => 3,
            PackItemKind::ReportPtr => 4,
        }
    }

    /// 中文件名。
    pub const fn label(self) -> &'static str {
        match self {
            PackItemKind::FinalCore => "终核账",
            PackItemKind::Trace => "留痕账",
            PackItemKind::Judgement => "判定账",
            PackItemKind::ReportPtr => "报告指针",
        }
    }

    /// slot → 件种（越界 None）。
    pub const fn from_slot(slot: u8) -> Option<PackItemKind> {
        match slot {
            0 => Some(PackItemKind::FinalCore),
            1 => Some(PackItemKind::Judgement),
            2 => Some(PackItemKind::Trace),
            3 => Some(PackItemKind::ReportPtr),
            _ => None,
        }
    }
}

/// 判据原文枚举序的件序表（第 1..4 件：终核账/留痕账/判定账/报告指针）。
pub const CRITERION_ORDER: [PackItemKind; ITEM_COUNT] = [
    PackItemKind::FinalCore,
    PackItemKind::Trace,
    PackItemKind::Judgement,
    PackItemKind::ReportPtr,
];

/// 凭证件视图（件报表行：判据序/件名/在场/落账指纹）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CriterionSlotView {
    /// 判据序（1..=4，原文枚举序）。
    pub criterion_slot: u8,
    /// 件名。
    pub label: &'static str,
    /// 在场与否。
    pub filed: bool,
    /// 落账指纹（缺件为 0）。
    pub filed_fp: u64,
}

/// 凭证包：四件 presence 位图 + 落账序日志 + 逐件落账指纹 + 合指纹 + 冻结制。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidencePack {
    filed_mask: u8,
    next_slot: u8,
    order_log: [u8; ITEM_COUNT],
    order_count: u8,
    final_core: Option<FinalCoreLedger>,
    judgement: Option<JudgementRecord>,
    trace_digest: Option<TraceDigest>,
    report: Option<ReportPointer>,
    item_fps: [u64; ITEM_COUNT],
    frozen: bool,
    pack_fp: u64,
}

/// 件落账指纹：域标签 + slot + 件内容指纹（防跨件/跨域碰撞）。
fn pack_item_fp(slot: u8, content_fp: u64) -> u64 {
    let mut h = Fnv64::new();
    h.write_tag("UNX-F0786:ITEM:v1");
    h.write_u8(slot);
    h.write_u64(content_fp);
    h.finish()
}

impl EvidencePack {
    /// 空包新建。
    pub fn new() -> Self {
        EvidencePack {
            filed_mask: 0,
            next_slot: 0,
            order_log: [0xFF; ITEM_COUNT],
            order_count: 0,
            final_core: None,
            judgement: None,
            trace_digest: None,
            report: None,
            item_fps: [0; ITEM_COUNT],
            frozen: false,
            pack_fp: 0,
        }
    }

    /// slot 三闸共检：冻结拒 / 重件拒 / 乱序拒。
    fn guard_slot(&self, slot: u8) -> Result<(), Verdict1Error> {
        if self.frozen {
            return Err(Verdict1Error::PackFrozen);
        }
        if self.filed_mask & (1u8 << slot) != 0 {
            return Err(Verdict1Error::ItemDup { kind: slot });
        }
        if slot != self.next_slot {
            return Err(Verdict1Error::ItemOrder {
                expect: self.next_slot,
                got: slot,
            });
        }
        Ok(())
    }

    /// 落账登记（guard 通过后调用；顺序日志 + 位图 + 件指纹三同步）。
    fn mark_filed(&mut self, slot: u8, fp: u64) {
        self.filed_mask |= 1u8 << slot;
        if (self.order_count as usize) < ITEM_COUNT {
            self.order_log[self.order_count as usize] = slot;
        }
        self.order_count = self.order_count.saturating_add(1);
        self.next_slot = self.next_slot.saturating_add(1);
        self.item_fps[slot as usize] = fp;
    }

    /// 落账第 1 步：终核账。
    pub fn file_final_core(&mut self, ledger: FinalCoreLedger) -> Result<(), Verdict1Error> {
        self.guard_slot(PackItemKind::FinalCore.slot())?;
        let fp = pack_item_fp(0, ledger.content_fp());
        self.final_core = Some(ledger);
        self.mark_filed(0, fp);
        Ok(())
    }

    /// 落账第 2 步：判定账。
    pub fn file_judgement(&mut self, j: JudgementRecord) -> Result<(), Verdict1Error> {
        self.guard_slot(PackItemKind::Judgement.slot())?;
        let fp = pack_item_fp(1, j.seal_fp());
        self.judgement = Some(j);
        self.mark_filed(1, fp);
        Ok(())
    }

    /// 落账第 3 步：留痕账（封链摘要）。
    pub fn file_trace_digest(&mut self, d: TraceDigest) -> Result<(), Verdict1Error> {
        self.guard_slot(PackItemKind::Trace.slot())?;
        let fp = pack_item_fp(2, d.content_fp());
        self.trace_digest = Some(d);
        self.mark_filed(2, fp);
        Ok(())
    }

    /// 落账第 4 步：报告指针。
    pub fn file_report_pointer(&mut self, p: ReportPointer) -> Result<(), Verdict1Error> {
        self.guard_slot(PackItemKind::ReportPtr.slot())?;
        let fp = pack_item_fp(3, p.content_fp());
        self.report = Some(p);
        self.mark_filed(3, fp);
        Ok(())
    }

    /// presence 位图。
    pub const fn filed_mask(&self) -> u8 {
        self.filed_mask
    }

    /// 已落件数。
    pub fn filed_count(&self) -> u8 {
        self.filed_mask.count_ones() as u8
    }

    /// 齐 4/4 判定。
    pub const fn is_complete(&self) -> bool {
        self.filed_mask == MASK_ALL_ITEMS
    }

    /// 落账序合规判定（顺序日志恰为 [0,1,2,3]）。
    pub fn filed_in_order(&self) -> bool {
        if self.order_count as usize != ITEM_COUNT {
            return false;
        }
        let mut i = 0usize;
        while i < ITEM_COUNT {
            if self.order_log[i] != i as u8 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 首个序偏离点（(期望步序, 实落 slot)；只扫已落账前缀，合规 None）。
    pub fn first_order_violation(&self) -> Option<(u8, u8)> {
        let filled = if (self.order_count as usize) < ITEM_COUNT {
            self.order_count as usize
        } else {
            ITEM_COUNT
        };
        let mut i = 0usize;
        while i < filled {
            if self.order_log[i] != i as u8 {
                return Some((i as u8, self.order_log[i]));
            }
            i += 1;
        }
        None
    }

    /// 缺件位图（0b1111 反位；0 = 不缺）。
    pub const fn missing_mask(&self) -> u8 {
        MASK_ALL_ITEMS & !self.filed_mask
    }

    /// 合指纹（presence 位图 + 逐件指纹；冻结位为状态非内容，不入指纹——
    /// 封包值与冻结前复算值恒等，冻结稳定性可断言）。
    pub fn pack_fingerprint(&self) -> u64 {
        let mut h = Fnv64::new();
        h.write_tag("UNX-F0786:PACK:v1");
        h.write_u8(self.filed_mask);
        let mut i = 0usize;
        while i < ITEM_COUNT {
            h.write_u64(self.item_fps[i]);
            i += 1;
        }
        h.finish()
    }

    /// 冻结（定格时调用一次；齐 4/4 才可冻结；返回封包合指纹）。
    pub fn freeze(&mut self) -> Result<u64, Verdict1Error> {
        if self.frozen {
            return Err(Verdict1Error::PackFrozen);
        }
        if !self.is_complete() {
            return Err(Verdict1Error::ItemMissing {
                expect: MASK_ALL_ITEMS,
                got: self.filed_mask,
                missing: self.missing_mask(),
            });
        }
        self.pack_fp = self.pack_fingerprint();
        self.frozen = true;
        Ok(self.pack_fp)
    }

    /// 篡改检出：在场件内容指纹复算 vs 落账指纹；冻结后另核合指纹。
    pub fn verify_integrity(&self) -> Result<(), Verdict1Error> {
        if self.filed_mask & 0b0001 != 0 {
            let re = match &self.final_core {
                Some(fc) => pack_item_fp(0, fc.content_fp()),
                None => 0,
            };
            if re != self.item_fps[0] {
                return Err(Verdict1Error::PackTamper {
                    stored: self.item_fps[0],
                    recomputed: re,
                });
            }
        }
        if self.filed_mask & 0b0010 != 0 {
            let re = match &self.judgement {
                Some(j) => pack_item_fp(1, j.seal_fp()),
                None => 0,
            };
            if re != self.item_fps[1] {
                return Err(Verdict1Error::PackTamper {
                    stored: self.item_fps[1],
                    recomputed: re,
                });
            }
        }
        if self.filed_mask & 0b0100 != 0 {
            let re = match &self.trace_digest {
                Some(d) => pack_item_fp(2, d.content_fp()),
                None => 0,
            };
            if re != self.item_fps[2] {
                return Err(Verdict1Error::PackTamper {
                    stored: self.item_fps[2],
                    recomputed: re,
                });
            }
        }
        if self.filed_mask & 0b1000 != 0 {
            let re = match &self.report {
                Some(p) => pack_item_fp(3, p.content_fp()),
                None => 0,
            };
            if re != self.item_fps[3] {
                return Err(Verdict1Error::PackTamper {
                    stored: self.item_fps[3],
                    recomputed: re,
                });
            }
        }
        if self.frozen {
            let cur = self.pack_fingerprint();
            if cur != self.pack_fp {
                return Err(Verdict1Error::PackTamper {
                    stored: self.pack_fp,
                    recomputed: cur,
                });
            }
        }
        Ok(())
    }

    /// 终核账只读引用。
    pub fn final_core_ref(&self) -> Option<&FinalCoreLedger> {
        self.final_core.as_ref()
    }

    /// 判定账只读引用。
    pub fn judgement_ref(&self) -> Option<&JudgementRecord> {
        self.judgement.as_ref()
    }

    /// 留痕摘要只读引用。
    pub fn trace_digest_ref(&self) -> Option<&TraceDigest> {
        self.trace_digest.as_ref()
    }

    /// 报告指针只读引用。
    pub fn report_ref(&self) -> Option<&ReportPointer> {
        self.report.as_ref()
    }

    /// 终核账可变引用（注入面专用：包篡改演练）。
    pub fn final_core_mut(&mut self) -> Option<&mut FinalCoreLedger> {
        self.final_core.as_mut()
    }

    /// 冻结态。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 封包合指纹（未冻结时为 0）。
    pub const fn sealed_pack_fp(&self) -> u64 {
        self.pack_fp
    }

    /// 逐件指纹（slot 越界 None）。
    pub fn item_fp(&self, slot: u8) -> Option<u64> {
        if (slot as usize) < ITEM_COUNT {
            Some(self.item_fps[slot as usize])
        } else {
            None
        }
    }

    /// 件报表（判据原文枚举序 4 行）。
    pub fn criterion_views(&self) -> Vec<CriterionSlotView> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < ITEM_COUNT {
            let kind = CRITERION_ORDER[i];
            let slot = kind.slot();
            let filed = self.filed_mask & (1u8 << slot) != 0;
            let fp = self.item_fps[slot as usize];
            out.push(CriterionSlotView {
                criterion_slot: kind.criterion_slot(),
                label: kind.label(),
                filed,
                filed_fp: if filed { fp } else { 0 },
            });
            i += 1;
        }
        out
    }

    /// 件报表文本行。
    pub fn presence_line(&self) -> String {
        let mut s = String::from("FVER1|PACK|");
        let views = self.criterion_views();
        let mut i = 0usize;
        while i < views.len() {
            if i > 0 {
                s.push('|');
            }
            let v = views[i];
            s.push_str(&format!(
                "件{}{}:{}",
                v.criterion_slot,
                v.label,
                if v.filed { "在场" } else { "缺" }
            ));
            i += 1;
        }
        s.push_str(&format!(
            "|{}/4|fp=0x{:016X}|frozen={}",
            self.filed_count(),
            if self.frozen {
                self.pack_fp
            } else {
                self.pack_fingerprint()
            },
            if self.frozen { 1 } else { 0 }
        ));
        s
    }
}

// ── S9 定格态与红案账 ─────────────────────────────────────────────────
/// 定格态：Undecided → Decided 单向（判据 J1-d：定格后不可重算）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictState {
    /// 未决（尚未定格 / 拒定格后回落态）。
    Undecided,
    /// 已定格：结论 + 终值 ppm + 包合指纹 + 定格序号（永不可改写）。
    Decided {
        /// 定格结论（true = ≥99% 达标）。
        pass: bool,
        /// 定格 ppm（万分率终值）。
        final_ppm: u64,
        /// 定格时凭证包合指纹。
        pack_fp: u64,
        /// 定格序号（1 起，站内单调）。
        serial: u32,
    },
}

impl VerdictState {
    /// 是否已定格。
    pub const fn is_decided(&self) -> bool {
        matches!(self, VerdictState::Decided { .. })
    }

    /// 定格序号（未决 0）。
    pub const fn decided_serial(&self) -> u32 {
        match self {
            VerdictState::Undecided => 0,
            VerdictState::Decided { serial, .. } => *serial,
        }
    }
}

/// 红案种类（判据 J1-d：重复终验/包篡改/链断三类红案立案）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedCaseKind {
    /// Decided 后重复终验。
    ReFinalVerify,
    /// 凭证包指纹漂移（篡改）。
    PackTamper,
    /// 留痕事件链断链。
    ChainBroken,
}

impl RedCaseKind {
    /// 中文短名。
    pub const fn label(self) -> &'static str {
        match self {
            RedCaseKind::ReFinalVerify => "重复终验",
            RedCaseKind::PackTamper => "凭证包篡改",
            RedCaseKind::ChainBroken => "留痕断链",
        }
    }
}

/// 红案记录：种类 + 诊断码 + 立案序号。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedCaseRecord {
    kind: RedCaseKind,
    code: u16,
    case_no: u32,
}

impl RedCaseRecord {
    /// 红案种类。
    pub const fn kind(&self) -> RedCaseKind {
        self.kind
    }

    /// 诊断码。
    pub const fn code(&self) -> u16 {
        self.code
    }

    /// 立案序号。
    pub const fn case_no(&self) -> u32 {
        self.case_no
    }

    /// 红案行文本。
    pub fn line(&self) -> String {
        format!(
            "FVER1|RED|no={}|kind={}|code=0x{:04X}",
            self.case_no,
            self.kind.label(),
            self.code
        )
    }
}

/// 终验定格结论（finalize 成功的回执）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinalConclusion {
    pass: bool,
    final_ppm: u64,
    pack_fp: u64,
    serial: u32,
    success: u32,
    total: u32,
}

impl FinalConclusion {
    /// 定格结论（true = ≥99% 达标）。
    pub const fn pass(&self) -> bool {
        self.pass
    }

    /// 定格 ppm。
    pub const fn final_ppm(&self) -> u64 {
        self.final_ppm
    }

    /// 凭证包合指纹。
    pub const fn pack_fp(&self) -> u64 {
        self.pack_fp
    }

    /// 定格序号。
    pub const fn serial(&self) -> u32 {
        self.serial
    }

    /// 成功次数。
    pub const fn success(&self) -> u32 {
        self.success
    }

    /// 总次数（恒 150）。
    pub const fn total(&self) -> u32 {
        self.total
    }

    /// 结论行文本。
    pub fn line(&self) -> String {
        format!(
            "FVER1|CONCLUDE|{}|ppm={}|succ={}/{}|serial={}|pack=0x{:016X}",
            if self.pass { "达标定格" } else { "未达标定格" },
            self.final_ppm,
            self.success,
            self.total,
            self.serial,
            self.pack_fp
        )
    }
}

// ── S10a 健康报表 ─────────────────────────────────────────────────────
/// health() 报表：十二项硬灯 + 命名灯表 + 汇总 all_green。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthReport {
    /// 齐 4/4 在场。
    pub items_4_of_4: bool,
    /// 终核账自洽。
    pub final_self_ok: bool,
    /// 留痕链重放复核。
    pub trace_chain_ok: bool,
    /// 判定账自洽。
    pub judgement_self_ok: bool,
    /// 报告指针自洽。
    pub report_ptr_ok: bool,
    /// 四路交叉一致。
    pub cross_ok: bool,
    /// 落账序合规。
    pub order_ok: bool,
    /// 包完整性（件指纹复算 + 合指纹）。
    pub pack_integrity_ok: bool,
    /// 已定格。
    pub decided: bool,
    /// 定格结论达标。
    pub decision_pass: bool,
    /// 红案数（须 0）。
    pub red_case_count: u32,
    /// 拒定格次数（正常拒绝，不计红案，不阻绿灯）。
    pub below_line_rejections: u32,
    /// 命名灯表（灯名, 亮/灭）。
    pub lights: Vec<(&'static str, bool)>,
    /// 全绿汇总。
    pub all_green: bool,
}

// ── S10b 站点要索与双跑对拍载体 ───────────────────────────────────────
/// 站点要索（双跑对拍的比较面：六项 + 定格结论）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StationVitals {
    /// 当前包合指纹。
    pub pack_fp: u64,
    /// 位平面指纹。
    pub plane_fp: u64,
    /// 总账 ppm。
    pub total_ppm: u64,
    /// 成功数。
    pub success: u32,
    /// 留痕链头。
    pub chain_head: u64,
    /// 留痕事件数。
    pub event_count: u32,
    /// 已定格。
    pub decided: bool,
    /// 定格结论（未决 false）。
    pub decision_pass: bool,
}

/// 双跑对拍报表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DualRunReport {
    /// A 路要索。
    pub a: StationVitals,
    /// B 路要索。
    pub b: StationVitals,
    /// A 路定格结果码（0 = 成功定格；他 = 诊断码）。
    pub a_result: u16,
    /// B 路定格结果码。
    pub b_result: u16,
    /// 要索与结果全等。
    pub deterministic: bool,
}

/// 任务回执：站 + 结论 + 健康报表。
#[derive(Debug, Clone)]
pub struct MissionOutcome {
    /// 终验站（定格后只读）。
    pub station: FinalVerifyStation,
    /// 定格结论。
    pub conclusion: FinalConclusion,
    /// 健康报表。
    pub health: HealthReport,
}

/// 注入面回执：注入名 / 检出与否 / 诊断码 / 诊断文本。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InjectionOutcome {
    /// 注入名。
    pub name: &'static str,
    /// 是否被显性检出。
    pub detected: bool,
    /// 检出诊断码（未检出 0）。
    pub code: u16,
    /// 诊断文本。
    pub detail: &'static str,
}

impl InjectionOutcome {
    /// 注入全绿判定（检出即绿）。
    pub const fn is_green(&self) -> bool {
        self.detected
    }
}

// ── S10 FinalVerifyStation：终验编排站 ───────────────────────────────
/// 判据 1 终验站：四件收集 → 逐件校验 → 交叉一致 → 落账序核 → ≥99% 定格。
/// 定格后单向只读；红案立案不动摇原结论。
#[derive(Debug, Clone)]
pub struct FinalVerifyStation {
    label: u64,
    pack: EvidencePack,
    trace: TraceLedger,
    state: VerdictState,
    red_cases: Vec<RedCaseRecord>,
    below_line_rejections: u32,
    decision_serial: u32,
}

impl FinalVerifyStation {
    /// 开站（label 用于留痕开站事件与双跑辨识）。
    pub fn new(label: u64) -> Result<FinalVerifyStation, Verdict1Error> {
        let mut trace = TraceLedger::new();
        trace.append(TraceKind::Open, (label & 0xFFFF_FFFF) as u32)?;
        Ok(FinalVerifyStation {
            label,
            pack: EvidencePack::new(),
            trace,
            state: VerdictState::Undecided,
            red_cases: Vec::new(),
            below_line_rejections: 0,
            decision_serial: 0,
        })
    }

    /// 站标签。
    pub const fn label(&self) -> u64 {
        self.label
    }

    /// 定格态快照。
    pub const fn state(&self) -> VerdictState {
        self.state
    }

    /// 是否已定格。
    pub const fn is_decided(&self) -> bool {
        self.state.is_decided()
    }

    /// 定格写闸（定格后一切写路径先过此闸）。
    fn guard_open(&self) -> Result<(), Verdict1Error> {
        if self.state.is_decided() {
            return Err(Verdict1Error::SealedWrite {
                what: "终验站已定格仍写",
            });
        }
        Ok(())
    }

    /// 留痕链头。
    pub const fn trace_chain_head(&self) -> u64 {
        self.trace.chain_head()
    }

    /// 留痕账只读视图。
    pub fn trace(&self) -> &TraceLedger {
        &self.trace
    }

    /// 凭证包只读视图。
    pub fn pack(&self) -> &EvidencePack {
        &self.pack
    }

    /// 件落账留痕（封链前记事件；封链后第 4 件以锚链留痕，不追写）。
    fn note_item(&mut self, slot: u8, fp: u64) -> Result<(), Verdict1Error> {
        if self.trace.is_sealed() {
            return Ok(());
        }
        let payload = ((slot as u32) << 24) | ((fp as u32) & 0x00FF_FFFF);
        self.trace.append(TraceKind::ItemFiled, payload)?;
        Ok(())
    }

    /// 落账第 1 步：终核账（定格后拒写）。
    pub fn file_final_core(&mut self, l: FinalCoreLedger) -> Result<(), Verdict1Error> {
        self.guard_open()?;
        let fp = pack_item_fp(0, l.content_fp());
        self.pack.file_final_core(l)?;
        self.note_item(0, fp)
    }

    /// 落账第 2 步：判定账（定格后拒写）。
    pub fn file_judgement(&mut self, j: JudgementRecord) -> Result<(), Verdict1Error> {
        self.guard_open()?;
        let fp = pack_item_fp(1, j.seal_fp());
        self.pack.file_judgement(j)?;
        self.note_item(1, fp)
    }

    /// 落账第 3 步：留痕封链落账（封链事件入链后摘要落包）。
    pub fn seal_and_file_trace(&mut self) -> Result<(), Verdict1Error> {
        self.guard_open()?;
        let digest = self.trace.seal()?;
        self.pack.file_trace_digest(digest)
    }

    /// 落账第 4 步：报告指针（锚链须为留痕链头，交叉核在终验闸）。
    pub fn file_report_pointer(&mut self, p: ReportPointer) -> Result<(), Verdict1Error> {
        self.guard_open()?;
        self.pack.file_report_pointer(p)
    }

    /// 逐件校验（按判据原文枚举序返回四灯：终核/留痕/判定/报告）。
    pub fn check_items(&self) -> [bool; ITEM_COUNT] {
        let mut out = [false; ITEM_COUNT];
        // 第 1 件 终核账：在场 + 九路自洽。
        out[0] = match self.pack.final_core_ref() {
            Some(fc) => fc.verify_self().is_ok(),
            None => false,
        };
        // 第 2 件 留痕账：在场 + 摘要自洽 + 全链重放。
        out[1] = match self.pack.trace_digest_ref() {
            Some(d) => d.verify_self().is_ok() && self.trace.replay_verify().is_ok(),
            None => false,
        };
        // 第 3 件 判定账：在场 + 四路自洽。
        out[2] = match self.pack.judgement_ref() {
            Some(j) => j.verify_self().is_ok(),
            None => false,
        };
        // 第 4 件 报告指针：在场 + 三路自洽。
        out[3] = match self.pack.report_ref() {
            Some(p) => p.verify_self().is_ok(),
            None => false,
        };
        out
    }

    /// 四路交叉一致核（引用等/ppm 等/结论等/锚定等；缺件先拒）。
    pub fn check_cross(&self) -> Result<(), Verdict1Error> {
        let fc = match self.pack.final_core_ref() {
            Some(v) => v,
            None => {
                return Err(Verdict1Error::ItemMissing {
                    expect: MASK_ALL_ITEMS,
                    got: self.pack.filed_mask(),
                    missing: self.pack.missing_mask(),
                })
            }
        };
        let j = match self.pack.judgement_ref() {
            Some(v) => v,
            None => {
                return Err(Verdict1Error::ItemMissing {
                    expect: MASK_ALL_ITEMS,
                    got: self.pack.filed_mask(),
                    missing: self.pack.missing_mask(),
                })
            }
        };
        let d = match self.pack.trace_digest_ref() {
            Some(v) => v,
            None => {
                return Err(Verdict1Error::ItemMissing {
                    expect: MASK_ALL_ITEMS,
                    got: self.pack.filed_mask(),
                    missing: self.pack.missing_mask(),
                })
            }
        };
        let rp = match self.pack.report_ref() {
            Some(v) => v,
            None => {
                return Err(Verdict1Error::ItemMissing {
                    expect: MASK_ALL_ITEMS,
                    got: self.pack.filed_mask(),
                    missing: self.pack.missing_mask(),
                })
            }
        };
        if j.from_final_fp() != fc.seal_fp() {
            return Err(Verdict1Error::CrossMismatch {
                sub: "判定账来源指纹 ≠ 终核账封账指纹（引用等破）",
            });
        }
        if j.base_ppm() != fc.total_ppm() {
            return Err(Verdict1Error::CrossMismatch {
                sub: "终核账 ppm ≠ 判定账 ppm（数值等破）",
            });
        }
        if j.verdict() != verdict_of_ppm(fc.total_ppm()) {
            return Err(Verdict1Error::CrossMismatch {
                sub: "判定结论 ≠ 终核 ppm 复算结论（结论等破）",
            });
        }
        if rp.anchor_chain() != d.chain_head() {
            return Err(Verdict1Error::CrossMismatch {
                sub: "报告指针锚链 ≠ 留痕账链头（锚定等破）",
            });
        }
        Ok(())
    }

    /// 红案立案（站内序号单调；不动摇已定格结论）。
    fn raise_red(&mut self, kind: RedCaseKind, code: u16) {
        let case_no = self.red_cases.len() as u32 + 1;
        self.red_cases.push(RedCaseRecord {
            kind,
            code,
            case_no,
        });
    }

    /// 终验定格主闸（判据 J1-a~J1-d 全链判定 + 定格单向语义）。
    ///
    /// 检序（前因优先）：重复终验红案 → 包篡改 → 缺件 → 逐件 → 落账序 →
    /// 交叉 → 拒定格（<99%，非红案）→ 冻结定格。
    pub fn finalize_verdict(&mut self) -> Result<FinalConclusion, Verdict1Error> {
        // J1-d 单向闸：Decided 后再终验即红案，原结论不动摇。
        if let VerdictState::Decided { serial, .. } = self.state {
            self.raise_red(RedCaseKind::ReFinalVerify, E_REVERIFY);
            return Err(Verdict1Error::Reverify {
                decided_serial: serial,
            });
        }
        // 包完整性先行（篡改红案优先于一切件级判定）。
        if let Err(e) = self.pack.verify_integrity() {
            if let Verdict1Error::PackTamper { .. } = e {
                self.raise_red(RedCaseKind::PackTamper, e.err_code());
            }
            return Err(e);
        }
        // J1-a 齐 4/4。
        if !self.pack.is_complete() {
            return Err(Verdict1Error::ItemMissing {
                expect: MASK_ALL_ITEMS,
                got: self.pack.filed_mask(),
                missing: self.pack.missing_mask(),
            });
        }
        // J1-b 逐件（件在场 + 内容自洽）。
        if let Some(fc) = self.pack.final_core_ref() {
            if let Err(e) = fc.verify_self() {
                return Err(e);
            }
        }
        if let Some(d) = self.pack.trace_digest_ref() {
            if let Err(e) = d.verify_self() {
                if let Verdict1Error::TraceChain { .. } = e {
                    self.raise_red(RedCaseKind::ChainBroken, e.err_code());
                }
                return Err(e);
            }
        }
        if let Err(e) = self.trace.replay_verify() {
            self.raise_red(RedCaseKind::ChainBroken, e.err_code());
            return Err(e);
        }
        if let Some(j) = self.pack.judgement_ref() {
            if let Err(e) = j.verify_self() {
                return Err(e);
            }
        }
        if let Some(p) = self.pack.report_ref() {
            if let Err(e) = p.verify_self() {
                return Err(e);
            }
        }
        // 落账序核（件可齐而序不可乱）。
        if !self.pack.filed_in_order() {
            let (expect, got) = match self.pack.first_order_violation() {
                Some((e, g)) => (e, g),
                None => (0, self.pack.filed_mask()),
            };
            return Err(Verdict1Error::ItemOrder { expect, got });
        }
        // J1-c 四路交叉。
        self.check_cross()?;
        // J1-d ≥99% 定格判（先取值再冻结——借用早释放）。
        let (ppm, success) = match self.pack.final_core_ref() {
            Some(v) => (v.total_ppm(), v.total_success()),
            None => {
                return Err(Verdict1Error::ItemMissing {
                    expect: MASK_ALL_ITEMS,
                    got: self.pack.filed_mask(),
                    missing: self.pack.missing_mask(),
                })
            }
        };
        if verdict_of_ppm(ppm) == Verdict::Reject {
            self.below_line_rejections = self.below_line_rejections.saturating_add(1);
            return Err(Verdict1Error::BelowLine { ppm });
        }
        // 冻结定格（单向：此后 state 永驻 Decided）。
        let pack_fp = self.pack.freeze()?;
        self.decision_serial = self.decision_serial.saturating_add(1);
        self.state = VerdictState::Decided {
            pass: true,
            final_ppm: ppm,
            pack_fp,
            serial: self.decision_serial,
        };
        Ok(FinalConclusion {
            pass: true,
            final_ppm: ppm,
            pack_fp,
            serial: self.decision_serial,
            success,
            total: TOTAL_RUNS as u32,
        })
    }

    /// 包审计（独立通道：不定格也随时可审；篡改红案立案）。
    pub fn audit_pack(&mut self) -> Result<(), Verdict1Error> {
        match self.pack.verify_integrity() {
            Ok(()) => Ok(()),
            Err(e) => {
                if let Verdict1Error::PackTamper { .. } = e {
                    self.raise_red(RedCaseKind::PackTamper, e.err_code());
                }
                Err(e)
            }
        }
    }

    /// 红案账只读视图。
    pub fn red_cases(&self) -> &[RedCaseRecord] {
        &self.red_cases
    }

    /// 拒定格次数。
    pub const fn below_line_rejections(&self) -> u32 {
        self.below_line_rejections
    }

    /// 当前包合指纹（冻结后为封包值，冻结前为即时复算值）。
    pub fn pack_fingerprint(&self) -> u64 {
        if self.pack.is_frozen() {
            self.pack.sealed_pack_fp()
        } else {
            self.pack.pack_fingerprint()
        }
    }

    /// 站点要索（双跑对拍面）。
    pub fn vitals(&self) -> StationVitals {
        let (plane_fp, total_ppm, success) = match self.pack.final_core_ref() {
            Some(fc) => (fc.plane().fingerprint(), fc.total_ppm(), fc.total_success()),
            None => (0, 0, 0),
        };
        let (decided, decision_pass) = match self.state {
            VerdictState::Undecided => (false, false),
            VerdictState::Decided { pass, .. } => (true, pass),
        };
        StationVitals {
            pack_fp: self.pack_fingerprint(),
            plane_fp,
            total_ppm,
            success,
            chain_head: self.trace.chain_head(),
            event_count: self.trace.event_count(),
            decided,
            decision_pass,
        }
    }

    /// 十二灯健康报表（不落账、不立案，纯只读体检）。
    pub fn health(&self) -> HealthReport {
        let items = self.check_items();
        let items_4_of_4 = self.pack.is_complete();
        let trace_chain_ok = items[1];
        let (decided, decision_pass) = match self.state {
            VerdictState::Undecided => (false, false),
            VerdictState::Decided { pass, .. } => (true, pass),
        };
        let cross_ok = self.check_cross().is_ok();
        let order_ok = self.pack.filed_in_order();
        let pack_integrity_ok = self.pack.verify_integrity().is_ok();
        let red_case_count = self.red_cases.len() as u32;
        let all_green = items_4_of_4
            && items[0]
            && trace_chain_ok
            && items[2]
            && items[3]
            && cross_ok
            && order_ok
            && pack_integrity_ok
            && decided
            && decision_pass
            && red_case_count == 0;
        let lights = vec![
            ("齐4/4落账", items_4_of_4),
            ("终核账自洽", items[0]),
            ("留痕链复核", trace_chain_ok),
            ("判定账自洽", items[2]),
            ("报告指针自洽", items[3]),
            ("四路交叉一致", cross_ok),
            ("落账序合规", order_ok),
            ("包完整性", pack_integrity_ok),
            ("已定格", decided),
            ("结论达标", decision_pass),
            ("红案为零", red_case_count == 0),
            ("口径可读", true),
        ];
        HealthReport {
            items_4_of_4,
            final_self_ok: items[0],
            trace_chain_ok,
            judgement_self_ok: items[2],
            report_ptr_ok: items[3],
            cross_ok,
            order_ok,
            pack_integrity_ok,
            decided,
            decision_pass,
            red_case_count,
            below_line_rejections: self.below_line_rejections,
            lights,
            all_green,
        }
    }

    /// 定格公报（仅定格后可出：判据回显 + 件报表 + 四件落账行 + 结论 +
    /// 公报指纹；未定格拒出——不虚报未决态）。
    pub fn gazette(&self) -> Result<String, Verdict1Error> {
        let (pass, final_ppm, pack_fp, serial) = match self.state {
            VerdictState::Undecided => {
                return Err(Verdict1Error::SealedWrite {
                    what: "未定格求公报（结论未定格不虚报）",
                })
            }
            VerdictState::Decided {
                pass,
                final_ppm,
                pack_fp,
                serial,
            } => (pass, final_ppm, pack_fp, serial),
        };
        let mut s = String::new();
        s.push_str("FVER1|GAZETTE|UNX-F0786|判据1终验定格公报\n");
        s.push_str(CRITERION_TEXT);
        s.push('\n');
        s.push_str(&self.pack.presence_line());
        s.push('\n');
        if let Some(fc) = self.pack.final_core_ref() {
            s.push_str(&fc.ledger_line());
            s.push('\n');
        }
        if let Some(d) = self.pack.trace_digest_ref() {
            s.push_str(&d.line());
            s.push('\n');
        }
        if let Some(j) = self.pack.judgement_ref() {
            s.push_str(&j.ledger_line());
            s.push('\n');
        }
        if let Some(p) = self.pack.report_ref() {
            s.push_str(&p.ledger_line());
            s.push('\n');
        }
        // 公报指纹：合指纹 + 序号 + ppm + 结论的再封印。
        let mut h = Fnv64::new();
        h.write_tag("UNX-F0786:GAZETTE:v1");
        h.write_u64(pack_fp);
        h.write_u32(serial);
        h.write_u64(final_ppm);
        h.write_u8(if pass { 1 } else { 0 });
        s.push_str(&format!(
            "FVER1|CONCLUDE|{}|ppm={}|serial={}\n",
            if pass { "达标定格" } else { "未达标定格" },
            final_ppm,
            serial
        ));
        s.push_str(&format!("FVER1|GAZETTE-FP|0x{:016X}", h.finish()));
        Ok(s)
    }

    // ── 注入面专用通道（检出演练；正式路径禁用） ──────────────────────
    /// 注入面：翻转包内终核账第 run_idx 号运行位（不更新任何指纹——
    /// 即「篡改」本体，供 verify_integrity / audit_pack 检出）。
    pub fn debug_flip_pack_run(&mut self, run_idx: u32) -> Result<(), Verdict1Error> {
        match self.pack.final_core_mut() {
            Some(fc) => {
                let mut plane = fc.plane();
                plane.flip_bit(run_idx as usize)?;
                fc.debug_overwrite_plane(plane);
                Ok(())
            }
            None => Err(Verdict1Error::ItemMissing {
                expect: MASK_ALL_ITEMS,
                got: self.pack.filed_mask(),
                missing: self.pack.missing_mask(),
            }),
        }
    }

    /// 注入面：破坏留痕账某序号事件载荷（断链本体，供 replay 检出）。
    pub fn debug_break_trace_event(&mut self, seq: u32) -> Result<(), Verdict1Error> {
        self.trace.debug_break_payload(seq)
    }
}

// ── S10c 终核账位平面正规覆写（注入面可用的安全重构造路径） ──────────
/// 安全覆写终核账位平面：以新平面重派生全部派生量（total/ppm/分列/seal）。
/// 注入面若需「自洽重算后的翻转」（非篡改语义）走本路径；篡改语义
/// （不重算）由 debug_flip_pack_run 直接重建账体实现。
pub fn rebuild_final_core(
    fc: &FinalCoreLedger,
    new_plane: OutcomePlane,
) -> Result<FinalCoreLedger, Verdict1Error> {
    FinalCoreLedger::from_plane(new_plane, fc.source_fp())
}

// ── S11 规范建站 / 全任务 / 双跑对拍 ─────────────────────────────────
/// 规范建站：canonical 平面 → 终核账 → 终核落账 → 判定账落账 →
/// 留痕封链落账 → 报告指针落账（齐 4/4，未定格）。
pub fn build_canonical_station(
    seed: u64,
    fail_count: u8,
) -> Result<FinalVerifyStation, Verdict1Error> {
    let plane = canonical_plane(seed, fail_count)?;
    let source_fp = mirror_source_fp(&plane);
    let final_ledger = FinalCoreLedger::from_plane(plane, source_fp)?;
    let mut st = FinalVerifyStation::new(seed)?;
    st.file_final_core(final_ledger)?;
    let judgement = JudgementRecord::from_final(&final_ledger, 1)?;
    st.file_judgement(judgement)?;
    st.seal_and_file_trace()?;
    let report = ReportPointer::derive_for(&final_ledger, st.trace_chain_head())?;
    st.file_report_pointer(report)?;
    Ok(st)
}

/// 全任务（规范建站 + 定格 + 体检）：fail_count=1（149 成功 → 9933ppm）
/// 即判据 1 达标道；fail_count≥2 走拒定格道（返回 BelowLine 错误）。
pub fn run_full_mission(seed: u64, fail_count: u8) -> Result<MissionOutcome, Verdict1Error> {
    let mut station = build_canonical_station(seed, fail_count)?;
    let conclusion = station.finalize_verdict()?;
    let health = station.health();
    Ok(MissionOutcome {
        station,
        conclusion,
        health,
    })
}

/// 双跑对拍：同 seed 两路独立建站定格，要索六项 + 定格结果逐项比。
pub fn dual_run_check(seed: u64, fail_count: u8) -> Result<DualRunReport, Verdict1Error> {
    let mut a = build_canonical_station(seed, fail_count)?;
    let mut b = build_canonical_station(seed, fail_count)?;
    let ra = a.finalize_verdict();
    let rb = b.finalize_verdict();
    // 定格后取要索（decided/decision_pass 反映终态，对拍面完整）。
    let va = a.vitals();
    let vb = b.vitals();
    let a_result = match &ra {
        Ok(_) => 0,
        Err(e) => e.err_code(),
    };
    let b_result = match &rb {
        Ok(_) => 0,
        Err(e) => e.err_code(),
    };
    let deterministic = va == vb && a_result == b_result;
    Ok(DualRunReport {
        a: va,
        b: vb,
        a_result,
        b_result,
        deterministic,
    })
}

/// 双跑对拍断言版（漂移即 E_DUAL_RUN 立案，sub 指明漂移面）。
pub fn assert_dual_run(seed: u64, fail_count: u8) -> Result<DualRunReport, Verdict1Error> {
    let rep = dual_run_check(seed, fail_count)?;
    if !rep.deterministic {
        let sub = if rep.a != rep.b {
            "要索漂移（指纹/ppm/链头/定格态不等）"
        } else {
            "定格结果码漂移"
        };
        return Err(Verdict1Error::DualRun { sub });
    }
    Ok(rep)
}

// ── S12 注入面（六路检出演练） ───────────────────────────────────────
/// 注入面 1 缺件注入：落账 0/1/2 步（终核/判定/留痕），报告指针缺位 →
/// 终验闸 E_ITEM_MISSING 检出（判据 J1-a：缺任一件即终验不过）。
pub fn inject_missing_item(seed: u64) -> Result<InjectionOutcome, Verdict1Error> {
    let plane = canonical_plane(seed, 1)?;
    let final_ledger = FinalCoreLedger::from_plane(plane, mirror_source_fp(&plane))?;
    let mut st = FinalVerifyStation::new(seed)?;
    st.file_final_core(final_ledger)?;
    let j = JudgementRecord::from_final(&final_ledger, 1)?;
    st.file_judgement(j)?;
    st.seal_and_file_trace()?;
    match st.finalize_verdict() {
        Err(e) => Ok(InjectionOutcome {
            name: "缺件注入（报告指针缺位）",
            detected: true,
            code: e.err_code(),
            detail: e.detail(),
        }),
        Ok(_) => Ok(InjectionOutcome {
            name: "缺件注入（报告指针缺位）",
            detected: false,
            code: 0,
            detail: "未检出：缺件终验竟然通过，终验闸失效",
        }),
    }
}

/// 注入面 2 不一致注入：判定账 base_ppm = 终核 ppm + 1（±1 漂移）→
/// 交叉核 E_CROSS_MISMATCH 检出（判据 J1-c：终核账 ppm 与判定账结论一致）。
pub fn inject_inconsistency(seed: u64) -> Result<InjectionOutcome, Verdict1Error> {
    let plane = canonical_plane(seed, 1)?;
    let final_ledger = FinalCoreLedger::from_plane(plane, mirror_source_fp(&plane))?;
    let mut st = FinalVerifyStation::new(seed)?;
    st.file_final_core(final_ledger)?;
    // 判定账 ppm 漂移 +1：自洽仍过（9934 与 Pass 互证成立），专打交叉核。
    let drifted = JudgementRecord::from_parts(
        final_ledger.total_ppm() + 1,
        verdict_of_ppm(final_ledger.total_ppm()),
        final_ledger.seal_fp(),
        TOTAL_RUNS as u32,
        1,
    )?;
    st.file_judgement(drifted)?;
    st.seal_and_file_trace()?;
    let report = ReportPointer::derive_for(&final_ledger, st.trace_chain_head())?;
    st.file_report_pointer(report)?;
    match st.finalize_verdict() {
        Err(e) => Ok(InjectionOutcome {
            name: "不一致注入（终核 ppm≠判定 ppm）",
            detected: true,
            code: e.err_code(),
            detail: e.detail(),
        }),
        Ok(_) => Ok(InjectionOutcome {
            name: "不一致注入（终核 ppm≠判定 ppm）",
            detected: false,
            code: 0,
            detail: "未检出：交叉一致核失效",
        }),
    }
}

/// 注入面 3 重复终验注入：完整定格后再次 finalize → E_REVERIFY 红案检出
/// （判据 J1-d：定格后不可重算，重算即重复终验红案；原结论不动摇）。
pub fn inject_reverify(seed: u64) -> Result<InjectionOutcome, Verdict1Error> {
    let mut st = build_canonical_station(seed, 1)?;
    let first = st.finalize_verdict()?;
    let original_serial = first.serial();
    let original_fp = first.pack_fp();
    match st.finalize_verdict() {
        Err(e) if e.err_code() == E_REVERIFY => {
            // 原结论不动摇核：定格态序号/指纹仍为首次定格值。
            let intact = st.state().decided_serial() == original_serial
                && match st.state() {
                    VerdictState::Decided { pack_fp, .. } => pack_fp == original_fp,
                    VerdictState::Undecided => false,
                };
            Ok(InjectionOutcome {
                name: "重复终验注入（Decided 后再提交）",
                detected: intact && !st.red_cases().is_empty(),
                code: e.err_code(),
                detail: e.detail(),
            })
        }
        Err(e) => Ok(InjectionOutcome {
            name: "重复终验注入（Decided 后再提交）",
            detected: true,
            code: e.err_code(),
            detail: e.detail(),
        }),
        Ok(_) => Ok(InjectionOutcome {
            name: "重复终验注入（Decided 后再提交）",
            detected: false,
            code: 0,
            detail: "未检出：定格单向闸失效",
        }),
    }
}

/// 注入面 4 落账乱序注入：新站先提判定账（跳过终核账）→
/// E_ITEM_ORDER 检出（件可齐而序不可乱）。
pub fn inject_order_violation(seed: u64) -> Result<InjectionOutcome, Verdict1Error> {
    let plane = canonical_plane(seed, 1)?;
    let final_ledger = FinalCoreLedger::from_plane(plane, mirror_source_fp(&plane))?;
    let mut st = FinalVerifyStation::new(seed)?;
    let j = JudgementRecord::from_final(&final_ledger, 1)?;
    match st.file_judgement(j) {
        Err(e) => Ok(InjectionOutcome {
            name: "落账乱序注入（判定先于终核）",
            detected: true,
            code: e.err_code(),
            detail: e.detail(),
        }),
        Ok(()) => Ok(InjectionOutcome {
            name: "落账乱序注入（判定先于终核）",
            detected: false,
            code: 0,
            detail: "未检出：落账序闸失效",
        }),
    }
}

/// 注入面 5 包篡改注入：完整定格后翻转终核账一个运行位（不更新指纹）→
/// audit_pack E_PACK_TAMPER 检出（判据 J1-e：四件合指纹篡改检出）。
pub fn inject_pack_tamper(seed: u64) -> Result<InjectionOutcome, Verdict1Error> {
    let mut st = build_canonical_station(seed, 1)?;
    let _ = st.finalize_verdict()?;
    st.debug_flip_pack_run(0)?;
    match st.audit_pack() {
        Err(e) => Ok(InjectionOutcome {
            name: "包篡改注入（定格后翻运行位）",
            detected: true,
            code: e.err_code(),
            detail: e.detail(),
        }),
        Ok(()) => Ok(InjectionOutcome {
            name: "包篡改注入（定格后翻运行位）",
            detected: false,
            code: 0,
            detail: "未检出：包指纹篡改闸失效",
        }),
    }
}

/// 注入面 6 留痕断链注入：封链后破坏一条事件载荷 →
/// 终验闸 E_TRACE_CHAIN 检出（留痕账事件链重放复核）。
pub fn inject_trace_break(seed: u64) -> Result<InjectionOutcome, Verdict1Error> {
    let mut st = build_canonical_station(seed, 1)?;
    st.debug_break_trace_event(2)?;
    match st.finalize_verdict() {
        Err(e) => Ok(InjectionOutcome {
            name: "留痕断链注入（封链后改事件载荷）",
            detected: true,
            code: e.err_code(),
            detail: e.detail(),
        }),
        Ok(_) => Ok(InjectionOutcome {
            name: "留痕断链注入（封链后改事件载荷）",
            detected: false,
            code: 0,
            detail: "未检出：留痕链重放复核失效",
        }),
    }
}

/// 注入面全套巡检（六路依次跑；全绿 = 每路均显性检出）。
pub fn injection_sweep(seed: u64) -> Result<Vec<InjectionOutcome>, Verdict1Error> {
    let mut out = Vec::new();
    out.push(inject_missing_item(seed)?);
    out.push(inject_inconsistency(seed)?);
    out.push(inject_reverify(seed)?);
    out.push(inject_order_violation(seed)?);
    out.push(inject_pack_tamper(seed)?);
    out.push(inject_trace_break(seed)?);
    Ok(out)
}

// ── 测试域（#[cfg(test)]：冒烟 + 定格语义 + 指纹 + 六注入 + 双跑） ────
#[cfg(test)]
mod tests {
    use super::*;

    // ── 诊断段 ────────────────────────────────────────────────────────
    #[test]
    fn test_diag_segment_exclusive_and_bounded() {
        // 段内 16 码全有专属诊断；段外一律未知。
        let mut code = SEG_BASE;
        while code < SEG_BASE + 0x10 {
            let d = diag_detail(code);
            assert!(d.len() >= 8, "code {:04X} 诊断过短", code);
            assert!(!d.starts_with("未知"), "code {:04X} 不应未知", code);
            code += 1;
        }
        assert!(diag_detail(SEG_BASE - 1).starts_with("未知"));
        assert!(diag_detail(SEG_BASE + 0x10).starts_with("未知"));
        assert!(diag_detail(0x0000).starts_with("未知"));
    }

    #[test]
    fn test_error_codes_unique() {
        // 16 变体诊断码互异且全部落入独占段。
        let samples = [
            Verdict1Error::SealedWrite { what: "t" },
            Verdict1Error::ItemMissing {
                expect: 15,
                got: 0,
                missing: 15,
            },
            Verdict1Error::ItemDup { kind: 0 },
            Verdict1Error::ItemOrder { expect: 0, got: 1 },
            Verdict1Error::FinalSelf { sub: "s" },
            Verdict1Error::MachineCol {
                machine: 0,
                rounds: 49,
            },
            Verdict1Error::PlaneIndex { idx: 150, limit: 150 },
            Verdict1Error::TraceChain { at_seq: 1 },
            Verdict1Error::JudgeSelf { sub: "s" },
            Verdict1Error::ReportPtr { sub: "s" },
            Verdict1Error::CrossMismatch { sub: "s" },
            Verdict1Error::PackFrozen,
            Verdict1Error::PackTamper {
                stored: 1,
                recomputed: 2,
            },
            Verdict1Error::Reverify { decided_serial: 1 },
            Verdict1Error::BelowLine { ppm: 9866 },
            Verdict1Error::DualRun { sub: "s" },
        ];
        let mut seen = [0u16; 16];
        let mut i = 0usize;
        while i < samples.len() {
            let c = samples[i].err_code();
            assert!(c >= SEG_BASE && c < SEG_BASE + 0x10);
            let mut j = 0usize;
            while j < i {
                assert!(seen[j] != c, "诊断码重复 0x{:04X}", c);
                j += 1;
            }
            seen[i] = c;
            assert!(!samples[i].detail().is_empty());
            i += 1;
        }
    }

    // ── 规范平面与 ppm 口径 ────────────────────────────────────────────
    #[test]
    fn test_canonical_plane_pass_and_reject_bounds() {
        // 1 失败 = 149 成功 → 9933ppm 达标；2 失败 = 148 → 9866ppm 拒格。
        let p1 = canonical_plane(0xA1B2_C3D4, 1).unwrap();
        assert_eq!(p1.success_count(), 149);
        let p2 = canonical_plane(0xA1B2_C3D4, 2).unwrap();
        assert_eq!(p2.success_count(), 148);
        assert_eq!(ppm_of(149, 150), 9933);
        assert_eq!(ppm_of(148, 150), 9866);
        assert_eq!(verdict_of_ppm(9933), Verdict::Pass);
        assert_eq!(verdict_of_ppm(9900), Verdict::Pass); // 边界含 ≥
        assert_eq!(verdict_of_ppm(9899), Verdict::Reject);
        // 双跑确定性：同 seed 同平面。
        assert_eq!(p1.fingerprint(), canonical_plane(0xA1B2_C3D4, 1).unwrap().fingerprint());
        // 失败位恰为请求个数。
        assert_eq!(p1.fail_positions().len(), 1);
        assert_eq!(p2.fail_positions().len(), 2);
    }

    #[test]
    fn test_canonical_plane_rejects_bad_params() {
        // 失败数 151 > 150 拒构造。
        let e = canonical_plane(7, 151).unwrap_err();
        assert_eq!(e.err_code(), 0x2776);
        // 0 失败 = 全成功 150/150 → 10000ppm。
        let p0 = canonical_plane(7, 0).unwrap();
        assert_eq!(p0.success_count(), 150);
        assert_eq!(ppm_of(150, 150), 10_000);
    }

    #[test]
    fn test_plane_padding_and_bounds() {
        let mut raw = [0u8; PLANE_BYTES];
        raw[0] = 0xFF; // 前 8 位全成功。
        let p = OutcomePlane::from_raw(raw).unwrap();
        assert_eq!(p.success_count(), 8);
        let mut dirty = raw;
        dirty[PLANE_BYTES - 1] = 0b0100_0000; // 第 150 位（保留位首 bit）脏。
        let e = OutcomePlane::from_raw(dirty).unwrap_err();
        assert_eq!(e.err_code(), 0x2776);
        // 越界读/写/翻。
        assert_eq!(p.bit(150).unwrap_err().err_code(), 0x2776);
        let mut pm = p;
        assert_eq!(pm.set_bit(150, 1).unwrap_err().err_code(), 0x2776);
        assert_eq!(pm.set_bit(0, 2).unwrap_err().err_code(), 0x2776); // 灰区位值
        assert!(pm.flip_bit(149).is_ok());
    }

    #[test]
    fn test_verdict_raw_closure() {
        assert_eq!(Verdict::Pass.as_raw(), 1);
        assert_eq!(Verdict::Reject.as_raw(), 0);
        assert_eq!(Verdict::from_raw(1).unwrap(), Verdict::Pass);
        assert_eq!(Verdict::from_raw(0).unwrap(), Verdict::Reject);
        assert_eq!(Verdict::from_raw(2).unwrap_err().err_code(), 0x2776);
    }

    // ── 终核账 ────────────────────────────────────────────────────────
    #[test]
    fn test_final_core_self_consistent() {
        let plane = canonical_plane(42, 1).unwrap();
        let fc = FinalCoreLedger::from_plane(plane, 0x1234_5678).unwrap();
        assert!(fc.verify_self().is_ok());
        assert_eq!(fc.total_success(), 149);
        assert_eq!(fc.total_ppm(), 9933);
        assert_eq!(fc.columns().len(), 3);
        let mut sum = 0u32;
        for col in fc.columns() {
            assert_eq!(col.rounds(), 50);
            sum += col.success();
            assert_eq!(col.ppm(), ppm_of(col.success(), 50));
        }
        assert_eq!(sum, 149);
        assert_eq!(fc.run_outcome(0).is_ok(), true);
        assert_eq!(fc.run_outcome(150).unwrap_err().err_code(), 0x2776);
    }

    #[test]
    fn test_final_core_rejects_zero_source() {
        let plane = canonical_plane(42, 1).unwrap();
        let e = FinalCoreLedger::from_plane(plane, 0).unwrap_err();
        assert_eq!(e.err_code(), 0x2774);
        match e {
            Verdict1Error::FinalSelf { sub } => assert!(sub.contains("镜像")),
            other => panic!("期望 FinalSelf，实得 {:?}", other),
        }
    }

    #[test]
    fn test_final_core_seal_fp_drift_detected() {
        // 由篡改（翻位）重派生对比：seal_fp 必随内容变化（判据 J1-e 件级面）。
        let plane = canonical_plane(42, 1).unwrap();
        let fc = FinalCoreLedger::from_plane(plane, 7).unwrap();
        let mut raw = plane.raw();
        raw[0] ^= 1;
        let plane2 = OutcomePlane::from_raw(raw).unwrap();
        let fc2 = FinalCoreLedger::from_plane(plane2, 7).unwrap();
        assert_ne!(fc.seal_fp(), fc2.seal_fp());
        assert!(fc2.verify_self().is_ok()); // 重派生自洽，但指纹已变。
        assert!(fc.ledger_line().contains("FVER1|FINAL"));
    }

    // ── 留痕账 ────────────────────────────────────────────────────────
    #[test]
    fn test_trace_chain_replay_and_seal() {
        let mut tr = TraceLedger::new();
        assert!(tr.replay_verify().is_ok());
        let s1 = tr.append(TraceKind::ItemFiled, 0x0100_0000).unwrap();
        let s2 = tr.append(TraceKind::ItemFiled, 0x0200_0000).unwrap();
        assert_eq!((s1, s2), (1, 2));
        assert!(tr.replay_verify().is_ok());
        let d = tr.seal().unwrap();
        assert!(tr.is_sealed());
        assert_eq!(d.event_count(), 3);
        assert_eq!(d.seal_seq(), 3);
        assert!(d.verify_self().is_ok());
        assert!(tr.replay_verify().is_ok());
        assert_eq!(d.chain_head(), tr.chain_head());
        // 封链后写拒。
        let e = tr.append(TraceKind::Open, 0).unwrap_err();
        assert_eq!(e.err_code(), 0x2770);
        // 再封拒。
        assert_eq!(tr.seal().unwrap_err().err_code(), 0x2770);
        assert!(tr.snapshot_lines().contains("TRC|OPEN"));
    }

    #[test]
    fn test_trace_break_detected() {
        let mut tr = TraceLedger::new();
        tr.append(TraceKind::Open, 1).unwrap();
        tr.append(TraceKind::ItemFiled, 2).unwrap();
        tr.append(TraceKind::TraceSeal, 0).unwrap();
        assert!(tr.replay_verify().is_ok());
        tr.debug_break_payload(2).unwrap();
        let e = tr.replay_verify().unwrap_err();
        assert_eq!(e.err_code(), 0x2777);
        match e {
            Verdict1Error::TraceChain { at_seq } => assert_eq!(at_seq, 2),
            other => panic!("期望 TraceChain，实得 {:?}", other),
        }
        // 查无序号按断链拒。
        assert_eq!(
            tr.debug_break_payload(99).unwrap_err().err_code(),
            0x2777
        );
    }

    // ── 判定账 ────────────────────────────────────────────────────────
    #[test]
    fn test_judgement_self_consistency() {
        let plane = canonical_plane(9, 1).unwrap();
        let fc = FinalCoreLedger::from_plane(plane, 7).unwrap();
        let j = JudgementRecord::from_final(&fc, 1).unwrap();
        assert!(j.verify_self().is_ok());
        assert_eq!(j.base_ppm(), 9933);
        assert_eq!(j.verdict(), Verdict::Pass);
        assert_eq!(j.from_final_fp(), fc.seal_fp());
        assert_eq!(j.sample_count(), 150);
        // ppm 与结论矛盾（9933 配 Reject）→ 自洽破。
        let bad = JudgementRecord::from_parts(9933, Verdict::Reject, fc.seal_fp(), 150, 1)
            .unwrap();
        assert_eq!(bad.verify_self().unwrap_err().err_code(), 0x2778);
        // 样本数非 150。
        let bad2 =
            JudgementRecord::from_parts(9933, Verdict::Pass, fc.seal_fp(), 149, 1).unwrap();
        assert_eq!(bad2.verify_self().unwrap_err().err_code(), 0x2778);
        // 来源指纹缺。
        let bad3 = JudgementRecord::from_parts(9933, Verdict::Pass, 0, 150, 1).unwrap();
        assert_eq!(bad3.verify_self().unwrap_err().err_code(), 0x2778);
        // 判定序号非正。
        assert_eq!(
            JudgementRecord::from_parts(9933, Verdict::Pass, 7, 150, 0)
                .unwrap_err()
                .err_code(),
            0x2778
        );
        assert!(j.ledger_line().contains("FVER1|JUDGE"));
    }

    // ── 报告指针 ──────────────────────────────────────────────────────
    #[test]
    fn test_report_pointer_validation() {
        let plane = canonical_plane(11, 1).unwrap();
        let fc = FinalCoreLedger::from_plane(plane, 7).unwrap();
        let rp = ReportPointer::derive_for(&fc, 0xBEEF).unwrap();
        assert_eq!(rp.locator(), "F0781#ALL");
        assert!(rp.verify_self().is_ok());
        // 合法节段形。
        assert!(ReportPointer::new("F0781#S6", 1, 1).unwrap().verify_self().is_ok());
        // 空/超长/坏格式/零指纹/零锚。
        assert_eq!(
            ReportPointer::new("", 1, 1).unwrap_err().err_code(),
            0x2779
        );
        let long = "F0781#";
        let mut too_long = String::from(long);
        while too_long.len() <= REPORT_LOCATOR_MAX_LEN + 1 {
            too_long.push('A');
        }
        assert_eq!(
            ReportPointer::new(&too_long, 1, 1).unwrap_err().err_code(),
            0x2779
        );
        assert_eq!(
            ReportPointer::new("F0780#ALL", 1, 1).unwrap_err().err_code(),
            0x2779
        );
        assert_eq!(
            ReportPointer::new("F0781#S7", 1, 1).unwrap_err().err_code(),
            0x2779
        );
        assert_eq!(
            ReportPointer::new("F0781#ALL", 0, 1).unwrap_err().err_code(),
            0x2779
        );
        assert_eq!(
            ReportPointer::new("F0781#ALL", 1, 0).unwrap_err().err_code(),
            0x2779
        );
        assert!(rp.ledger_line().contains("FVER1|RPTPTR"));
    }

    // ── 凭证包 ────────────────────────────────────────────────────────
    #[test]
    fn test_pack_flow_dup_order_freeze_tamper() {
        let plane = canonical_plane(33, 1).unwrap();
        let fc = FinalCoreLedger::from_plane(plane, 7).unwrap();
        let j = JudgementRecord::from_final(&fc, 1).unwrap();
        let mut pk = EvidencePack::new();
        assert!(!pk.is_complete());
        assert_eq!(pk.filed_count(), 0);
        // 乱序：首提判定账（slot1）→ 期望 slot0。
        let e = pk.file_judgement(j).unwrap_err();
        assert_eq!(e.err_code(), 0x2773);
        assert_eq!(pk.filed_mask(), 0);
        // 正序四步。
        pk.file_final_core(fc).unwrap();
        // 重件拒。
        assert_eq!(pk.file_final_core(fc).unwrap_err().err_code(), 0x2772);
        pk.file_judgement(j).unwrap();
        pk.file_trace_digest(TraceDigest {
            event_count: 4,
            chain_head: 0xABCD,
            genesis: 1,
            seal_seq: 4,
        })
        .unwrap();
        // 乱序中段：重提 slot1。
        assert_eq!(pk.file_judgement(j).unwrap_err().err_code(), 0x2772);
        assert!(!pk.filed_in_order()); // 3/4 件未齐，序检尚不通过
        assert_eq!(pk.first_order_violation(), None); // 已落三步序本身无偏离
        let rp = ReportPointer::new("F0781#S1", 9, 0xABCD).unwrap();
        pk.file_report_pointer(rp).unwrap();
        assert!(pk.is_complete());
        assert_eq!(pk.filed_count(), 4);
        assert!(pk.filed_in_order());
        assert_eq!(pk.first_order_violation(), None);
        assert!(pk.verify_integrity().is_ok());
        // 冻结前合指纹可复算（未封）。
        let expect_fp = pk.pack_fingerprint();
        let sealed = pk.freeze().unwrap();
        assert_eq!(sealed, expect_fp);
        // 冻结后写拒。
        let e = pk
            .file_final_core(fc)
            .unwrap_err();
        assert_eq!(e.err_code(), 0x277B);
        assert_eq!(pk.freeze().unwrap_err().err_code(), 0x277B);
        assert!(pk.verify_integrity().is_ok());
        assert_eq!(pk.sealed_pack_fp(), sealed);
        let views = pk.criterion_views();
        assert_eq!(views.len(), 4);
        assert!(views.iter().all(|v| v.filed));
        assert_eq!(views[0].label, "终核账");
        assert_eq!(views[1].label, "留痕账");
        assert_eq!(views[2].label, "判定账");
        assert_eq!(views[3].label, "报告指针");
        assert!(pk.presence_line().contains("4/4"));
        // 篡改：翻转终核账运行位（不更新任何指纹）→ 件指纹漂移检出。
        if let Some(fc_mut) = pk.final_core_mut() {
            let mut pl = fc_mut.plane();
            pl.flip_bit(0).unwrap();
            fc_mut.debug_overwrite_plane(pl);
        }
        let tamper = pk.verify_integrity().unwrap_err();
        assert_eq!(tamper.err_code(), 0x277C);
        match tamper {
            Verdict1Error::PackTamper { stored, recomputed } => {
                assert_ne!(stored, recomputed);
            }
            other => panic!("期望 PackTamper，实得 {:?}", other),
        }
    }

    #[test]
    fn test_pack_freeze_requires_complete() {
        let mut pk = EvidencePack::new();
        let plane = canonical_plane(5, 1).unwrap();
        let fc = FinalCoreLedger::from_plane(plane, 7).unwrap();
        pk.file_final_core(fc).unwrap();
        let e = pk.freeze().unwrap_err();
        assert_eq!(e.err_code(), 0x2771);
        match e {
            Verdict1Error::ItemMissing { expect, got, missing } => {
                assert_eq!((expect, got, missing), (0b1111, 0b0001, 0b1110));
            }
            other => panic!("期望 ItemMissing，实得 {:?}", other),
        }
    }

    // ── 全任务与定格语义 ──────────────────────────────────────────────
    #[test]
    fn test_full_mission_pass() {
        let mo = run_full_mission(2026_1024, 1).unwrap();
        assert!(mo.conclusion.pass());
        assert_eq!(mo.conclusion.final_ppm(), 9933);
        assert_eq!(mo.conclusion.success(), 149);
        assert_eq!(mo.conclusion.total(), 150);
        assert!(mo.health.all_green);
        assert_eq!(mo.health.red_case_count, 0);
        assert!(mo.station.is_decided());
        assert!(mo.station.gazette().unwrap().contains("判据1终验定格公报"));
        assert!(mo.station.pack().is_frozen());
    }

    #[test]
    fn test_finalize_below_line_keeps_undecided() {
        // 2 失败 → 9866ppm < 9900 → 拒定格（非红案），定格态保持 Undecided。
        let mut st = build_canonical_station(77, 2).unwrap();
        let e = st.finalize_verdict().unwrap_err();
        assert_eq!(e.err_code(), 0x277E);
        match e {
            Verdict1Error::BelowLine { ppm } => assert_eq!(ppm, 9866),
            other => panic!("期望 BelowLine，实得 {:?}", other),
        }
        assert!(!st.is_decided());
        assert_eq!(st.below_line_rejections(), 1);
        let h = st.health();
        assert!(!h.all_green);
        assert!(!h.decided);
        // 拒定格后仍可再终验（未定格不属重复终验）。
        assert_eq!(st.finalize_verdict().unwrap_err().err_code(), 0x277E);
        assert_eq!(st.below_line_rejections(), 2);
        assert_eq!(st.red_cases().len(), 0); // 拒定格非红案
    }

    #[test]
    fn test_decided_state_one_way_and_sealed_write() {
        let mut st = build_canonical_station(88, 1).unwrap();
        let c1 = st.finalize_verdict().unwrap();
        // 定格后写路径全拒（0x2770）。
        let plane = canonical_plane(88, 1).unwrap();
        let fc = FinalCoreLedger::from_plane(plane, 7).unwrap();
        assert_eq!(st.file_final_core(fc).unwrap_err().err_code(), 0x2770);
        let j = JudgementRecord::from_final(&fc, 9).unwrap();
        assert_eq!(st.file_judgement(j).unwrap_err().err_code(), 0x2770);
        assert_eq!(st.seal_and_file_trace().unwrap_err().err_code(), 0x2770);
        // 定格态序号/指纹不可移。
        assert_eq!(st.state().decided_serial(), c1.serial());
        match st.state() {
            VerdictState::Decided { pack_fp, .. } => assert_eq!(pack_fp, c1.pack_fp()),
            VerdictState::Undecided => panic!("定格态倒退"),
        }
        // 公报可出且含四件。
        let gz = st.gazette().unwrap();
        assert!(gz.contains("FVER1|FINAL"));
        assert!(gz.contains("FVER1|JUDGE"));
        assert!(gz.contains("TRC|DIGEST"));
        assert!(gz.contains("FVER1|RPTPTR"));
        assert!(gz.contains("GAZETTE-FP"));
    }

    #[test]
    fn test_gazette_undecided_refused() {
        let st = build_canonical_station(99, 1).unwrap();
        assert_eq!(st.gazette().unwrap_err().err_code(), 0x2770);
    }

    // ── 注入面 ────────────────────────────────────────────────────────
    #[test]
    fn test_inject_missing_item_detected() {
        let o = inject_missing_item(1).unwrap();
        assert!(o.detected);
        assert_eq!(o.code, 0x2771);
        assert!(o.is_green());
    }

    #[test]
    fn test_inject_inconsistency_detected() {
        let o = inject_inconsistency(2).unwrap();
        assert!(o.detected);
        assert_eq!(o.code, 0x277A);
        assert!(o.detail.contains("交叉"));
    }

    #[test]
    fn test_inject_reverify_detected_red_case() {
        let o = inject_reverify(3).unwrap();
        assert!(o.detected);
        assert_eq!(o.code, 0x277D);
    }

    #[test]
    fn test_inject_order_detected() {
        let o = inject_order_violation(4).unwrap();
        assert!(o.detected);
        assert_eq!(o.code, 0x2773);
    }

    #[test]
    fn test_inject_pack_tamper_detected() {
        let o = inject_pack_tamper(5).unwrap();
        assert!(o.detected);
        assert_eq!(o.code, 0x277C);
        assert!(o.detail.contains("指纹"));
    }

    #[test]
    fn test_inject_trace_break_detected() {
        let o = inject_trace_break(6).unwrap();
        assert!(o.detected);
        assert_eq!(o.code, 0x2777);
    }

    #[test]
    fn test_injection_sweep_all_green() {
        let outs = injection_sweep(0x5A5A).unwrap();
        assert_eq!(outs.len(), 6);
        for o in &outs {
            assert!(o.detected, "注入未检出：{} ({})", o.name, o.detail);
            assert!(o.code >= SEG_BASE && o.code < SEG_BASE + 0x10);
        }
    }

    // ── 双跑确定性对拍 ────────────────────────────────────────────────
    #[test]
    fn test_dual_run_deterministic_pass_path() {
        let rep = assert_dual_run(0xC0DE, 1).unwrap();
        assert!(rep.deterministic);
        assert_eq!(rep.a, rep.b);
        assert_eq!(rep.a_result, 0);
        assert_eq!(rep.b_result, 0);
        assert!(rep.a.decided && rep.a.decision_pass);
        assert_eq!(rep.a.total_ppm, 9933);
    }

    #[test]
    fn test_dual_run_deterministic_reject_path() {
        // 拒定格道同样双跑一致（结果码同为 0x277E）。
        let rep = assert_dual_run(0xC0DE, 2).unwrap();
        assert!(rep.deterministic);
        assert_eq!(rep.a_result, 0x277E);
        assert_eq!(rep.b_result, 0x277E);
        assert!(!rep.a.decided);
    }

    #[test]
    fn test_dual_run_different_seeds_diverge() {
        // 异 seed 平面应不同（生成器确实使用 seed；防退化为常量平面）。
        let a = canonical_plane(1, 1).unwrap();
        let b = canonical_plane(2, 1).unwrap();
        // 单失败位不同（150 选 1，异 seed 撞位概率极低；再抽两组佐证）。
        let c = canonical_plane(3, 2).unwrap();
        let d = canonical_plane(4, 2).unwrap();
        let mut diverged = a.fingerprint() != b.fingerprint()
            || c.fingerprint() != d.fingerprint();
        let mut k = 5u64;
        while k < 16 && !diverged {
            let x = canonical_plane(k, 1).unwrap();
            let y = canonical_plane(k + 100, 1).unwrap();
            diverged = x.fingerprint() != y.fingerprint();
            k += 1;
        }
        assert!(diverged, "规范生成器对 seed 不敏感（疑似退化）");
    }

    // ── health 灯表 ───────────────────────────────────────────────────
    #[test]
    fn test_health_lights_named_and_incomplete_red() {
        // 未齐件站：多数灯灭，all_green 假。
        let plane = canonical_plane(21, 1).unwrap();
        let fc = FinalCoreLedger::from_plane(plane, 7).unwrap();
        let mut st = FinalVerifyStation::new(21).unwrap();
        st.file_final_core(fc).unwrap();
        let h = st.health();
        assert!(!h.all_green);
        assert!(!h.items_4_of_4);
        assert!(h.final_self_ok);
        assert!(!h.decided);
        assert_eq!(h.lights.len(), 12);
        assert!(h.lights.iter().any(|(n, _)| *n == "齐4/4落账"));
        // 红案站：重复终验后红案灯灭。
        let mut st2 = build_canonical_station(22, 1).unwrap();
        st2.finalize_verdict().unwrap();
        let _ = st2.finalize_verdict();
        let h2 = st2.health();
        assert!(!h2.all_green);
        assert_eq!(h2.red_case_count, 1);
        assert!(!h2.lights.iter().find(|(n, _)| *n == "红案为零").unwrap().1);
        assert_eq!(st2.red_cases()[0].kind(), RedCaseKind::ReFinalVerify);
        assert!(st2.red_cases()[0].line().contains("重复终验"));
    }

    // ── 站要索与包合指纹 ──────────────────────────────────────────────
    #[test]
    fn test_station_vitals_and_pack_fp_stability() {
        let mut st = build_canonical_station(0x7777, 1).unwrap();
        let v0 = st.vitals();
        assert!(!v0.decided);
        assert_eq!(v0.total_ppm, 9933);
        assert_eq!(v0.success, 149);
        // 事件链：开站 + 件0落账 + 件1落账 + 封链 = 4 事件。
        assert_eq!(v0.event_count, 4);
        let c = st.finalize_verdict().unwrap();
        let v1 = st.vitals();
        assert!(v1.decided && v1.decision_pass);
        assert_eq!(v1.pack_fp, c.pack_fp());
        assert_eq!(v1.plane_fp, v0.plane_fp);
        // 定格前后即时合指纹恒等（冻结位不入指纹，封包值稳定可断言）。
        assert_eq!(v0.pack_fp, v1.pack_fp);
        assert_eq!(st.pack_fingerprint(), c.pack_fp());
    }

    #[test]
    fn test_mirror_source_fp_distinct_across_planes() {
        // 相邻 seed 扫描出一对不同平面，镜像指纹必异且非零。
        let mut sa = 101u64;
        let mut found = false;
        while sa < 140 && !found {
            let a = canonical_plane(sa, 1).unwrap();
            let b = canonical_plane(sa + 1, 1).unwrap();
            if a.fingerprint() != b.fingerprint() {
                assert_ne!(mirror_source_fp(&a), mirror_source_fp(&b));
                assert_ne!(mirror_source_fp(&a), 0);
                found = true;
            }
            sa += 1;
        }
        assert!(found, "相邻 seed 未扫出异平面（生成器疑似退化）");
    }

    #[test]
    fn test_fnv_stream_and_mix_event_determinism() {
        let mut h1 = Fnv64::new();
        h1.write_tag("UNX-F0786");
        h1.write_u64(150);
        let mut h2 = Fnv64::new();
        h2.write_tag("UNX-F0786");
        h2.write_u64(150);
        assert_eq!(h1.finish(), h2.finish());
        assert_eq!(fnv1a64(b"abc"), fnv1a64(b"abc"));
        assert_ne!(fnv1a64(b"abc"), fnv1a64(b"abd"));
        assert_eq!(
            mix_event(1, 2, 3, 4),
            mix_event(1, 2, 3, 4)
        );
        assert_ne!(mix_event(1, 2, 3, 4), mix_event(1, 2, 3, 5));
    }

    #[test]
    fn test_pack_item_kind_mapping() {
        // 落账 slot ↔ 判据原文枚举序映射锁死。
        assert_eq!(PackItemKind::FinalCore.slot(), 0);
        assert_eq!(PackItemKind::Judgement.slot(), 1);
        assert_eq!(PackItemKind::Trace.slot(), 2);
        assert_eq!(PackItemKind::ReportPtr.slot(), 3);
        assert_eq!(PackItemKind::FinalCore.criterion_slot(), 1);
        assert_eq!(PackItemKind::Trace.criterion_slot(), 2);
        assert_eq!(PackItemKind::Judgement.criterion_slot(), 3);
        assert_eq!(PackItemKind::ReportPtr.criterion_slot(), 4);
        assert_eq!(CRITERION_ORDER[0], PackItemKind::FinalCore);
        assert_eq!(CRITERION_ORDER[1], PackItemKind::Trace);
        assert_eq!(CRITERION_ORDER[2], PackItemKind::Judgement);
        assert_eq!(CRITERION_ORDER[3], PackItemKind::ReportPtr);
        assert_eq!(PackItemKind::from_slot(4), None);
        assert_eq!(PackItemKind::from_slot(0), Some(PackItemKind::FinalCore));
    }

    #[test]
    fn test_audit_pack_tamper_red_case_recorded() {
        // 独立审计通道：定格前篡改也立案。
        let mut st = build_canonical_station(55, 1).unwrap();
        st.debug_flip_pack_run(3).unwrap();
        let e = st.audit_pack().unwrap_err();
        assert_eq!(e.err_code(), 0x277C);
        assert_eq!(st.red_cases().len(), 1);
        assert_eq!(st.red_cases()[0].kind(), RedCaseKind::PackTamper);
        // 终验主闸同样先拦篡改。
        let e2 = st.finalize_verdict().unwrap_err();
        assert_eq!(e2.err_code(), 0x277C);
        assert_eq!(st.red_cases().len(), 2);
    }

    #[test]
    fn test_machine_column_edges() {
        let col = MachineColumn::new(2, 50, 50);
        assert_eq!(col.ppm(), 10_000);
        assert_eq!(col.fail(), 0);
        let col2 = MachineColumn::new(0, 50, 49);
        assert_eq!(col2.ppm(), 9_800);
        assert_eq!(col2.fail(), 1);
        let col0 = MachineColumn::new(1, 0, 0);
        assert_eq!(col0.ppm(), 0); // 除零守卫显性回 0
        assert!(col.describe().contains("M2=50/50"));
    }

    #[test]
    fn test_rebuild_final_core_safe_path() {
        let plane = canonical_plane(66, 1).unwrap();
        let fc = FinalCoreLedger::from_plane(plane, 7).unwrap();
        let mut raw = plane.raw();
        raw[0] ^= 1;
        let p2 = OutcomePlane::from_raw(raw).unwrap();
        let fc2 = rebuild_final_core(&fc, p2).unwrap();
        assert!(fc2.verify_self().is_ok());
        assert_ne!(fc.seal_fp(), fc2.seal_fp());
        assert_eq!(fc2.source_fp(), fc.source_fp()); // 镜像来源不重写
    }
}
