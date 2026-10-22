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
    //
    // 修正记录（标签互换，真缺陷）：原标签写作 `"C1902-03 三态逐例10"`，
    // 而紧邻的注释写的是 `C1902-02`，被测实现 `self_check()` 里的 ID 也是
    // `C1902-02 cases10`——**标签、注释、实现三处里标签是那个异类**，
    // 于是 02 与 03 整段互换（下面那条 NM 闭环同样被标成 02）。
    //
    // 为什么这不是「文字瑕疵」：`CheckSet::add` **不去重**，只按写入序落
    // 定容槽；`red_items_text()` 直接拿 `name` 点名。故一旦某条判据转红，
    // 报出来的名字会指向**另一条判据**，而点进去看又是对的——这种
    // 「名字与内容对不上」的账比没有名字更坏。
    cs.add(
        "C1902-02 三态逐例10",
        ob.cases_pass == unx1902_lazysave_deep::POLICY_CASE_COUNT,
        "AlwaysOnSwitchOut/Lazy/Never 逐例",
    );

    // ---- C1902-03 TS 置位→首次使用 #NM 陷阱→恢复/清位闭环 ----
    cs.add("C1902-03 NM闭环", ob.closure.closed, "TS→#NM→保存→清TS");

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

    // ---- C1902-06 扩展状态区布局不变量（L1~L5 不重叠不越界） ----
    // 与C1902-07 分列两条：本域范式是**一项一性质**，布局（几何不变量）
    // 与账守恒（代数等式）是两个性质，捆一条则任一红都看不出红在哪半边。
    cs.add("C1902-06 布局不变量", ob.layout_ok, "L1~L5 区段不重叠不越界");

    // ---- C1902-06b 扩展状态账守恒（save = restore + lazy_pending） ----
    //
    // 编号用 `06b` 而非 `07`：被测实现的 `self_check()` 里 07 号是**窗协同**，
    // 本壳也把窗放在 07（见下）。若这里占 07，则07/08/09/10 全体顺移一位，
    // 壳与实现的 ID 就再也对不上——而这套 ID 是外部按`C1902-NN`检索的
    // **唯一索引**，错位等于让按号排查的人查错条目。
    cs.add("C1902-06b 账守恒", ob.ledger_ok, "save=restore+lazy_pending");

    // ---- C1902-07 摊销窗协同（窗内豁免不误判丢失 / 非深度区拒开） ----
    //
    // 修正记录（一条捆五个字段 → 无法定位）：原判据把窗探针的**五个字段**
    // 用 `&&` 捆成一条，任一不成立就整条红，而 panic 只报「窗内延迟/窗闭落盘」
    // 这一句——看不到到底是 `opened` 没开成、`shallow_refused` 没拒成，
    // 还是 `forced_flush == 0`。拆成五条具名判据（子条件与原合取逐项同构）。
    //
    // 拆分同时暴露出第五个条件此前**必然红**：`forced_flush >= 1`。
    // 根因是实现缺记账——`AmortWindow::note_flush()` 在全仓**零调用点**
    // （`close_window` 的注释写「由 F1903 接管」，而 F1903 根本没接），
    // `note_deferred()` 同样零调用点 ⇒ `forced_flush` 恒0、`deferred_in_window`
    // 恒 0。即「窗内延迟」「窗闭落盘」两个接口位**从来没人写**。
    // 已在 `unx1902_lazysave_deep::close_window` / `switch_out` 补上记账，
    // 故这条判据现在可以恢复，并且是真断言而非恒假。
    cs.add("C1902-07a 非深度区拒开窗", ob.window.shallow_refused, "Sse 区开窗被拒（能力位不足不假装能开）");
    cs.add("C1902-07b 深度区开窗成", ob.window.opened, "Avx512 区开窗成功");
    cs.add("C1902-07c 窗内挂起被豁免", ob.window.exempt, "窗内 lazy 槽标记 in_window（不误判丢失）");
    // 窗闭后**不该**还有 in_window 的挂起——这一条是 07c 的收口，
    // 少了它「窗内豁免」就可能变成「永久豁免」。
    cs.add("C1902-07d 窗闭豁免即收", !ob.window.exempt_after_close, "窗闭后 in_window 挂起被清（豁免不永续）");
    cs.add(
        "C1902-07e 窗闭强制落盘",
        ob.window.forced_flush >= 1,
        "窗内挂起在窗闭时被强制落盘（窗内省的账窗闭后有人补，不悬空）",
    );

    // ---- C1902-08 TLV 长度自洽 + C1902-09 KV 导出含键 ----
    // 修正记录：原调用漏传样本参数（E0061 实锤——`export_tlv` 收
    // `(&Observation, &ThreadSlot)`、`export_kv` 收 `&Observation`）。
    // 这里以 canonical 线程槽（tid=1、xcr0=0x7 对应 SSE+AVX 全开）构造
    // 导出样本，使 TLV/KV 走真实编码路径而非空样本。
    let sample = unx1902_lazysave_deep::ThreadSlot::new(1, 0x7);
    let tlv = unx1902_lazysave_deep::export_tlv(&ob, &sample);
    cs.add(
        "C1902-08 TLV自洽",
        unx1902_lazysave_deep::tlv_len_ok(&tlv),
        "魔数+版本+长度回读",
    );
    let kv = unx1902_lazysave_deep::export_kv(&ob);
    cs.add(
        "C1902-09 KV导出",
        kv.contains("UNX-F1902") && kv.contains("assert.detect"),
        "键值导出含判据键",
    );

    // ---- C1902-10 综合总闸 ----
    //
    // 修正记录（同上，ID 撞车）：原标签 `"C1902-01 总闸"`与本函数**首条**
    // `"C1902-01 判定表12格"` 同号，而被测实现的对应 ID 是 `C1902-10 gate`。
    // 撞号的后果同上：两条判据同名前缀，红项点名无法区分是哪一条。
    //
    // 另注：总闸走 `all_ok()`，它覆盖 01~06b+07 的核心六项，但**不含**
    // 08/09 两项导出断言（`all_ok` 的实现在 deep 版里不含 TLV/KV）。
    // 故总闸绿 ≠ 导出路径绿，两者是并列关系而非包含关系。
    cs.add("C1902-10 总闸", ob.all_ok(), "判据 UNX-F1902-J1 汇总");

    cs
}
