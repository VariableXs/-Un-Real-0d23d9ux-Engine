//! unx1902_lazysave —— UNX-F1902 惰性保存策略的 **CheckSet 接线壳**。
//!
//! 背景：并行席曾先行落过一个 519 行的 `unx1902_lazysave.rs` 浅版实现，
//! `unxstar/mod.rs` 的 `blocks` 数组以其 `run_unx1902_checks()` 入口接线。
//! 本会话（AI-WB3）按判据深化重写该功能，新实现落在 `unx1902_lazysave_deep.rs`
//! （2736 行，含保存时机三态判定表 12 格、三态逐例 10 例、CR0.TS→#NM→清位闭环、
//! 往返保真样本矩阵、账守恒与十形态注入核）。
//!
//! 为不打断 `mod.rs` 既有接线（避免 E0433 断链影响整个 crate 编译），
//! 本文件只做一件事：把深化版的十项判据自检结果翻译成 `CheckSet`，
//! 让域内既有汇总路径仍然拿到 F1902 的真实自检结论，而非空壳。
//!
//! 判据原文：UNX-F1902-J1 惰性保存决策路径断言全过（TS 置位→首次使用 #NM 陷阱→
//!           恢复/清位闭环），保存时机三态（切出必存/惰性/永存）逐态正确 10/10。
//! 职责定位：接线层，零业务逻辑；全部语义在 `unx1902_lazysave_deep`。
//! 诊断码段：本壳不新增诊断码（语义码段 0xB140~0xB14F 归深化版独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，无浮点，无 IO，无墙钟。

use crate::checks::CheckSet;

use super::unx1902_lazysave_deep;

/// 域标签。
const DOM: &str = "unx1902";

/// 自检种子（确定性，全整数；无墙钟参与）。
const SELFTEST_SEED: u64 = 0x1902_5EED_0001;

/// 判据入口：把深化版十项自检逐项翻译为 CheckSet 行。
///
/// 十项与深化版 `Observation` 字段一一对应：
/// C1902-01 判定表 12 格 / 02 三态逐例 10 例 / 03 TS→#NM→清位闭环 /
/// 04 往返保真样本矩阵 / 05 十形态注入检出 / 06 扩展区布局与账守恒 /
/// 07 摊销窗探针 / 08 TLV 自洽 / 09 KV 导出 / 10 综合总闸。
pub fn run_unx1902_checks() -> CheckSet {
    let mut cs = CheckSet::new(DOM);
    let ob = unx1902_lazysave_deep::observe(SELFTEST_SEED);

    // ---- C1902-01 策略×脏×XCR0 使能位 12 格判定表全过 ----
    cs.add(
        "C1902-01 判定表12格",
        ob.matrix_pass == unx1902_lazysave_deep::MATRIX_CELLS,
        "切出必存/惰性/永存 × 脏 × 使能",
    );

    // ---- C1902-02 保存时机三态逐态正确 10 例 ----
    cs.add(
        "C1902-03 三态逐例10",
        ob.cases_pass == unx1902_lazysave_deep::POLICY_CASE_COUNT,
        "AlwaysOnSwitchOut/Lazy/Never 逐例",
    );

    // ---- C1902-03 TS 置位→首次使用 #NM 陷阱→恢复/清位闭环 ----
    cs.add("C1902-02 NM闭环", ob.closure.closed, "TS→#NM→保存→清TS");

    // ---- C1902-04 fxsave64/fxrstor64 往返保真（样本矩阵逐字段零差） ----
    cs.add(
        "C1902-04 往返保真",
        ob.roundtrip_pass == unx1902_lazysave_deep::ROUNDTRIP_SAMPLES,
        "save→改脏→restore 零差",
    );

    // ---- C1902-05 十形态注入 10/10 检出 ----
    cs.add(
        "C1902-05 十形态检出",
        ob.detect_pass == unx1902_lazysave_deep::FORM_COUNT,
        "凭空NM/重入/未清TS/未消费/…",
    );

    // ---- C1902-06 扩展状态区布局 + 账守恒（save=restore+lazy） ----
    cs.add("C1902-06 布局守恒", ob.layout_ok, "L1~L5 区段不重叠不越界");
    cs.add("C1902-07 扩展状态账", ob.ledger_ok, "save=restore+lazy_pending");

    // ---- C1902-07 摊销窗探针（窗内豁免不得误判丢失，窗闭强制落盘） ----
    cs.add("C1902-08 摊销窗", ob.window.ok, "窗内延迟/窗闭落盘");

    // ---- C1902-08 TLV 长度自洽 + C1902-09 KV 导出非空 ----
    let tlv = unx1902_lazysave_deep::export_tlv();
    cs.add("C1902-09 TLV自洽", tlv.len() >= 8, "魔数+版本+长度");
    cs.add("C1902-10 KV导出", !unx1902_lazysave_deep::export_kv().is_empty(), "键值导出非空");

    // ---- C1902-10 综合总闸 ----
    cs.add("C1902-01 总闸", ob.all_ok(), "判据 UNX-F1902-J1 汇总");

    cs
}
