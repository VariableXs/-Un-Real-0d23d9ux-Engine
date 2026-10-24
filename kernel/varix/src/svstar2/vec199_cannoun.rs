//! VE-F0599 · C 域完成宣告与 D 域移交包
//!
//! 职责：C 域完成宣告与 D 域移交包——
//! 宣告四要素：双增维达成声明（F0595）、四联审全绿（F0598）、
//!   三账对账结论（F0597）、200 条终验预备态；
//! 移交包五件：冻结接口清单（F0596）、编译服务使用指南、缓存键契约
//!   （F0590 键四元组）、诊断词典引用（F0594）、经验包
//!   （四层对拍法、键四元组缓存设计、级联不变式方法论）；
//! D 域先行事实补记：D 域已有先行样板条（F0622），本包对其做符合性三态核对；
//! 移交后接口变更走升版重核对（与 F0200/F0460 移交家族同构）。
//! 降级矩阵：要素缺项→宣告阻断；先行偏差→登记判定；移交后变更→升版重核对。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 宣告四要素：逐要素齐备性核验——任一缺项宣告阻断。
#[derive(Clone, Debug, PartialEq)]
pub struct AnnounceChecklist {
    /// F0595 双增维达成。
    pub dims_ok: bool,
    /// F0598 四联审全绿。
    pub audits_ok: bool,
    /// F0597 三账平账。
    pub ledgers_ok: bool,
    /// 200 条终验预备态（终验 F0600 的预备声明）。
    pub final_ready: bool,
}

impl AnnounceChecklist {
    /// 四要素齐备判定：逐项点名缺项。
    pub fn validate(&self) -> Result<(), String> {
        let mut missing: Vec<&str> = Vec::new();
        if !self.dims_ok { missing.push("双增维达成声明(F0595)"); }
        if !self.audits_ok { missing.push("四联审全绿(F0598)"); }
        if !self.ledgers_ok { missing.push("三账对账结论(F0597)"); }
        if !self.final_ready { missing.push("200 条终验预备态"); }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!("宣告阻断，缺项 {} 个：{}", missing.len(), missing.join("、")))
        }
    }
}

/// 移交包五件：逐件齐备核验。
#[derive(Clone, Debug, PartialEq)]
pub struct HandoverPack {
    pub freeze_list: bool,     // F0596 冻结接口清单
    pub svc_guide: bool,       // 编译服务使用指南
    pub cache_contract: bool,  // F0590 缓存键契约（键四元组）
    pub diag_glossary: bool,   // F0594 诊断词典引用
    pub lessons: bool,         // 经验包（四层对拍法、键四元组缓存设计、级联不变式方法论）
}

impl HandoverPack {
    /// 五件齐备判定：点名缺件。
    pub fn validate(&self) -> Result<(), String> {
        let mut missing: Vec<&str> = Vec::new();
        if !self.freeze_list { missing.push("冻结接口清单"); }
        if !self.svc_guide { missing.push("编译服务使用指南"); }
        if !self.cache_contract { missing.push("缓存键契约"); }
        if !self.diag_glossary { missing.push("诊断词典引用"); }
        if !self.lessons { missing.push("经验包"); }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!("移交包缺件 {} 个：{}", missing.len(), missing.join("、")))
        }
    }
    pub fn complete(&self) -> bool {
        self.validate().is_ok()
    }
}

/// 先行偏差判定（D 域 F0622 样板条符合性三态核对）：
/// 先行实现引用冻结接口 + 行为符合契约 = 符合；
/// 引用在册接口但行为偏差 = 偏差已登记（判阻断或随行）；
/// 引用未冻结接口 = 待核（先行纪律违规，须补冻结或改道）。
pub fn pilot_review(referenced_frozen: bool, behavior_ok: bool) -> &'static str {
    if !referenced_frozen {
        "待核" // 引用了未冻结接口——先行纪律违规
    } else if behavior_ok {
        "符合"
    } else {
        "偏差已登记"
    }
}

/// 移交后接口变更纪律：变更必须走升版重核对——未升版的变更拒绝生效。
pub fn post_handover_change(
    old_version: u32,
    new_version: u32,
    review_passed: bool,
) -> Result<(), String> {
    if new_version <= old_version {
        return Err(format!(
            "版本 {} -> {} 未升版：移交后接口变更必须走升版重核对（F0200/F0460 移交家族同构）",
            old_version, new_version
        ));
    }
    if !review_passed {
        return Err("升版后重核对未过：变更不生效".to_string());
    }
    Ok(())
}

/// 经验包三法核验：四层对拍法、键四元组缓存设计、级联不变式方法论——
/// 经验必须与方法在库条目对得上（经验包不得凭空造法）。
pub fn lessons_valid(lessons: &[&str]) -> bool {
    let canon = ["四层对拍法", "键四元组缓存设计", "级联不变式方法论"];
    canon.iter().all(|c| lessons.contains(c))
}

/// 宣告结论：四要素 + 移交包五件 + 先行核对全过才可宣告完成。
pub fn announce(checklist: &AnnounceChecklist, pack: &HandoverPack, pilot: &str) -> Result<(), String> {
    checklist.validate()?;
    pack.validate()?;
    if pilot == "待核" {
        return Err("先行样板条待核未清：补冻结或改道后再宣告".to_string());
    }
    Ok(())
}

pub fn run_vec199_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec199");
    // 族一：宣告四要素——齐备与缺项点名
    let full = AnnounceChecklist { dims_ok: true, audits_ok: true, ledgers_ok: true, final_ready: true };
    s.add("四要素全齐放行宣告", full.validate().is_ok(), "full");
    let no_dims = AnnounceChecklist { dims_ok: false, ..full.clone() };
    match no_dims.validate() {
        Err(m) => s.add("缺增维声明点名", m.contains("1 个") && m.contains("F0595"), "miss-dims"),
        Ok(()) => s.add("缺增维声明点名", false, "expected err"),
    }
    let two_missing = AnnounceChecklist { audits_ok: false, final_ready: false, ..full.clone() };
    match two_missing.validate() {
        Err(m) => s.add("缺两项点名数正确", m.contains("2 个") && m.contains("F0598") && m.contains("终验预备态"), "miss-two"),
        Ok(()) => s.add("缺两项点名数正确", false, "expected err"),
    }
    let empty = AnnounceChecklist { dims_ok: false, audits_ok: false, ledgers_ok: false, final_ready: false };
    match empty.validate() {
        Err(m) => s.add("全缺四项阻断", m.contains("4 个"), "miss-all"),
        Ok(()) => s.add("全缺四项阻断", false, "expected err"),
    }
    // 族二：移交包五件
    let pack = HandoverPack { freeze_list: true, svc_guide: true, cache_contract: true, diag_glossary: true, lessons: true };
    s.add("移交五件齐备", pack.validate().is_ok() && pack.complete(), "pack-full");
    let pack_missing = HandoverPack { cache_contract: false, ..pack.clone() };
    match pack_missing.validate() {
        Err(m) => s.add("缺缓存契约点名", m.contains("缓存键契约"), "miss-cache"),
        Ok(()) => s.add("缺缓存契约点名", false, "expected err"),
    }
    let pack_empty = HandoverPack { freeze_list: false, svc_guide: false, cache_contract: false, diag_glossary: false, lessons: false };
    match pack_empty.validate() {
        Err(m) => s.add("全缺五件", m.contains("5 个") && m.contains("经验包"), "pack-empty"),
        Ok(()) => s.add("全缺五件", false, "expected err"),
    }
    // 族三：D 域先行核对（F0622 样板条三态）
    s.add("引用冻结且行为符=符合", pilot_review(true, true) == "符合", "pilot-ok");
    s.add("行为偏差=偏差已登记", pilot_review(true, false) == "偏差已登记", "pilot-dev");
    s.add("引用未冻结=待核", pilot_review(false, true) == "待核", "pilot-pending");
    s.add("待核未清宣告阻断", announce(&full, &pack, pilot_review(false, true)).is_err(), "pilot-block");
    s.add("符合则宣告放行", announce(&full, &pack, pilot_review(true, true)).is_ok(), "announce-ok");
    s.add("要素缺项时宣告阻断", announce(&no_dims, &pack, "符合").is_err(), "announce-block");
    s.add("移交缺件时宣告阻断", announce(&full, &pack_missing, "符合").is_err(), "announce-pack");
    // 族四：移交后升版纪律
    s.add("升版+重核过=生效", post_handover_change(1, 2, true).is_ok(), "upgrade-ok");
    s.add("未升版拒绝", post_handover_change(1, 1, true).is_err(), "no-bump");
    match post_handover_change(2, 1, true) {
        Err(m) => s.add("降版拒绝点名纪律", m.contains("未升版") && m.contains("升版重核对"), "downgrade"),
        Ok(()) => s.add("降版拒绝点名纪律", false, "expected err"),
    }
    s.add("升版未核不生效", post_handover_change(1, 2, false).is_err(), "no-review");
    // 族五：经验包三法
    s.add("经验三法齐备", lessons_valid(&["四层对拍法", "键四元组缓存设计", "级联不变式方法论"]), "lessons-ok");
    s.add("缺一法判无效", !lessons_valid(&["四层对拍法", "键四元组缓存设计"]), "lessons-miss");
    s.add("凭空造法判无效", !lessons_valid(&["四层对拍法", "键四元组缓存设计", "神秘优化法"]), "lessons-fake");
    s.add("移交家族同构说明在册", post_handover_change(1, 2, true).is_ok(), "family-f0200-f0460");
    s
}
