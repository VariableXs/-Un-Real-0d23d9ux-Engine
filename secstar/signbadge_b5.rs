//! F178 签名状态角标 · 批次五深化（secstar · G-G-08）。
//!
//! 批次五功能面（达成率最低模块 41%——本批主攻。与 b3「渲染与路由」、
//! b4「时序与统计」互补，本批管「信任动作与交互」）：
//! - [`TrustAction`]：信任动作流——点角标 → 确认 → 写入信任表 → 角标
//!   降级灰点（用户给信任的完整交互闭环，不是只能看不能动）；
//! - [`HoverProbe`]：悬停命中检测——角标 12px 区域的点按判定
//!   （命中面：点角标外不算，点在角标上才算——命中要精确）；
//! - [`BadgeSorter`]：任务栏角标排序——按严重度稳定排序（黄盾 >
//!   灰盾 > 灰点 > 无——眼睛先看到最该警惕的）；
//! - [`TrustExpiry`]：信任过期账——授予日 + 有效期 → 到期自动除名
//!   + 时间线留痕（信任有期限：不是一劳永逸的豁免）；
//! - [`tooltip_three_parts`]：tooltip 三要素化——发生了什么/为什么/
//!   下一步（第 9 章错误三要素在角标面的落地）。
//!
//! 零堆纪律：定长表 + 定长缓冲，无 alloc。

use super::signbadge::{Badge, BADGE_SIZE_PX};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 信任动作流
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustPhase {
    /// 看到角标（未动作）。
    Seen,
    /// 点开确认面板（确认是两段——信任不因误触授予）。
    Confirming,
    /// 已写入信任表（角标随之降级）。
    Trusted,
    /// 取消（安全出路——关闭面板无副作用）。
    Cancelled,
}

/// 信任动作状态机（两段确认 + 取消安全出路——第 9 章纪律）。
pub struct TrustAction {
    pub phase: TrustPhase,
    pub app_id: u32,
}

impl TrustAction {
    pub const fn new(app_id: u32) -> TrustAction {
        TrustAction { phase: TrustPhase::Seen, app_id }
    }

    pub fn open_confirm(&mut self) -> bool {
        if self.phase == TrustPhase::Seen {
            self.phase = TrustPhase::Confirming;
            true
        } else {
            false
        }
    }

    /// 确认授予：返回写入信任表的 app_id。
    pub fn confirm(&mut self) -> Option<u32> {
        if self.phase == TrustPhase::Confirming {
            self.phase = TrustPhase::Trusted;
            Some(self.app_id)
        } else {
            None
        }
    }

    /// 取消：任何未授予阶段可退（取消永远安全）。
    pub fn cancel(&mut self) -> bool {
        if matches!(self.phase, TrustPhase::Seen | TrustPhase::Confirming) {
            self.phase = TrustPhase::Cancelled;
            true
        } else {
            false
        }
    }
}

/// 信任表写入器（与主层 BadgeResolver 语义同源：Trusted+Unsigned → 灰点）。
pub fn demote_after_trust(state: super::signbadge::SignState, was: Badge) -> Badge {
    match state {
        super::signbadge::SignState::ChainVerified => was, // 链验不受信任表影响
        _ => Badge::DotGray,                               // 未签/自签信任后 → 灰点
    }
}

// ---------------------------------------------------------------------------
// 悬停命中检测
// ---------------------------------------------------------------------------

/// 命中判定：点 (px, py) 是否落在角标 12px 方框内（角标左上角 anchor）。
pub fn hover_hit(anchor_x: i32, anchor_y: i32, px: i32, py: i32) -> bool {
    let size = BADGE_SIZE_PX as i32;
    px >= anchor_x && px < anchor_x + size && py >= anchor_y && py < anchor_y + size
}

/// 命中带边距判定：悬停热区比视觉区大 2px（小目标易点——4K 下也点得中）。
pub const HOVER_SLACK_PX: i32 = 2;

pub fn hover_hit_generous(anchor_x: i32, anchor_y: i32, px: i32, py: i32) -> bool {
    let s = HOVER_SLACK_PX;
    let size = BADGE_SIZE_PX as i32;
    px >= anchor_x - s && px < anchor_x + size + s && py >= anchor_y - s && py < anchor_y + size + s
}

// ---------------------------------------------------------------------------
// 角标严重度排序（稳定）
// ---------------------------------------------------------------------------

/// 严重度序（越大越先）：黄盾 3 > 灰盾 2 > 灰点 1 > 无 0。
pub fn severity(b: Badge) -> u8 {
    match b {
        Badge::None => 0,
        Badge::DotGray => 1,
        Badge::ShieldGray => 2,
        Badge::ShieldYellow => 3,
    }
}

/// 稳定排序：按严重度降序，同严重度保持原序（插入排序——小表够用）。
pub fn sort_by_severity(badges: &mut [Badge]) {
    for i in 1..badges.len() {
        let mut j = i;
        while j > 0 && severity(badges[j]) > severity(badges[j - 1]) {
            badges.swap(j, j - 1);
            j -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
// 信任过期账
// ---------------------------------------------------------------------------

/// 信任有效期（天——期限化：信任随时间衰减）。
pub const TRUST_TTL_DAYS: u32 = 90;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrustGrant {
    pub app_id: u32,
    pub granted_day: u32,
}

/// 到期判定与除名（返回被除名者列表——时间线留痕的数据源）。
pub struct TrustExpiry {
    grants: [Option<TrustGrant>; 16],
    pub n: usize,
}

impl TrustExpiry {
    pub const fn new() -> TrustExpiry {
        TrustExpiry { grants: [const { None }; 16], n: 0 }
    }

    pub fn grant(&mut self, app_id: u32, day: u32) -> bool {
        if self.n >= 16 || self.grants[..self.n].iter().flatten().any(|g| g.app_id == app_id) {
            return false;
        }
        self.grants[self.n] = Some(TrustGrant { app_id, granted_day: day });
        self.n += 1;
        true
    }

    /// 到期清算：返回被除名 app 列表（写入 out，返回个数）。
    pub fn expire_due(&mut self, today: u32, out: &mut [u32; 16]) -> usize {
        let mut k = 0;
        let mut i = 0;
        while i < self.n {
            let g = self.grants[i].unwrap();
            if today.saturating_sub(g.granted_day) > TRUST_TTL_DAYS {
                if k < out.len() {
                    out[k] = g.app_id;
                    k += 1;
                }
                // 压实除名。
                for j in i..self.n - 1 {
                    self.grants[j] = self.grants[j + 1];
                }
                self.grants[self.n - 1] = None;
                self.n -= 1;
            } else {
                i += 1;
            }
        }
        k
    }

    pub fn is_trusted(&self, app_id: u32) -> bool {
        self.grants[..self.n].iter().flatten().any(|g| g.app_id == app_id)
    }
}

// ---------------------------------------------------------------------------
// tooltip 三要素化
// ---------------------------------------------------------------------------

/// 三要素内容（发生了什么/为什么/下一步——按角标态给真话）。
pub fn tooltip_three_parts(badge: Badge) -> Option<(&'static str, &'static str, &'static str)> {
    match badge {
        Badge::ShieldGray => Some((
            "这个应用没有数字签名",
            "签名可验证来源，无签名无法确认发布者",
            "确认来源后可点击角标将其加入信任列表",
        )),
        Badge::ShieldYellow => Some((
            "这个应用是自签名的",
            "开发者用自己的证书签名，未经第三方链验证",
            "信任开发者后可加入信任列表",
        )),
        Badge::DotGray => Some((
            "这个应用在你信任列表里",
            "信任列表内的应用豁免签名要求（90 天有效期）",
            "到期后角标会重新出现，可再次信任",
        )),
        Badge::None => None, // 链验无角标——无 tooltip 不打扰
    }
}

// ---------------------------------------------------------------------------
// 批次五自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_signbadge_b5_checks() -> CheckSet {
    use super::signbadge::SignState as SS;
    let mut cs = CheckSet::new("F178-b5");

    // 1) 信任动作流：Seen→Confirming→Trusted 返回 app_id（闭环）。
    let mut a = TrustAction::new(42);
    let s1 = a.open_confirm();
    let granted = a.confirm();
    cs.add(
        "trust_action_flow",
        s1 && granted == Some(42) && a.phase == TrustPhase::Trusted,
        "",
    );

    // 2) 信任取消：确认面板可退、无副作用（取消是安全出路）。
    let mut a2 = TrustAction::new(7);
    a2.open_confirm();
    let cancelled = a2.cancel();
    let no_grant = a2.confirm();
    cs.add("trust_cancel_safe", cancelled && no_grant.is_none() && a2.phase == TrustPhase::Cancelled, "");

    // 3) 未开面板直接确认 = 拒（两段确认不可跳段——信任不因误触授予）。
    let mut a3 = TrustAction::new(9);
    cs.add("trust_two_step", a3.confirm().is_none(), "");

    // 4) 信任后降级：未签+信任 → 灰点；链验不受信任表影响（不越权降级）。
    cs.add(
        "demote_after_trust",
        demote_after_trust(SS::Unsigned, Badge::ShieldGray) == Badge::DotGray
            && demote_after_trust(SS::SelfSigned, Badge::ShieldYellow) == Badge::DotGray
            && demote_after_trust(SS::ChainVerified, Badge::None) == Badge::None,
        "",
    );

    // 5) 悬停命中：角标内真、角标外假、边缘逐点（12px 方框四边）。
    let inside = hover_hit(100, 50, 105, 55);
    let outside = !hover_hit(100, 50, 113, 55);
    let edge_tl = hover_hit(100, 50, 100, 50);
    let edge_br = hover_hit(100, 50, 111, 61);
    cs.add("hover_hit_bounds", inside && outside && edge_tl && edge_br, "");

    // 6) 悬停热区放宽：视觉区外 2px 内仍命中（小目标易点）。
    let slack = hover_hit_generous(100, 50, 99, 50) && hover_hit_generous(100, 50, 113, 61);
    let not_too_far = !hover_hit_generous(100, 50, 114, 50);
    cs.add("hover_slack", slack && not_too_far, "");

    // 7) 严重度排序：混排 → 黄盾在前、同严重度保序（稳定排序）。
    let mut seq = [Badge::None, Badge::ShieldYellow, Badge::DotGray, Badge::ShieldGray, Badge::ShieldYellow];
    sort_by_severity(&mut seq);
    cs.add(
        "severity_sort",
        seq[0] == Badge::ShieldYellow
            && seq[1] == Badge::ShieldYellow
            && seq[2] == Badge::ShieldGray
            && seq[3] == Badge::DotGray
            && seq[4] == Badge::None,
        "",
    );

    // 8) 信任授予：登记/查重（同应用不重复授）。
    let mut e = TrustExpiry::new();
    let ok = e.grant(3, 10);
    let dup = e.grant(3, 20);
    cs.add("trust_grant_dedup", ok && !dup && e.is_trusted(3), "");

    // 9) 信任过期：91 天除名并出列、90 天留任（TTL 线逐点）。
    let mut e2 = TrustExpiry::new();
    e2.grant(5, 0);
    let mut out = [0u32; 16];
    let not_yet = e2.expire_due(90, &mut out) == 0 && e2.is_trusted(5);
    let expired = e2.expire_due(91, &mut out) == 1 && out[0] == 5 && !e2.is_trusted(5);
    cs.add("trust_ttl_line", not_yet && expired, "");

    // 10) 过期清算压实：两条过期一条未到期 → 只除过期、未到期位前移。
    let mut e3 = TrustExpiry::new();
    e3.grant(1, 0);
    e3.grant(2, 50);
    e3.grant(3, 0);
    let mut out3 = [0u32; 16];
    let k = e3.expire_due(91, &mut out3);
    cs.add(
        "trust_expire_compact",
        k == 2 && out3[0] == 1 && out3[1] == 3 && e3.is_trusted(2) && e3.n == 1,
        "",
    );

    // 11) tooltip 三要素：三态各有三段、链验 None（无角标不打扰）。
    let g = tooltip_three_parts(Badge::ShieldGray).unwrap();
    let y = tooltip_three_parts(Badge::ShieldYellow).unwrap();
    let d = tooltip_three_parts(Badge::DotGray).unwrap();
    cs.add(
        "tooltip_three_parts",
        g.1.contains("签名") && y.1.contains("证书") && d.1.contains("90 天") && tooltip_three_parts(Badge::None).is_none(),
        "",
    );

    // 12) 主册常量贯通：角标 12px / TTL 90 天与保留期同尺。
    cs.add("consts_aligned", BADGE_SIZE_PX == 12 && TRUST_TTL_DAYS == 90, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次五）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b5 {
    use super::*;

    #[test]
    fn trust_action_full_matrix() {
        // 全路径矩阵：授予/取消/跳段 三路互不串扰。
        let mut a = TrustAction::new(1);
        a.open_confirm();
        a.cancel();
        assert_eq!(a.phase, TrustPhase::Cancelled);
        // Cancelled 后再确认仍拒（终态纪律）。
        assert!(a.confirm().is_none());
    }

    #[test]
    fn severity_sort_is_stable() {
        // 稳定性：同严重度两元素相对序不变（不洗用户看到的顺序）。
        let mut seq = [Badge::ShieldGray, Badge::ShieldYellow, Badge::ShieldYellow, Badge::ShieldGray];
        sort_by_severity(&mut seq);
        assert_eq!(seq, [Badge::ShieldYellow, Badge::ShieldYellow, Badge::ShieldGray, Badge::ShieldGray]);
    }

    #[test]
    fn expiry_multi_round() {
        // 多轮清算：TTL 滚动除名（90 天信任持续滚动续期场景）。
        let mut e = TrustExpiry::new();
        e.grant(10, 0);
        e.grant(11, 30);
        e.grant(12, 60);
        let mut out = [0u32; 16];
        // 第 91 天：10 过期；第 121 天：11 过期；第 151 天：12 过期。
        assert_eq!(e.expire_due(91, &mut out), 1);
        assert_eq!(e.expire_due(121, &mut out), 1);
        assert_eq!(e.expire_due(151, &mut out), 1);
        assert_eq!(e.n, 0);
    }
}
