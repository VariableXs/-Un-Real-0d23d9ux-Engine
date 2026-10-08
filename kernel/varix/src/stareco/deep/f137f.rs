//! 深化层三 · F137 API 稳定性承诺（2026-09-26 深化批次三）。
//!
//! 补深使用方工程面（主册 G-D-12）：调用面扫描器（谁还在用旧签名）、
//! 破坏性变更分类器（参数增删/返回变更/语义变更分级）、双读窗口进度
//! （旧调用衰减外推清零日）、弃用公告 lint（版本+替代双要素）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 调用面扫描：消费方登记 → 受变更影响的清单
// ---------------------------------------------------------------------------

/// 消费方登记：消费方名 + 调用的 API 名集合。
pub struct Consumer {
    pub name: &'static str,
    pub calls: alloc::vec::Vec<&'static str>,
}

/// 受影响清单：与变更集相交的消费方（稳定序按登记序）。
pub fn affected_consumers<'a>(
    consumers: &'a [Consumer],
    changed_apis: &[&'static str],
) -> alloc::vec::Vec<&'a str> {
    consumers
        .iter()
        .filter(|c| c.calls.iter().any(|a| changed_apis.contains(a)))
        .map(|c| c.name)
        .collect()
}

// ---------------------------------------------------------------------------
// 破坏性变更分类器：变更种类 → 强制迁移级别
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BreakKind {
    /// 参数增加且无默认（调用点全部编译错）。
    ParamAdded,
    /// 参数删除（调用点编译错）。
    ParamRemoved,
    /// 参数类型变更。
    ParamTypeChanged,
    /// 返回类型变更。
    ReturnChanged,
    /// 语义变更（签名不变、行为变——最阴险：编译器不报）。
    SemanticChanged,
    /// 文档措辞修订（非破坏）。
    DocsOnly,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BreakLevel {
    NonBreaking,
    Silent,
    Hard,
}

pub fn classify_break(k: BreakKind) -> BreakLevel {
    match k {
        BreakKind::ParamAdded
        | BreakKind::ParamRemoved
        | BreakKind::ParamTypeChanged
        | BreakKind::ReturnChanged => BreakLevel::Hard,
        BreakKind::SemanticChanged => BreakLevel::Silent,
        BreakKind::DocsOnly => BreakLevel::NonBreaking,
    }
}

/// CI 门禁规则：Hard 变更必须有 ADR 授权 + 双读窗口；Silent 必须
/// 有行为对拍记录；NonBreaking 直通。
pub fn gate_decision(
    k: BreakKind,
    adr_authorized: bool,
    dual_read_open: bool,
    behavior_audited: bool,
) -> Result<&'static str, &'static str> {
    match classify_break(k) {
        BreakLevel::NonBreaking => Ok("直通：非破坏性修订"),
        BreakLevel::Hard => {
            if !adr_authorized {
                Err("Hard 变更未授权：ADR 缺失，CI 拦截")
            } else if !dual_read_open {
                Err("Hard 变更未开双读窗：旧调用方无迁移期，CI 拦截")
            } else {
                Ok("放行：授权+双读窗齐备")
            }
        }
        BreakLevel::Silent => {
            if !behavior_audited {
                Err("语义变更无对拍记录：最阴险类，CI 拦截")
            } else {
                Ok("放行：语义变更已对拍")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 双读窗口进度：旧调用计数按周衰减 → 线性外推清零日
// ---------------------------------------------------------------------------

pub struct DualReadProgress {
    /// (周序, 旧调用计数) 按周序递增登记。
    pub samples: alloc::vec::Vec<(u32, u32)>,
    pub window_len_days: u32,
}

impl DualReadProgress {
    pub fn new(window_len_days: u32) -> DualReadProgress {
        DualReadProgress { samples: alloc::vec::Vec::new(), window_len_days }
    }

    pub fn record(&mut self, week: u32, old_calls: u32) -> Result<(), &'static str> {
        if let Some((w, _)) = self.samples.last() {
            if week <= *w {
                return Err("周序必须递增：回填拒绝（数据诚实线）");
            }
        }
        self.samples.push((week, old_calls));
        Ok(())
    }

    /// 线性外推：以最近两个样本的斜率推清零周；斜率非负（未衰减）
    /// 返回 None——如实登记"未见收敛"，不硬造日期。
    pub fn projected_zero_week(&self) -> Option<u32> {
        if self.samples.len() < 2 {
            return None;
        }
        let (_, c1) = self.samples[self.samples.len() - 2];
        let (w2, c2) = self.samples[self.samples.len() - 1];
        if c2 >= c1 {
            return None;
        }
        let slope = c1 - c2; // 每周衰减量
        let weeks_left = (c2 + slope - 1) / slope; // 向上取整
        Some(w2 + weeks_left)
    }

    /// 清零日在窗内才达标（迁移期收口判据）。
    pub fn on_track(&self) -> bool {
        match self.projected_zero_week() {
            Some(w) => w * 7 <= self.window_len_days,
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 弃用公告 lint：必须带版本 + 替代双要素（bot 可校验的格式）
// ---------------------------------------------------------------------------

/// 公告文本机器面：`deprecated: since=vN alt=<替代名>` 形态校验。
pub fn deprecation_notice_ok(notice: &str) -> bool {
    let s = notice.trim();
    let Some(rest) = s.strip_prefix("deprecated:") else {
        return false;
    };
    let mut has_since = false;
    let mut has_alt = false;
    for part in rest.split_whitespace() {
        if let Some(v) = part.strip_prefix("since=v") {
            has_since = !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit());
        }
        if let Some(a) = part.strip_prefix("alt=") {
            has_alt = !a.is_empty();
        }
    }
    has_since && has_alt
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F137F_TAG: &str = "stareco-F137-deep3";

pub fn run_f137_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F137F_TAG);

    // 调用面扫描
    let consumers = [
        Consumer { name: "files-app", calls: alloc::vec!["fs_open", "fs_read"] },
        Consumer { name: "search", calls: alloc::vec!["search_query"] },
        Consumer { name: "editor", calls: alloc::vec!["fs_open", "edit_save"] },
    ];
    let hit = affected_consumers(&consumers, &["fs_open"]);
    set.add("f137f affected", hit == alloc::vec!["files-app", "editor"], "调用面命中稳定序");
    set.add(
        "f137f unaffected",
        affected_consumers(&consumers, &["fs_write"]).is_empty(),
        "无交集零误伤",
    );

    // 分类器
    set.add(
        "f137f hard level",
        classify_break(BreakKind::ParamAdded) == BreakLevel::Hard
            && classify_break(BreakKind::ReturnChanged) == BreakLevel::Hard,
        "签名破坏=Hard",
    );
    set.add(
        "f137f silent level",
        classify_break(BreakKind::SemanticChanged) == BreakLevel::Silent,
        "语义变更=Silent",
    );
    set.add(
        "f137f docs level",
        classify_break(BreakKind::DocsOnly) == BreakLevel::NonBreaking,
        "文档修订=NonBreaking",
    );

    // 门禁
    set.add(
        "f137f gate hard ok",
        gate_decision(BreakKind::ParamAdded, true, true, false) == Ok("放行：授权+双读窗齐备"),
        "授权+窗齐放行",
    );
    set.add(
        "f137f gate no adr",
        gate_decision(BreakKind::ParamRemoved, false, true, false).is_err(),
        "无 ADR 拦截",
    );
    set.add(
        "f137f gate no window",
        gate_decision(BreakKind::ParamRemoved, true, false, false).is_err(),
        "无双读窗拦截",
    );
    set.add(
        "f137f gate silent",
        gate_decision(BreakKind::SemanticChanged, false, false, false).is_err(),
        "语义无对拍拦截",
    );
    set.add(
        "f137f gate docs",
        gate_decision(BreakKind::DocsOnly, false, false, false).is_ok(),
        "文档修订直通",
    );

    // 双读进度
    let mut dp = DualReadProgress::new(90);
    set.add("f137f backfill reject", dp.record(2, 10).is_ok() && dp.record(2, 9).is_err(), "周序回填拒绝");
    let _ = dp.record(4, 8);
    let _ = dp.record(6, 6);
    set.add("f137f projection", dp.projected_zero_week() == Some(9), "斜率 1/周→第 9 周清零");
    set.add("f137f on track", dp.on_track(), "9×7=63 ≤ 90 在窗内");
    let mut dp2 = DualReadProgress::new(90);
    let _ = dp2.record(1, 5);
    let _ = dp2.record(2, 7); // 上升——未收敛
    set.add("f137f no convergence", dp2.projected_zero_week().is_none() && !dp2.on_track(), "未收敛如实登记");

    // 弃用公告 lint
    set.add(
        "f137f notice ok",
        deprecation_notice_ok("deprecated: since=v3 alt=fs_open2"),
        "双要素齐",
    );
    set.add("f137f notice no alt", !deprecation_notice_ok("deprecated: since=v3"), "缺替代拒绝");
    set.add("f137f notice no ver", !deprecation_notice_ok("deprecated: alt=x"), "缺版本拒绝");
    set.add("f137f notice wrong", !deprecation_notice_ok("will be removed"), "非公告格式拒绝");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn projection_boundary() {
        let mut dp = DualReadProgress::new(90);
        let _ = dp.record(0, 10);
        let _ = dp.record(3, 4); // 斜率 6/周，余 4 → 1 周清零 → 第 4 周
        assert_eq!(dp.projected_zero_week(), Some(4));
        // 单样本不外推
        let mut one = DualReadProgress::new(90);
        let _ = one.record(1, 3);
        assert!(one.projected_zero_week().is_none());
    }

    #[test]
    fn notice_variants() {
        assert!(deprecation_notice_ok("deprecated: since=v10 alt=a_b"));
        assert!(!deprecation_notice_ok("deprecated: since= alt=x"));
        assert!(!deprecation_notice_ok("deprecated: since=v alt=x"));
    }
}
