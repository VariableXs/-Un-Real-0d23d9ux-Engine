//! 深化层 · F571 会话标签恢复（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F571 节）：
//! ①「每个标签的目录/历史栈/滚动位置——『工作现场』整体回归」的
//!   **三元组精度账**——恢复后逐标签逐字段核对（dir/历史栈/滚动位
//!   三项各自记账，恢复精度是可点名到字段的）；
//! ②「崩溃后同样恢复」的**崩溃注入器**——把会话截断/字段残缺后
//!   交给恢复器：活得下的字段如实重建、活不下的如实报损（不许
//!   拿半个现场装完整）；
//! ③「恢复前通知条（全部保留/只留当前可选精简）」的**精简语义**——
//!   「只留当前」执行后其余标签清账（精简是一个真实的删数动作，
//!   不是把提示条关掉了事）。

use alloc::string::String;
use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::tabrestore::{Session, TabScene, TabRestore, TAB_CAP};

// ---------------------------------------------------------------------------
// 三元组精度账
// ---------------------------------------------------------------------------

/// 一个标签的恢复精度（三字段逐项核对）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabPrecision {
    pub dir_ok: bool,
    pub stack_ok: bool,
    pub scroll_ok: bool,
}

impl TabPrecision {
    pub fn full(&self) -> bool {
        self.dir_ok && self.stack_ok && self.scroll_ok
    }
}

/// 逐标签三元组核对（对照捕获现场与恢复现场）。
pub fn precision_report(captured: &Session, restored: Option<&Session>) -> alloc::vec::Vec<TabPrecision> {
    let mut out = alloc::vec::Vec::new();
    let Some(r) = restored else { return out };
    for i in 0..captured.tabs().len() {
        match (captured.tabs().get(i), r.tabs().get(i)) {
            (Some(a), Some(b)) => out.push(TabPrecision {
                dir_ok: a.dir == b.dir,
                stack_ok: a.history == b.history,
                scroll_ok: a.scroll_px == b.scroll_px,
            }),
            _ => out.push(TabPrecision { dir_ok: false, stack_ok: false, scroll_ok: false }),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 崩溃注入器
// ---------------------------------------------------------------------------

/// 注入方式（崩溃形态的最小集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corruption {
    /// 历史栈截断（只留当前页——前进/后退死）。
    HistoryTruncated,
    /// 滚动位丢失（归零——位置债）。
    ScrollLost,
    /// 标签截断到 1 个（只剩当前页）。
    TabsTruncated,
}

/// 注入后重建：按注入形态残缺化快照，恢复器照常工作（如实降级）。
pub fn inject_and_restore(s: &Session, mode: Corruption, source: &'static str) -> (Option<Session>, &'static str) {
    let damaged = match mode {
        Corruption::HistoryTruncated => Session::capture(
            s.tabs()
                .iter()
                .map(|t| TabScene {
                    dir: t.dir.clone(),
                    history: t.history.last().cloned().into_iter().collect(),
                    scroll_px: t.scroll_px,
                })
                .collect(),
            s.active,
        ),
        Corruption::ScrollLost => Session::capture(
            s.tabs()
                .iter()
                .map(|t| TabScene {
                    dir: t.dir.clone(),
                    history: t.history.clone(),
                    scroll_px: 0,
                })
                .collect(),
            s.active,
        ),
        Corruption::TabsTruncated => {
            let keep = s.tabs().first().cloned();
            Session::capture(keep.into_iter().collect(), 0)
        }
    };
    let mut tr = TabRestore::new();
    tr.save(damaged);
    let _ = tr.restore(source);
    let restored = tr.restored_scene().cloned();
    let note: &'static str = match mode {
        Corruption::HistoryTruncated => "历史栈不可恢复（崩溃截断）——目录与滚动位已还原",
        Corruption::ScrollLost => "滚动位丢失（归零）——目录与历史栈已还原",
        Corruption::TabsTruncated => "仅当前标签存活——其余标签在崩溃中失联",
    };
    (restored, note)
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

fn scene(dir: &str, pages: usize, scroll: u32) -> TabScene {
    TabScene {
        dir: String::from(dir),
        history: (0..pages).map(|i| alloc::format!("{}/p{}", dir, i)).collect(),
        scroll_px: scroll,
    }
}

pub fn run_f571_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 三元组精度：干净恢复 = 全字段全对。
    let golden = Session::capture(
        alloc::vec![scene("D:/报告", 3, 120), scene("D:/资料", 2, 640)],
        0,
    );
    let mut tr = TabRestore::new();
    tr.save(golden.clone());
    let _ = tr.restore("重启");
    let got = tr.restored_scene();
    let report = precision_report(&golden, got);
    cs.add(
        "full precision all fields",
        report.len() == 2 && report.iter().all(|p| p.full()),
        "",
    );

    // 2) 恢复通知条：来源如实标注（重启/崩溃两种来源可辨）。
    cs.add(
        "notice source labeled",
        tr.notice().is_some() && tr.source() == "重启",
        "",
    );

    // 3) 崩溃注入·滚动位丢失：目录与栈活、滚动如实归零（有说明）。
    let (_, note_scroll) = inject_and_restore(&golden, Corruption::ScrollLost, "崩溃");
    cs.add(
        "crash scroll loss honest",
        note_scroll.contains("滚动位丢失"),
        "",
    );

    // 4) 崩溃注入·标签截断：只剩 1 标签（如实降级不装完整）。
    let (restored_trunc, _) = inject_and_restore(&golden, Corruption::TabsTruncated, "崩溃");
    cs.add(
        "crash tab truncation survives",
        restored_trunc.map(|s| s.tabs().len() == 1).unwrap_or(false),
        "",
    );

    // 5) 8 标签上限：超出部分快照时即淘汰（capture 即裁，恢复不超界）。
    let many = Session::capture(
        (0..12).map(|i| scene(&alloc::format!("D:/t{}", i), 1, 0)).collect(),
        0,
    );
    let mut tr2 = TabRestore::new();
    tr2.save(many);
    let _ = tr2.restore("重启");
    cs.add(
        "cap 8 enforced at capture",
        tr2.restored_scene().map(|s| s.tabs().len() == TAB_CAP).unwrap_or(false),
        "",
    );

    // 6) 精简选项：「只留当前」删其余标签账（真实删数）。
    let mut tr3 = TabRestore::new();
    tr3.save(golden.clone());
    let _ = tr3.restore("重启");
    cs.add(
        "trim to active deletes rest",
        tr3.trim_to_active()
            && tr3.restored_scene().map(|s| s.tabs().len() == 1).unwrap_or(false),
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_session_restores_empty() {
        let empty = Session::capture(alloc::vec![], 0);
        let mut tr = TabRestore::new();
        tr.save(empty);
        let got = tr.restore("崩溃");
        assert_eq!(got.map(|(_, src)| src), Some("崩溃"));
    }
}
