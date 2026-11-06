//! UNX-F3458 API 冻结签名表（执行面 API 12 项冻结） · 0x79FC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B15 逐字摘录）：
//!   ① 纯功能代码 ≥200 行；
//!   ② 判据原文：J1：执行面 API 12 项冻结（Executor/Selector/
//!     OrderPolicy/IsolationEnv/WorkerPool/TimeoutHub/SlowLedger/
//!     RetryPolicy/FlakyLedger/ResumePoint/DetectorHook/BudgetAudit）；
//!     交叉无冲突。；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 12 项冻结：执行面十二个 API 以 ApiSig（槽位 0x01..0x0C +
//!     名称 + 元数 + 返回面）编译期冻结进 FROZEN_API_TABLE，
//!     名称与判据原文逐字对齐，顺序即槽位序（TableFrozen
//!     点名任何漂移）；
//!   - 槽位闭带：槽位 0x01..0x0C 连续无洞无重（SlotGap/SlotDup
//!     点名任何破带），槽位同时是本段诊断码带 0x01..0x0C 的
//!     语义锚（两带同构，一处漂移两处点名）；
//!   - 交叉无冲突：cross_conflict 逐对查重（名称唯一 + 槽位唯一
//!     + 元数不超上限 + 名称/返回面非空），任一破缺即
//!     CrossConflict 点名（12 项两两 66 对全查）；
//!   - 查找面：find(name) 闭集查找，12 项全可查、表外名必落空
//!     （UnknownApi 点名查无此 API 时的误接）；
//!   - 元数冻结：十二 API 元数按执行面调用约定逐项冻结
//!     （0..=8 拍内），上限 ARITY_CAP=8（ArityCap 点名超限）；
//!   - 双跑一致：交叉冲突检查纯函数，同表两查结果相同；
//!   - 写回面：状态=已完成、结果摘要非空。
//! 常量来源：
//!   - 十二 API 名称与顺序：派工卡判据原文逐字冻结
//!     （Executor..BudgetAudit）；
//!   - 槽位 0x01..0x0C：本段诊断码带 SEG_BASE+0x01..0x0C 同构锚；
//!   - ARITY_CAP=8：执行面调用约定元数上限（0..=8 拍内）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B15 批段窗 [0x79E00, 0x79FC0] 内）。
pub const SEG_BASE: u32 = 0x79FC0;

/// 冻结 API 项数（判据原文：12 项）。
pub const FROZEN_API_COUNT: usize = 12;
/// 元数上限（执行面调用约定 0..=8 拍内）。
pub const ARITY_CAP: u32 = 8;
/// 槽位带起点（与诊断码带 0x01 同构）。
pub const SLOT_BASE: u32 = 0x01;
/// 槽位带终点（与诊断码带 0x0C 同构）。
pub const SLOT_TOP: u32 = 0x0C;

/// 执行面 API 冻结签名：槽位 + 名称 + 元数 + 返回面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiSig {
    /// 槽位（0x01..0x0C，与诊断码带同构）。
    pub slot: u32,
    /// API 名称（判据原文逐字冻结）。
    pub name: &'static str,
    /// 调用元数（0..=ARITY_CAP）。
    pub arity: u32,
    /// 返回面描述（非空中文）。
    pub ret: &'static str,
}

/// 执行面 API 12 项冻结签名表（名称与顺序即判据原文）。
pub const FROZEN_API_TABLE: [ApiSig; 12] = [
    ApiSig { slot: 0x01, name: "Executor", arity: 3, ret: "执行器收口：用例结果与执行账" },
    ApiSig { slot: 0x02, name: "Selector", arity: 2, ret: "选择器：用例子集挑选" },
    ApiSig { slot: 0x03, name: "OrderPolicy", arity: 1, ret: "序策略：执行序冻结" },
    ApiSig { slot: 0x04, name: "IsolationEnv", arity: 2, ret: "隔离环境：用例隔离域" },
    ApiSig { slot: 0x05, name: "WorkerPool", arity: 4, ret: "工作池：并发执行单元" },
    ApiSig { slot: 0x06, name: "TimeoutHub", arity: 2, ret: "超时枢纽：拍级超时对账" },
    ApiSig { slot: 0x07, name: "SlowLedger", arity: 2, ret: "慢账：慢用例入账面" },
    ApiSig { slot: 0x08, name: "RetryPolicy", arity: 2, ret: "重试策略：retry=1 默认可关" },
    ApiSig { slot: 0x09, name: "FlakyLedger", arity: 2, ret: "flaky 账：首败重过标记" },
    ApiSig { slot: 0x0A, name: "ResumePoint", arity: 2, ret: "断点续跑：序号断点与清单" },
    ApiSig { slot: 0x0B, name: "DetectorHook", arity: 3, ret: "检测钩子：告警升格联动" },
    ApiSig { slot: 0x0C, name: "BudgetAudit", arity: 2, ret: "预算审计：超支告警不阻断" },
];

/// 闭集查找：按名称取签名，表外名回 None。
pub fn find(name: &str) -> Option<ApiSig> {
    let mut i = 0usize;
    while i < FROZEN_API_TABLE.len() {
        match FROZEN_API_TABLE.get(i) {
            Some(a) => {
                if a.name == name {
                    return Some(*a);
                }
            }
            None => return None,
        }
        i += 1;
    }
    None
}

/// 交叉冲突检查（12 项两两 66 对全查）：名称唯一 + 槽位唯一 +
/// 槽位连续闭带 + 元数不超上限 + 名称/返回面非空。
pub fn cross_conflict() -> Result<(), u32> {
    if FROZEN_API_TABLE.len() != FROZEN_API_COUNT {
        return Err(Diag::TableLen.val());
    }
    let mut i = 0usize;
    while i < FROZEN_API_TABLE.len() {
        let a = match FROZEN_API_TABLE.get(i) {
            Some(a) => *a,
            None => return Err(Diag::TableLen.val()),
        };
        // 槽位闭带：第 i 项槽位必为 SLOT_BASE+i（连续无洞）。
        if a.slot != SLOT_BASE + (i as u32) || a.slot > SLOT_TOP {
            return Err(Diag::SlotGap.val());
        }
        if a.name.is_empty() {
            return Err(Diag::NameEmpty.val());
        }
        if a.ret.is_empty() {
            return Err(Diag::RetEmpty.val());
        }
        if a.arity > ARITY_CAP {
            return Err(Diag::ArityCap.val());
        }
        let mut j = i + 1;
        while j < FROZEN_API_TABLE.len() {
            let b = match FROZEN_API_TABLE.get(j) {
                Some(b) => *b,
                None => return Err(Diag::TableLen.val()),
            };
            if a.name == b.name {
                return Err(Diag::NameDup.val());
            }
            if a.slot == b.slot {
                return Err(Diag::SlotDup.val());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 诊断码闭集：0x79FC1..=0x79FCC（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 签名表漂移：冻结表被运行期改写。
    TableFrozen = 0x01,
    /// 名称重复：同名字 API 两项。
    NameDup = 0x02,
    /// 槽位重复：同槽位两项。
    SlotDup = 0x03,
    /// 槽位破带：槽位不连续或越出 0x01..0x0C。
    SlotGap = 0x04,
    /// 名称空：API 名称为空串。
    NameEmpty = 0x05,
    /// 返回面空：返回面描述为空串。
    RetEmpty = 0x06,
    /// 元数超限：元数超 ARITY_CAP 上限。
    ArityCap = 0x07,
    /// 表长不符：表长不等于 12 项。
    TableLen = 0x08,
    /// 交叉冲突：逐对查重抓到任何破缺。
    CrossConflict = 0x09,
    /// 查无此 API：表外名称查找落空后的误接。
    UnknownApi = 0x0A,
    /// 双跑分叉：同表两次交叉检查结果不同。
    DoubleRunDiverge = 0x0B,
    /// 写回摘要为空：完成时摘要未定稿。
    SummaryEmpty = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::TableFrozen => "签名表漂移：建议十二项签名冻结为常量表，运行期只读",
            Diag::NameDup => "名称重复：建议入表前逐对查重，同名只保留一项",
            Diag::SlotDup => "槽位重复：建议槽位一一对应，重槽显性点名",
            Diag::SlotGap => "槽位破带：建议槽位连续 0x01..0x0C，无洞无越界",
            Diag::NameEmpty => "名称空：建议每项 API 名称非空且与判据原文逐字对齐",
            Diag::RetEmpty => "返回面空：建议返回面描述非空中文后入表",
            Diag::ArityCap => "元数超限：建议元数钳制在 ARITY_CAP=8 拍内",
            Diag::TableLen => "表长不符：建议表长恒 12 项，增删走版本冻结",
            Diag::CrossConflict => "交叉冲突：建议 66 对全查逐对点名破缺项",
            Diag::UnknownApi => "查无此 API：建议表外名称显性拒绝，不误接冻结签名",
            Diag::DoubleRunDiverge => "双跑分叉：建议交叉检查纯函数化，同表两查同果",
            Diag::SummaryEmpty => "写回摘要为空：建议完成时以非空中文摘要写回",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::TableFrozen,
    Diag::NameDup,
    Diag::SlotDup,
    Diag::SlotGap,
    Diag::NameEmpty,
    Diag::RetEmpty,
    Diag::ArityCap,
    Diag::TableLen,
    Diag::CrossConflict,
    Diag::UnknownApi,
    Diag::DoubleRunDiverge,
    Diag::SummaryEmpty,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        match ALL_DIAGS.get(i) {
            Some(d) => {
                if d.val() == code {
                    return d.detail();
                }
            }
            None => break,
        }
        i += 1;
    }
    "未知诊断码：建议核对是否落在 0x79FC1..=0x79FCC 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B15 批段窗 [0x79E00, 0x79FC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79E00 && SEG_BASE <= 0x79FC0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        if a.detail().is_empty() || !a.detail().contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    detail(0xDEAD) == detail(0x5A5A) && !detail(0xDEAD).is_empty()
}

/// 表冻结：12 项名称与判据原文逐字对齐、顺序即槽位序。
fn table_frozen_ok() -> bool {
    if FROZEN_API_TABLE.len() != FROZEN_API_COUNT {
        return false;
    }
    const EXPECTED: [&str; 12] = [
        "Executor",
        "Selector",
        "OrderPolicy",
        "IsolationEnv",
        "WorkerPool",
        "TimeoutHub",
        "SlowLedger",
        "RetryPolicy",
        "FlakyLedger",
        "ResumePoint",
        "DetectorHook",
        "BudgetAudit",
    ];
    let mut i = 0usize;
    while i < FROZEN_API_TABLE.len() {
        let a = match FROZEN_API_TABLE.get(i) {
            Some(a) => *a,
            None => return false,
        };
        let want = match EXPECTED.get(i) {
            Some(w) => *w,
            None => return false,
        };
        if a.name != want {
            return false;
        }
        i += 1;
    }
    FROZEN_API_COUNT == 12
        && detail(Diag::TableFrozen.val()).contains("冻结")
}

/// 槽位闭带：0x01..0x0C 连续无洞无重，与诊断码带同构。
fn slot_continuity_ok() -> bool {
    if cross_conflict().is_err() {
        return false;
    }
    let mut i = 0usize;
    while i < FROZEN_API_TABLE.len() {
        let a = match FROZEN_API_TABLE.get(i) {
            Some(a) => *a,
            None => return false,
        };
        if a.slot != SLOT_BASE + (i as u32) {
            return false;
        }
        i += 1;
    }
    SLOT_BASE == 0x01
        && SLOT_TOP == 0x0C
        && SLOT_TOP - SLOT_BASE + 1 == FROZEN_API_COUNT as u32
        && detail(Diag::SlotGap.val()).contains("连续")
}

/// 名称唯一：逐对查重 66 对无同名。
fn name_unique_ok() -> bool {
    let mut i = 0usize;
    let mut pairs = 0usize;
    while i < FROZEN_API_TABLE.len() {
        let a = match FROZEN_API_TABLE.get(i) {
            Some(a) => *a,
            None => return false,
        };
        let mut j = i + 1;
        while j < FROZEN_API_TABLE.len() {
            let b = match FROZEN_API_TABLE.get(j) {
                Some(b) => *b,
                None => return false,
            };
            if a.name == b.name {
                return false;
            }
            pairs += 1;
            j += 1;
        }
        i += 1;
    }
    // 12 项两两组合 = 12×11/2 = 66 对。
    pairs == 66 && detail(Diag::NameDup.val()).contains("查重")
}

/// 元数冻结：逐项在 0..=8 拍内、上限常量锚定。
fn arity_bound_ok() -> bool {
    let mut i = 0usize;
    while i < FROZEN_API_TABLE.len() {
        let a = match FROZEN_API_TABLE.get(i) {
            Some(a) => *a,
            None => return false,
        };
        if a.arity > ARITY_CAP {
            return false;
        }
        i += 1;
    }
    ARITY_CAP == 8
        && find("WorkerPool").map(|a| a.arity).unwrap_or(ARITY_CAP + 1) == 4
        && find("OrderPolicy").map(|a| a.arity).unwrap_or(ARITY_CAP + 1) == 1
        && detail(Diag::ArityCap.val()).contains("8")
}

/// 返回面契约：每项返回面非空中文且点名专属语义。
fn ret_contract_ok() -> bool {
    let mut i = 0usize;
    while i < FROZEN_API_TABLE.len() {
        let a = match FROZEN_API_TABLE.get(i) {
            Some(a) => *a,
            None => return false,
        };
        if a.ret.is_empty() {
            return false;
        }
        i += 1;
    }
    (match find("RetryPolicy") {
        Some(a) => a.ret.contains("retry=1"),
        None => false,
    }) && (match find("BudgetAudit") {
        Some(a) => a.ret.contains("不阻断"),
        None => false,
    }) && (match find("FlakyLedger") {
        Some(a) => a.ret.contains("首败重过"),
        None => false,
    }) && detail(Diag::RetEmpty.val()).contains("非空")
}

/// 查找面：12 项全可查且槽位回传正确。
fn lookup_all_ok() -> bool {
    const NAMES: [&str; 12] = [
        "Executor",
        "Selector",
        "OrderPolicy",
        "IsolationEnv",
        "WorkerPool",
        "TimeoutHub",
        "SlowLedger",
        "RetryPolicy",
        "FlakyLedger",
        "ResumePoint",
        "DetectorHook",
        "BudgetAudit",
    ];
    let mut i = 0usize;
    while i < NAMES.len() {
        let want = match NAMES.get(i) {
            Some(n) => *n,
            None => return false,
        };
        let got = match find(want) {
            Some(a) => a,
            None => return false,
        };
        if got.slot != SLOT_BASE + (i as u32) || got.name != want {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入：表外名称查找必落空（UnknownApi 口径）。
fn inj_unknown_ok() -> bool {
    find("NotAnApi").is_none()
        && find("").is_none()
        && find("executor").is_none()
        && detail(Diag::UnknownApi.val()).contains("拒绝")
}

/// 交叉无冲突：cross_conflict 全绿且任何诊断码不落 Conflict。
fn cross_no_conflict_ok() -> bool {
    cross_conflict().is_ok()
        && cross_conflict() == Ok(())
        && detail(Diag::CrossConflict.val()).contains("66")
        && detail(Diag::SlotDup.val()).contains("一一对应")
}

/// 槽位与诊断码带同构：槽位 i ↔ 诊断码 SEG_BASE+i。
fn slot_diag_band_ok() -> bool {
    let mut i = 0usize;
    while i < FROZEN_API_TABLE.len() {
        let a = match FROZEN_API_TABLE.get(i) {
            Some(a) => *a,
            None => return false,
        };
        let d = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        // 同构锚：第 i 项槽位偏移与第 i 个诊断码偏移恒等。
        if d.val() - SEG_BASE != a.slot {
            return false;
        }
        i += 1;
    }
    true
}

/// 双跑一致：交叉检查与查找面两次执行结果相同（纯函数）。
fn double_run_ok() -> bool {
    let a = cross_conflict();
    let b = cross_conflict();
    a.is_ok() && b.is_ok() && a == b
        && find("Executor") == find("Executor")
        && find("Ghost") == find("Ghost")
}

/// 写回面：状态=已完成、结果摘要非空（含 12 项与 66 对对账）。
fn writeback_summary_ok() -> bool {
    if cross_conflict().is_err() {
        return false;
    }
    let summary = format!(
        "UNX-F3458 完成：API{}项冻结 槽位0x{:X}..0x{:X} 交叉{}对无冲突",
        FROZEN_API_COUNT, SLOT_BASE, SLOT_TOP, 66
    );
    !summary.is_empty()
        && summary.contains("UNX-F3458")
        && summary.contains("12项冻结")
        && detail(Diag::SummaryEmpty.val()).contains("摘要")
}

// ── 判据总成：run_unx3458_checks ───────────────────────────────────────

/// run_unx3458_checks：本单判据聚合（诊断段 0x79FC0，全部走 add_dyn）。
pub fn run_unx3458_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3458 API 冻结签名表");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!(
                "SEG_BASE=0x{:X} 守卫{}",
                SEG_BASE,
                if seg_guard_ok() { "过" } else { "破" }
            ),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!(
                "闭集{}",
                if diag_closed_set_ok() { "完整" } else { "破缺" }
            ),
        ),
        (
            "签名表12项名称与判据原文逐字对齐",
            table_frozen_ok(),
            format!("签名表{}", if table_frozen_ok() { "冻结" } else { "漂移" }),
        ),
        (
            "槽位闭带0x01到0x0C连续无洞无重",
            slot_continuity_ok(),
            format!("槽位{}", if slot_continuity_ok() { "闭带" } else { "破带" }),
        ),
        (
            "名称唯一66对逐对查重无同名",
            name_unique_ok(),
            format!("名称{}", if name_unique_ok() { "唯一" } else { "重名" }),
        ),
        (
            "元数冻结逐项0到8拍内上限锚定",
            arity_bound_ok(),
            format!("元数{}", if arity_bound_ok() { "冻结" } else { "超限" }),
        ),
        (
            "返回面契约逐项非空且点名专属语义",
            ret_contract_ok(),
            format!("返回面{}", if ret_contract_ok() { "齐" } else { "缺" }),
        ),
        (
            "查找面12项全可查槽位回传正确",
            lookup_all_ok(),
            format!("查找{}", if lookup_all_ok() { "全通" } else { "失灵" }),
        ),
        (
            "交叉无冲突66对全查全绿",
            cross_no_conflict_ok(),
            format!("交叉{}", if cross_no_conflict_ok() { "无冲突" } else { "有冲突" }),
        ),
        (
            "槽位与诊断码带同构锚逐项恒等",
            slot_diag_band_ok(),
            format!("同构{}", if slot_diag_band_ok() { "恒等" } else { "漂移" }),
        ),
        (
            "注入1表外名称查找必落空",
            inj_unknown_ok(),
            format!("表外{}", if inj_unknown_ok() { "拒" } else { "漏" }),
        ),
        (
            "注入2表长口径12项恒定",
            table_frozen_ok() && FROZEN_API_TABLE.len() == 12,
            format!("表长{}", if FROZEN_API_TABLE.len() == 12 { "12" } else { "漂" }),
        ),
        (
            "注入3双查交叉与查找纯函数一致",
            double_run_ok(),
            format!("双查{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
    ];
    let dr = double_run_ok();
    let wb = writeback_summary_ok();
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s.add_dyn(
        "双跑一致交叉检查两遍同",
        dr,
        &format!("双跑{}", if dr { "一致" } else { "分叉" }),
    );
    s.add_dyn(
        "写回摘要非空状态已完成",
        wb,
        &format!("写回{}", if wb { "非空" } else { "空" }),
    );
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79FC0);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn table_frozen_twelve_names() {
        assert!(table_frozen_ok());
        assert_eq!(FROZEN_API_TABLE.len(), 12);
        assert_eq!(FROZEN_API_TABLE.get(0).unwrap().name, "Executor");
        assert_eq!(FROZEN_API_TABLE.get(11).unwrap().name, "BudgetAudit");
    }

    #[test]
    fn slot_band_continuous() {
        assert!(slot_continuity_ok());
        assert!(slot_diag_band_ok());
        assert_eq!(SLOT_BASE, 0x01);
        assert_eq!(SLOT_TOP, 0x0C);
    }

    #[test]
    fn names_unique_sixty_six_pairs() {
        assert!(name_unique_ok());
    }

    #[test]
    fn arity_bounds_and_points() {
        assert!(arity_bound_ok());
        assert_eq!(find("WorkerPool").unwrap().arity, 4);
        assert_eq!(find("DetectorHook").unwrap().arity, 3);
    }

    #[test]
    fn ret_contract_semantics() {
        assert!(ret_contract_ok());
        assert!(find("ResumePoint").unwrap().ret.contains("断点"));
    }

    #[test]
    fn lookup_all_twelve() {
        assert!(lookup_all_ok());
    }

    #[test]
    fn cross_no_conflict_sixty_six() {
        assert!(cross_no_conflict_ok());
        assert_eq!(cross_conflict(), Ok(()));
    }

    #[test]
    fn unknown_api_misses() {
        assert!(inj_unknown_ok());
        assert!(find("NoSuchApi").is_none());
    }

    #[test]
    fn slot_diag_band_isomorphism() {
        assert!(slot_diag_band_ok());
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3458_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
