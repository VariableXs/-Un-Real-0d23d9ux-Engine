//! 提交者反馈回路（WP-305 · B-4303 提交状态全链路可查）。
//!
//! MD2 篇 43.3：每份提交在星图页可见状态（已收到、复核中、已收录、未收录
//! 加理由），收录的星卡标注"社区首发"贡献标识；未收录的给具体差距（缺哪
//! 条判例、哪个数据矛盾）——**让下一次提交更接近收录，通道才是活的**。季
//! 度统计（提交量、收录率、Top 贡献者）进生态运营报表，通道的健康度被运
//! 营而不是被遗忘。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 状态四态（穷举）与差距面
// ---------------------------------------------------------------------------

/// 提交状态（穷举四态——状态面没有第五种显示）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SubmState {
    /// 已收到（进复核队列）。
    Received,
    /// 复核中。
    InReview,
    /// 已收录。
    Accepted,
    /// 未收录（必须带具体理由——见 Gap）。
    Rejected,
}

/// 具体差距（未收录的理由必须落到数字：缺哪条判例、哪个数据矛盾）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gap {
    /// 缺失判例数（"缺哪条判例"的计数面）。
    pub missing_cases: u8,
    /// 数据矛盾点数（"哪个数据矛盾"的计数面）。
    pub data_conflicts: u8,
}

impl Gap {
    /// 理由具体判：至少一处可指认的差距——"不够好"式的泛泛理由不算理由。
    pub fn specific(&self) -> bool {
        self.missing_cases > 0 || self.data_conflicts > 0
    }
}

/// 社区首发标注判（收录的星卡标注"社区首发"贡献标识）——标注与来源恒等：
/// 社区来源收录必标（漏标即红）、非社区来源不许标（多标即红）、未收录不许标。
pub fn first_label_ok(state: SubmState, from_community: bool, labeled: bool) -> bool {
    match state {
        SubmState::Accepted => labeled == from_community,
        _ => !labeled,
    }
}

// ---------------------------------------------------------------------------
// 全链路可查（状态迁移留痕）
// ---------------------------------------------------------------------------

/// 全链路迁移数：已收到→复核中→终态（收录或未收录），恰好两跳。
pub const FULL_TRACE_HOPS: u8 = 2;

/// 全链路可查判（**B-4303 达标线：提交状态全链路可查**）——终态提交的
/// 留痕必须恰好两跳：少一跳是没走完流程，多一跳是流程外有影子站。
pub fn full_trace(hops: u8) -> bool {
    hops == FULL_TRACE_HOPS
}

// ---------------------------------------------------------------------------
// 季度统计（通道健康度被运营不是被遗忘）
// ---------------------------------------------------------------------------

/// 季度统计三数（提交量、收录率、Top 贡献者——进生态运营报表）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QuarterStats {
    /// 提交量。
    pub submissions: u16,
    /// 收录量。
    pub accepted: u16,
    /// Top 贡献者编号（0 = 无）。
    pub top_contrib: u8,
}

impl QuarterStats {
    /// 收录率（千分比，下取整）——零提交返回 None 不编数（与 wineattr
    /// hit_permille 同款语义：没数据就说没数据）。
    pub fn accept_permille(&self) -> Option<u16> {
        if self.submissions == 0 {
            None
        } else {
            Some(self.accepted * 1000 / self.submissions)
        }
    }

    /// 统计自洽：收录量不超过提交量；收录率分母为零时收录量也必为零。
    pub fn coherent(&self) -> bool {
        self.accepted <= self.submissions
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-4303 · 5 项）
// ---------------------------------------------------------------------------

pub fn run_feedback_checks() -> CheckSet {
    let mut set = CheckSet::new("B-4303 反馈可见");
    // 1. 状态四态穷举：四态可构造且互异（状态面没有第五种显示）。
    let four = [
        SubmState::Received,
        SubmState::InReview,
        SubmState::Accepted,
        SubmState::Rejected,
    ];
    let mut all_diff = true;
    let mut i = 0;
    while i < four.len() {
        let mut j = i + 1;
        while j < four.len() {
            if four[i] == four[j] {
                all_diff = false;
            }
            j += 1;
        }
        i += 1;
    }
    set.add(
        "B-4303 状态四态穷举",
        all_diff,
        "已收到/复核中/已收录/未收录——四态穷举，星图页没有第五种显示",
    );
    // 2. 未收录必给具体差距：泛泛理由拒（B-4303 达标线的理由面）。
    let empty_gap = Gap { missing_cases: 0, data_conflicts: 0 };
    let gap_cases = Gap { missing_cases: 3, data_conflicts: 0 };
    let gap_conflict = Gap { missing_cases: 0, data_conflicts: 1 };
    set.add(
        "B-4303 未收录给差距",
        !empty_gap.specific() && gap_cases.specific() && gap_conflict.specific(),
        "缺哪条判例、哪个数据矛盾——落到数字的理由才让下一次提交更接近收录",
    );
    // 3. 社区首发标注：标注只属于社区来源的收录卡。
    set.add(
        "B-4303 社区首发标注",
        first_label_ok(SubmState::Accepted, true, true)
            && !first_label_ok(SubmState::Accepted, true, false)
            && !first_label_ok(SubmState::Accepted, false, true)
            && !first_label_ok(SubmState::Rejected, true, true),
        "收录的社区卡必标'社区首发'——未收录不许标、非社区来源不许标",
    );
    // 4. 全链路可查：终态恰好两跳（B-4303 达标线的留痕面）。
    set.add(
        "B-4303 全链路可查",
        full_trace(2) && !full_trace(1) && !full_trace(3),
        "已收到→复核中→终态恰好两跳——少一跳没走完，多一跳流程外有影子站",
    );
    // 5. 季度统计三数：收录率千分比、零提交不编数、账面自洽。
    let q = QuarterStats { submissions: 40, accepted: 25, top_contrib: 7 };
    let q0 = QuarterStats { submissions: 0, accepted: 0, top_contrib: 0 };
    set.add(
        "B-4303 季度统计三数",
        q.accept_permille() == Some(625)
            && q.coherent()
            && q0.accept_permille().is_none()
            && q0.coherent(),
        "提交量/收录率/Top 贡献者进运营报表——零提交返回 None 不编数",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe19 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe19_four_states_and_gap() {
        // 四态穷举覆盖（match 逐态可达）。
        let all = [SubmState::Received, SubmState::InReview, SubmState::Accepted, SubmState::Rejected];
        let mut i = 0;
        while i < all.len() {
            let s = all[i];
            match s {
                SubmState::Received | SubmState::InReview | SubmState::Accepted | SubmState::Rejected => {}
            }
            i += 1;
        }
        // 差距面：两计数任一非零即具体；双零泛泛拒。
        assert!(Gap { missing_cases: 1, data_conflicts: 0 }.specific());
        assert!(Gap { missing_cases: 0, data_conflicts: 2 }.specific());
        assert!(Gap { missing_cases: 4, data_conflicts: 5 }.specific());
        assert!(!Gap { missing_cases: 0, data_conflicts: 0 }.specific());
    }

    #[test]
    fn fe19_first_label_matrix() {
        // 标注矩阵逐格断言（状态×来源×标注的有效组合）——期望表即判据面。
        let table = [
            (SubmState::Accepted, true, true, true),   // 社区收录必标
            (SubmState::Accepted, true, false, false), // 社区收录漏标
            (SubmState::Accepted, false, true, false), // 非社区多标
            (SubmState::Accepted, false, false, true), // 非社区不标
            (SubmState::Rejected, true, true, false),  // 未收录不许标
            (SubmState::Rejected, false, false, true), // 未收录不标即可
            (SubmState::InReview, true, true, false),  // 复核中不许标
            (SubmState::Received, false, true, false), // 已收到不许标
        ];
        let mut i = 0;
        while i < table.len() {
            let (st, fc, lb, expect) = table[i];
            assert_eq!(first_label_ok(st, fc, lb), expect);
            i += 1;
        }
    }

    #[test]
    fn fe19_full_trace_hops() {
        // 恰好两跳：0/1/3/255 全拒。
        assert!(full_trace(2));
        assert!(!full_trace(0));
        assert!(!full_trace(1));
        assert!(!full_trace(3));
        assert!(!full_trace(255));
        assert_eq!(FULL_TRACE_HOPS, 2);
    }

    #[test]
    fn fe19_stats_math() {
        // 收录率先算再断（先除后乘下取整）：25/40 = 0.625 → 625。
        let q = QuarterStats { submissions: 40, accepted: 25, top_contrib: 7 };
        assert_eq!(q.accept_permille(), Some(625));
        // 1/3 = 0.333… → 333（下取整）。
        let q3 = QuarterStats { submissions: 3, accepted: 1, top_contrib: 0 };
        assert_eq!(q3.accept_permille(), Some(333));
        // 零提交 None 不编数；自洽面收录<=提交。
        let q0 = QuarterStats { submissions: 0, accepted: 0, top_contrib: 0 };
        assert_eq!(q0.accept_permille(), None);
        assert!(q0.coherent());
        let bad = QuarterStats { submissions: 2, accepted: 3, top_contrib: 0 };
        assert!(!bad.coherent());
    }
}
