
// ---------------------------------------------------------------------------
// F004 · 深化批次四：拒绝记录缓存文件钉值 + 诚实卡片同体系锚
//
// 主册依据（G-A-04【数据与存储】）：「拒绝记录存 `cache/wow64-refusals.json`
// （文件哈希 → 已提示标记），上限 1000 条」；【交互设计】「拒绝卡片样式对齐
// F035 兼容性向导（同一卡片体系）」。既有面：VXR4 序列化/RefusalLedger/
// HonestCard 不重复——本段钉文件名并登记格式口径冲突（见下）。
//
// **口径冲突登记（一处一事实）**：主册写 .json（开放格式惯例）；批次一落的是
// VXR4 自定轻量二进制（与 F020 dump 同族惯例）。以主册文件名为准、格式迁移
// 随闸门（JSON 面复用 F009 export_json 设施）——两处不各自为政。
// ---------------------------------------------------------------------------

/// 拒绝记录缓存路径（主册【数据与存储】原文钉值）。
pub const REFUSAL_CACHE_PATH: &str = "cache\\wow64-refusals.json";
/// 拒绝记录上限（主册同句钉值——与既有 REFUSAL_CAP 同值对账锚）。
pub const REFUSAL_LEDGER_CAP: usize = 1000;

/// F004 深化批次四自检。
pub fn run_wow64_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F004-wow64-deep3");
    // 1) 缓存路径与上限钉值（主册原文；上限与既有 REFUSAL_CAP 同值不漂移）。
    cs.add(
        "refusal_cache_path_and_cap_pinned",
        REFUSAL_CACHE_PATH == "cache\\wow64-refusals.json"
            && REFUSAL_LEDGER_CAP == 1000
            && REFUSAL_CAP == REFUSAL_LEDGER_CAP,
        "",
    );
    // 2) 诚实卡片三要素字段在位（F035 同一卡片体系的结构锚——既有字段面）。
    let card = MachineVerdict::ThirtyTwo.honest_card();
    cs.add(
        "honest_card_f035_same_system",
        matches!(card, Some(c) if !c.what.is_empty() && !c.why.is_empty() && !c.next.is_empty()),
        "",
    );
    cs
}
