//! F568 任务栏中键关闭 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：图标/缩略图两层中键；三问联动；与 F419 语义一致；
//! 误触防护（无确认直接关的风险评估——仅对无未保存窗生效）；开关可关。
//!
//! **设计要点（主册）**：
//! - 中键点任务栏图标 = 关闭该应用全部窗口（中键党的 muscle memory）；
//! - 多窗时与 F419「关闭所有」语义一致（未保存三问 F310 逐窗）；
//! - 中键点缩略图 = 关那一个窗（F352 已有 ×，中键是快捷等价）；
//! - 误触防护：直接关闭仅对无未保存窗生效——有未保存窗必走三问；
//! - 开关可关（非中键用户零感知）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 一个窗口的未保存态（F310 三问的输入面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirtyState {
    Clean,
    Unsaved,
}

/// 中键关闭引擎（任务栏侧）。
pub struct MidClose {
    enabled: bool,
    /// 应用 → 窗口账（id, 未保存态）。
    apps: Vec<(String, Vec<(u64, DirtyState)>)>,
    /// 实际被关闭的窗口账（对账面——F419 语义一致性对拍）。
    closed: Vec<(u64, &'static str)>,
}

impl MidClose {
    pub fn new() -> MidClose {
        MidClose {
            enabled: true,
            apps: Vec::new(),
            closed: Vec::new(),
        }
    }

    /// 开关（可关——非中键用户零感知）。
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// 登记窗口。
    pub fn add_win(&mut self, app: &str, id: u64, dirty: DirtyState) {
        match self.apps.iter_mut().find(|(a, _)| a == app) {
            Some((_, ws)) => ws.push((id, dirty)),
            None => self.apps.push((String::from(app), alloc::vec![(id, dirty)])),
        }
    }

    /// 窗口未保存态更新（F310 输入面）。
    pub fn set_dirty(&mut self, id: u64, dirty: DirtyState) -> bool {
        for (_, ws) in self.apps.iter_mut() {
            for w in ws.iter_mut() {
                if w.0 == id {
                    w.1 = dirty;
                    return true;
                }
            }
        }
        false
    }

    /// 中键点任务栏图标：关该应用全部窗口。
    ///
    /// 误触防护语义（主册）：无未保存窗 → 直接关；任一窗未保存 →
    /// 返回该窗清单走三问（F310 逐窗），不直接关任何窗。
    pub fn middle_click_icon(&mut self, app: &str) -> Result<alloc::vec::Vec<u64>, &'static str> {
        if !self.enabled {
            return Err("中键关闭已关");
        }
        let ws = match self.apps.iter().find(|(a, _)| a == app) {
            Some((_, ws)) => ws.clone(),
            None => return Err("应用无窗口"),
        };
        if ws.iter().any(|(_, d)| *d == DirtyState::Unsaved) {
            // 有未保存 → 三问链（此处交还调用方逐窗三问，不直接关）。
            return Err("存在未保存窗口——需逐窗三问");
        }
        let ids: Vec<u64> = ws.iter().map(|(id, _)| *id).collect();
        for id in &ids {
            self.closed.push((*id, "icon-middle"));
        }
        self.purge(app);
        Ok(ids)
    }

    /// 中键点缩略图：关那一个窗（F352 × 的快捷等价）。
    pub fn middle_click_thumb(&mut self, app: &str, id: u64) -> Result<(), &'static str> {
        if !self.enabled {
            return Err("中键关闭已关");
        }
        let ws = match self.apps.iter().find(|(a, _)| a == app) {
            Some((_, ws)) => ws.clone(),
            None => return Err("应用无窗口"),
        };
        let w = ws.iter().find(|(i, _)| *i == id).ok_or("窗口不存在")?;
        if w.1 == DirtyState::Unsaved {
            return Err("未保存窗口——需三问");
        }
        self.closed.push((id, "thumb-middle"));
        self.purge_one(app, id);
        Ok(())
    }

    /// 三问（F310）裁决后的强制关（未保存窗经三问「不保存」后的落点——
    /// 与 F419 关闭所有语义同链）。
    pub fn force_close_after_prompt(&mut self, app: &str, id: u64) -> bool {
        let ok = self.purge_one(app, id);
        if ok {
            self.closed.push((id, "after-three-questions"));
        }
        ok
    }

    /// 关闭账（F419 一致性对拍面）。
    pub fn closed_log(&self) -> &[(u64, &'static str)] {
        &self.closed
    }

    fn purge(&mut self, app: &str) {
        self.apps.retain(|(a, _)| a != app);
    }

    fn purge_one(&mut self, app: &str, id: u64) -> bool {
        for (a, ws) in self.apps.iter_mut() {
            if a == app {
                let before = ws.len();
                ws.retain(|(i, _)| *i != id);
                return ws.len() != before;
            }
        }
        false
    }
}

impl Default for MidClose {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_midclose_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 图标层中键：无未保存 → 全窗直关（返回被关 id 清单）。
    let mut m = MidClose::new();
    m.add_win("浏览器", 1, DirtyState::Clean);
    m.add_win("浏览器", 2, DirtyState::Clean);
    let closed = m.middle_click_icon("浏览器");
    set.add(
        "icon middle closes all clean",
        closed == Ok(alloc::vec![1u64, 2]) && m.closed_log().len() == 2,
        "",
    );

    // 2. 三问联动：有未保存窗 → 拒直接关、要求逐窗三问。
    let mut m2 = MidClose::new();
    m2.add_win("记事本", 3, DirtyState::Unsaved);
    m2.add_win("记事本", 4, DirtyState::Clean);
    let guarded = m2.middle_click_icon("记事本");
    set.add(
        "unsaved routes to three questions",
        guarded == Err("存在未保存窗口——需逐窗三问") && m2.closed_log().is_empty(),
        "",
    );

    // 3. 三问后落点：逐窗裁决「不保存」→ force_close 落账。
    let forced = m2.force_close_after_prompt("记事本", 3);
    let rest = m2.middle_click_icon("记事本");
    set.add(
        "after prompt flow completes",
        forced && rest == Ok(alloc::vec![4u64]),
        "",
    );

    // 4. 缩略图层中键：关单窗（另一窗存活）。
    let mut m3 = MidClose::new();
    m3.add_win("终端", 5, DirtyState::Clean);
    m3.add_win("终端", 6, DirtyState::Clean);
    m3.middle_click_thumb("终端", 5).unwrap();
    let all = m3.middle_click_icon("终端");
    set.add(
        "thumb middle closes single",
        m3.closed_log().len() == 2 && all == Ok(alloc::vec![6u64]),
        "",
    );

    // 5. 缩略图层未保存窗：中键拒关（防护同样生效）。
    let mut m4 = MidClose::new();
    m4.add_win("文档", 7, DirtyState::Unsaved);
    let thumb_unsaved = m4.middle_click_thumb("文档", 7);
    set.add(
        "thumb unsaved guarded too",
        thumb_unsaved == Err("未保存窗口——需三问") && m4.closed_log().is_empty(),
        "",
    );

    // 6. 与 F419 语义一致：关闭账三层来源（icon/thumb/three-questions）
    //    走同一账面（对拍唯一源）。
    let sources: Vec<&str> = m3.closed_log().iter().map(|(_, s)| *s).collect();
    set.add(
        "f419 unified close ledger",
        sources.contains(&"thumb-middle") && sources.contains(&"icon-middle"),
        "",
    );

    // 7. 开关可关：关后中键全部拒绝（非中键用户零感知）。
    let mut m5 = MidClose::new();
    m5.add_win("画图", 8, DirtyState::Clean);
    m5.set_enabled(false);
    set.add(
        "off switch blocks middle click",
        !m5.enabled() && m5.middle_click_icon("画图") == Err("中键关闭已关"),
        "",
    );

    // 8. 空应用/未知窗诚实拒绝。
    let mut m6 = MidClose::new();
    set.add(
        "unknown app honest error",
        m6.middle_click_icon("无") == Err("应用无窗口"),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_update_reflects_guard() {
        let mut m = MidClose::new();
        m.add_win("a", 1, DirtyState::Clean);
        assert!(m.middle_click_icon("a").is_ok());
        m.add_win("b", 2, DirtyState::Clean);
        m.set_dirty(2, DirtyState::Unsaved);
        assert!(m.middle_click_icon("b").is_err());
        m.set_dirty(2, DirtyState::Clean);
        assert!(m.middle_click_icon("b").is_ok());
    }

    #[test]
    fn thumb_missing_window_errs() {
        let mut m = MidClose::new();
        m.add_win("a", 1, DirtyState::Clean);
        assert!(m.middle_click_thumb("a", 9).is_err());
    }

    #[test]
    fn force_close_missing_false() {
        let mut m = MidClose::new();
        assert!(!m.force_close_after_prompt("a", 1));
    }
}
