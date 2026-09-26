//! F179 权限审计页（secstar · G-G-09）——权限不是设完就忘的开关，是持续可见的关系。
//!
//! 主册判据（验收标准第一句）：
//! **时间线与三源账本对拍一致；收回权限即时性实测（运行中应用请求即拒）；导出段脱敏合规。**
//!
//! 功能定义（G-G-09）：设置中心「隐私和安全性」应用权限审计：每应用的权限
//! 授予史（何时给了什么，F038 事件流）/越权尝试计数（F177 数据）/网络访问
//! 统计（F060 折算分项）/一键收回全部权限。
//!
//! 【交互设计】应用详情权限页：时间线视图（权限变更/拦截事件按日分组）+
//! 统计卡（网络量/拦截数）+「收回全部权限」红钮（二次确认+说明后果=该应用
//! 需重新请求）；导出该应用审计段。
//! 【数据与存储】审计数据引用 F177/F060/F038 各账本（一处一事实不复制）；
//! 时间线渲染现算。
//! 【状态与异常】审计数据部分缺失（总闸关过）→ 时间线标注空窗（不伪造
//! 连续）；应用卸载 → 审计段归档 90 天后清；收回权限即时生效（运行中应用
//! 下次请求时拒）。
//! 【设计细节】「权限授予史」=F038 权限卡确认事件流（显式授权才记——暗
//! 授权不存在）；统计卡数字三源求和口径文档化；红钮文案模板：
//! 「收回后，{应用} 的所有能力请求将被拒绝并记录」——后果前置；时间线空窗
//! 黄条：「{时段} 因隐私设置未记录」。
//!
//! 三源接入纪律（跨分队接缝显式参数注入口——同 secstar2 先例）：F038 授予
//! 事件、F177 拦截事件、F060 网络分项由调用方注入本层；本层只做时间线
//! 合成、口径求和与执法状态机，不复制任何账本本体（一处一事实）。
//!
//! 零堆纪律：定长事件表 + 定长时间线槽，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 审计段归档保留 90 天（应用卸载后）。
pub const ARCHIVE_RETENTION_DAYS: u32 = 90;
/// 时间线按日分组的槽位天数（渲染现算窗口：90 天覆盖归档全周期）。
pub const TIMELINE_DAYS: usize = 90;
/// 「收回全部权限」红钮确认步数：两步（按下→确认）。
pub const REVOKE_CONFIRM_STEPS: u32 = 2;
/// 统计卡网络量折算单位：MB（F060 分项口径）。
pub const NETWORK_UNIT_MB: u64 = 1_048_576;

// ---------------------------------------------------------------------------
// 三源事件（注入面——账本本体在 F038/F177/F060，本层不复制）
// ---------------------------------------------------------------------------

/// 事件源标签（三源求和口径的文档化锚点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// F038 权限卡确认事件流（显式授权才记）。
    Grant,
    /// F177 能力执法拦截事件。
    Intercept,
    /// F060 网络折算分项（累计 MB 记在 value）。
    Network,
}

impl Source {
    /// 三源标签的稳定序号（口径文档化：统计卡数字=本序号三维求和）。
    pub fn ord(self) -> u8 {
        match self {
            Source::Grant => 0,
            Source::Intercept => 1,
            Source::Network => 2,
        }
    }
}

/// 一条注入的审计事件（天粒度时间戳=自纪元的日序数；value 语义随源）。
#[derive(Clone, Copy, Debug)]
pub struct AuditEvent {
    pub source: Source,
    /// 日序数（day index）。
    pub day: u32,
    /// Grant=能力枚举值；Intercept=执法点枚举值；Network=累计字节数。
    pub value: u64,
    /// 敏感载荷引用（脱敏导出时剥除——注入测试锚）。
    pub sensitive: bool,
}

/// 单应用审计段（卸载后进入归档期，90 天后清）。
pub struct AppAudit {
    pub app_id: u32,
    events: [Option<AuditEvent>; 256],
    event_n: usize,
    /// 应用已卸载（审计段归档中）。
    pub uninstalled: bool,
    /// 卸载日（日序数；归档倒计时起点）。
    pub uninstalled_day: u32,
    /// 总闸关闭过的日区间登记（空窗黄条依据——不伪造连续）。
    privacy_gaps: [(u32, u32); 8],
    gap_n: usize,
}

impl AppAudit {
    pub const fn new(app_id: u32) -> Self {
        AppAudit {
            app_id,
            events: [const { None }; 256],
            event_n: 0,
            uninstalled: false,
            uninstalled_day: 0,
            privacy_gaps: [(0, 0); 8],
            gap_n: 0,
        }
    }

    /// 注入一条三源事件（表满诚实拒绝并返回 false——不静默丢）。
    pub fn record(&mut self, ev: AuditEvent) -> bool {
        if self.event_n >= 256 {
            return false;
        }
        self.events[self.event_n] = Some(ev);
        self.event_n += 1;
        true
    }

    /// 登记一段「隐私总闸关闭」区间（时间线空窗黄条依据）。
    pub fn mark_privacy_gap(&mut self, from_day: u32, to_day: u32) {
        if self.gap_n < 8 && from_day <= to_day {
            self.privacy_gaps[self.gap_n] = (from_day, to_day);
            self.gap_n += 1;
        }
    }

    /// 某日是否处于登记的空窗区间内。
    pub fn in_privacy_gap(&self, day: u32) -> bool {
        self.privacy_gaps[..self.gap_n]
            .iter()
            .any(|(a, b)| day >= *a && day <= *b)
    }

    /// 统计卡三源求和（口径文档化：按源枚举序号三维相加）。
    /// Network 源按 MB 折算（F060 分项口径），其余按条数。
    pub fn stat_cards(&self) -> (u64, u64, u64) {
        let mut grants = 0u64;
        let mut intercepts = 0u64;
        let mut net_mb = 0u64;
        for ev in self.events[..self.event_n].iter().flatten() {
            match ev.source {
                Source::Grant => grants += 1,
                Source::Intercept => intercepts += 1,
                Source::Network => net_mb += ev.value / NETWORK_UNIT_MB,
            }
        }
        (grants, intercepts, net_mb)
    }

    /// 卸载登记（进入 90 天归档期）。
    pub fn mark_uninstalled(&mut self, day: u32) {
        self.uninstalled = true;
        self.uninstalled_day = day;
    }

    /// 归档到期判定：卸载满 90 天该清（返回 true 表示应清除）。
    pub fn archive_expired(&self, today: u32) -> bool {
        self.uninstalled && today.saturating_sub(self.uninstalled_day) >= ARCHIVE_RETENTION_DAYS as u32
    }
}

// ---------------------------------------------------------------------------
// 时间线合成（渲染现算——按日分组；空窗黄条；不伪造连续）
// ---------------------------------------------------------------------------

/// 时间线单行（一日聚合视图）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DayRow {
    pub day: u32,
    pub grants: u32,
    pub intercepts: u32,
    /// 该日处于隐私空窗（黄条行——数据缺失，如实标注）。
    pub gap: bool,
}

/// 合成时间线：近 `days` 天逐日出行（含空窗行——不伪造连续）。
pub fn synthesize_timeline(audit: &AppAudit, today: u32, days: usize) -> [Option<DayRow>; TIMELINE_DAYS] {
    let mut rows: [Option<DayRow>; TIMELINE_DAYS] = [const { None }; TIMELINE_DAYS];
    let n = days.min(TIMELINE_DAYS);
    let start = today.saturating_sub(n as u32 - 1);
    for i in 0..n {
        let day = start + i as u32;
        let mut row = DayRow { day, grants: 0, intercepts: 0, gap: audit.in_privacy_gap(day) };
        for ev in audit.events[..audit.event_n].iter().flatten() {
            if ev.day == day {
                match ev.source {
                    Source::Grant => row.grants += 1,
                    Source::Intercept => row.intercepts += 1,
                    Source::Network => {}
                }
            }
        }
        rows[i] = Some(row);
    }
    rows
}

// ---------------------------------------------------------------------------
// 收回全部权限（两步确认红钮 + 即时执法 + 审计留痕）
// ---------------------------------------------------------------------------

/// 红钮状态机：Idle → Confirming（第一步按下）→ Armed（第二步确认）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RevokePhase {
    Idle,
    Confirming,
    Armed,
}

/// 收回执法闸：置位后该应用的一切能力请求即拒（运行中应用下次请求即拒）。
pub struct RevokeGate {
    pub app_id: u32,
    phase: RevokePhase,
    pub revoked: bool,
    /// 收回动作审计留痕（日序数）。
    pub revoked_day: u32,
    /// 收回后被拒的请求计数（即时性对账锚）。
    pub denied_since_revoke: u64,
}

impl RevokeGate {
    pub const fn new(app_id: u32) -> Self {
        RevokeGate { app_id, phase: RevokePhase::Idle, revoked: false, revoked_day: 0, denied_since_revoke: 0 }
    }

    /// 红钮文案模板（后果前置——主册逐字）。
    /// 调用方以应用名填充 `{应用}` 槽位；本层返回固定骨架两段。
    pub fn confirm_copy() -> (&'static str, &'static str) {
        ("收回后，{应用} 的所有能力请求将被拒绝并记录", "该应用需重新请求全部权限")
    }

    /// 红钮推进一步；两步全满才真正收回。
    pub fn press(&mut self, day: u32) -> RevokePhase {
        match self.phase {
            RevokePhase::Idle => self.phase = RevokePhase::Confirming,
            RevokePhase::Confirming => {
                self.phase = RevokePhase::Armed;
                self.revoked = true;
                self.revoked_day = day;
            }
            RevokePhase::Armed => {}
        }
        self.phase
    }

    /// Esc/取消回退（「取消」永远是安全出路）。
    pub fn cancel(&mut self) {
        if self.phase == RevokePhase::Confirming {
            self.phase = RevokePhase::Idle;
        }
    }

    /// 能力请求裁决：收回后一律拒绝（运行中应用下次请求即拒——即时性）。
    pub fn adjudicate(&mut self) -> bool {
        if self.revoked {
            self.denied_since_revoke += 1;
            false
        } else {
            true
        }
    }
}

// ---------------------------------------------------------------------------
// 导出脱敏（F120 三查同源：路径/用户名/序列号零残留）
// ---------------------------------------------------------------------------

/// 导出帧上限（定长——脱敏后写入）。
pub const EXPORT_FRAME_CAP: usize = 512;

/// 审计段导出：敏感事件只保留「源+日+计数」骨架，敏感载荷剥除；
/// 返回写入长度（表满截断即缺陷——断言口径见单测）。
pub fn export_sanitized(audit: &AppAudit, out: &mut [u8; EXPORT_FRAME_CAP]) -> usize {
    // 导出骨架行：`<源标签><日>:<值>`（敏感事件值置 0——载荷不随导出）。
    let mut pos = 0usize;
    for ev in audit.events[..audit.event_n].iter().flatten() {
        let tag: &[u8] = match ev.source {
            Source::Grant => b"G",
            Source::Intercept => b"I",
            Source::Network => b"N",
        };
        // 每行最长 32 字节：tag(1)+':'(1)+day(≤10)+':'(1)+value(≤20)+';'(1)。
        if pos + 33 > out.len() {
            break;
        }
        out[pos..pos + tag.len()].copy_from_slice(tag);
        pos += tag.len();
        pos += write_u32(out, &mut pos, ev.day);
        out[pos] = b':';
        pos += 1;
        let val = if ev.sensitive { 0 } else { ev.value };
        pos += write_u64(out, &mut pos, val);
        out[pos] = b';';
        pos += 1;
    }
    pos
}

fn write_u32(out: &mut [u8], pos: &mut usize, v: u32) -> usize {
    // 写入从 *pos 起的十进制串，返回长度（不自增 pos——调用方统一推进）。
    let mut buf = [0u8; 10];
    let mut l = 0;
    let mut x = v;
    if x == 0 {
        buf[0] = b'0';
        l = 1;
    }
    while x > 0 {
        buf[l] = b'0' + (x % 10) as u8;
        l += 1;
        x /= 10;
    }
    for i in 0..l {
        out[*pos + i] = buf[l - 1 - i];
    }
    l
}

fn write_u64(out: &mut [u8], pos: &mut usize, v: u64) -> usize {
    let mut buf = [0u8; 20];
    let mut l = 0;
    let mut x = v;
    if x == 0 {
        buf[0] = b'0';
        l = 1;
    }
    while x > 0 {
        buf[l] = b'0' + (x % 10) as u8;
        l += 1;
        x /= 10;
    }
    for i in 0..l {
        out[*pos + i] = buf[l - 1 - i];
    }
    l
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
#[inline(never)]
pub fn run_permaudit_checks() -> CheckSet {
    let mut cs = CheckSet::new("F179-permaudit");

    // 1) 三源注入 → 统计卡三源求和口径一致（Grant 条数/Intercept 条数/Network MB 折算）。
    let mut a = AppAudit::new(1);
    a.record(AuditEvent { source: Source::Grant, day: 10, value: 3, sensitive: false });
    a.record(AuditEvent { source: Source::Grant, day: 11, value: 5, sensitive: false });
    a.record(AuditEvent { source: Source::Intercept, day: 12, value: 1, sensitive: false });
    a.record(AuditEvent { source: Source::Network, day: 12, value: 40 * NETWORK_UNIT_MB, sensitive: false });
    let (g, i, n) = a.stat_cards();
    cs.add("three_source_stat_cards", g == 2 && i == 1 && n == 40, "");

    // 2) 时间线合成：按日分组计数正确（Grant/Intercept 落日对位）。
    let rows = synthesize_timeline(&a, 12, 5);
    let d10 = rows[2].unwrap();
    let d12 = rows[4].unwrap();
    cs.add(
        "timeline_day_grouping",
        d10.day == 10 && d10.grants == 1 && d12.day == 12 && d12.intercepts == 1,
        "",
    );

    // 3) 空窗黄条：登记隐私空窗后该日出 gap 行（不伪造连续）。
    let mut b = AppAudit::new(2);
    b.record(AuditEvent { source: Source::Grant, day: 20, value: 1, sensitive: false });
    b.mark_privacy_gap(21, 23);
    let rows_b = synthesize_timeline(&b, 23, 5);
    let gap_row = rows_b[4].unwrap();
    cs.add("privacy_gap_day_row", gap_row.day == 23 && gap_row.gap && rows_b[3].unwrap().gap, "");

    // 4) 收回权限即时性：两步确认后，运行中应用下次请求即拒且计数留痕。
    let mut gate = RevokeGate::new(7);
    let before = gate.adjudicate();
    gate.press(30);
    let mid = gate.phase == RevokePhase::Confirming && gate.adjudicate(); // 未收回仍放行
    gate.press(30);
    let after = !gate.adjudicate() && !gate.adjudicate() && gate.denied_since_revoke == 2;
    cs.add("revoke_immediate_deny", before && mid && after, "");

    // 5) 红钮取消回退：Confirming 态取消 → Idle，不误收回。
    let mut gate2 = RevokeGate::new(8);
    gate2.press(31);
    gate2.cancel();
    cs.add(
        "revoke_cancel_safe",
        gate2.phase == RevokePhase::Idle && !gate2.revoked && gate2.adjudicate(),
        "",
    );

    // 6) 红钮文案模板（后果前置——主册逐字骨架）。
    let (l1, l2) = RevokeGate::confirm_copy();
    cs.add(
        "revoke_copy_template",
        l1.contains("所有能力请求将被拒绝并记录") && l2.contains("重新请求"),
        "",
    );

    // 7) 卸载归档 90 天：未满不清、满 90 天应清。
    let mut c = AppAudit::new(3);
    c.mark_uninstalled(100);
    cs.add(
        "archive_90d_retention",
        !c.archive_expired(100 + 89) && c.archive_expired(100 + 90),
        "",
    );

    // 8) 导出脱敏：敏感事件值归零、非敏感保真（F120 三查注入锚）。
    let mut d = AppAudit::new(4);
    d.record(AuditEvent { source: Source::Grant, day: 40, value: 777, sensitive: true });
    d.record(AuditEvent { source: Source::Grant, day: 41, value: 42, sensitive: false });
    let mut frame = [0u8; EXPORT_FRAME_CAP];
    let len = export_sanitized(&d, &mut frame);
    let text = core::str::from_utf8(&frame[..len]).unwrap_or("");
    cs.add(
        "export_sanitized",
        text.contains("G40:0;") && text.contains("G41:42;") && !text.contains("777"),
        "",
    );

    // 9) 导出帧恒不越界（512 上限内截断安全——溢出即缺陷）。
    let mut e = AppAudit::new(5);
    for day in 0..64u32 {
        let _ = e.record(AuditEvent { source: Source::Intercept, day, value: day as u64, sensitive: false });
    }
    let mut frame2 = [0u8; EXPORT_FRAME_CAP];
    let len2 = export_sanitized(&e, &mut frame2);
    cs.add("export_frame_bounded", len2 <= EXPORT_FRAME_CAP, "");

    // 10) 事件表满诚实拒绝（不静默丢——返回 false）。
    let mut f = AppAudit::new(6);
    let mut all_recorded = true;
    for k in 0..257u32 {
        if !f.record(AuditEvent { source: Source::Grant, day: k, value: 1, sensitive: false }) {
            all_recorded = k == 256;
        }
    }
    cs.add("event_table_full_honest_reject", all_recorded && f.event_n == 256, "");

    // 11) 常量对账（90 天/两步确认/MB 单位——一处一事实）。
    cs.add(
        "constants_reconciled",
        ARCHIVE_RETENTION_DAYS == 90 && REVOKE_CONFIRM_STEPS == 2 && NETWORK_UNIT_MB == 1_048_576,
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_synthesis_full_window() {
        // 90 天全窗口合成不越界、逐日行齐（渲染现算口径）。
        let mut a = AppAudit::new(11);
        for day in 0..90u32 {
            let _ = a.record(AuditEvent { source: Source::Intercept, day, value: 1, sensitive: false });
        }
        let rows = synthesize_timeline(&a, 89, 90);
        for (i, row) in rows.iter().enumerate() {
            let r = row.unwrap();
            assert_eq!(r.day, i as u32);
            assert_eq!(r.intercepts, 1);
        }
    }

    #[test]
    fn revoke_gate_state_machine() {
        // Idle→Confirming→Armed 全程；重复按 Armed 不再推进。
        let mut g = RevokeGate::new(21);
        assert_eq!(g.press(1), RevokePhase::Confirming);
        assert_eq!(g.press(1), RevokePhase::Armed);
        assert_eq!(g.press(1), RevokePhase::Armed);
        assert!(g.revoked && g.revoked_day == 1);
    }

    #[test]
    fn network_unit_boundary() {
        // 恰好 1MB 折算为 1；不足 1MB 折算为 0（下取整口径文档化）。
        let mut a = AppAudit::new(31);
        let _ = a.record(AuditEvent { source: Source::Network, day: 1, value: NETWORK_UNIT_MB, sensitive: false });
        let _ = a.record(AuditEvent { source: Source::Network, day: 1, value: NETWORK_UNIT_MB - 1, sensitive: false });
        assert_eq!(a.stat_cards().2, 1);
    }

    #[test]
    fn privacy_gap_boundaries() {
        // 空窗区间端点含边（from/to 都算空窗——黄条不吞边界日）。
        let mut a = AppAudit::new(41);
        a.mark_privacy_gap(5, 7);
        assert!(!a.in_privacy_gap(4) && a.in_privacy_gap(5) && a.in_privacy_gap(7) && !a.in_privacy_gap(8));
    }

    #[test]
    fn export_is_valid_utf8_always() {
        // 任意事件组合导出恒为合法 UTF-8（帧内纯 ASCII——脱敏骨架纪律）。
        let mut a = AppAudit::new(51);
        for day in 0..100u32 {
            let _ = a.record(AuditEvent { source: Source::Network, day, value: u64::MAX, sensitive: day % 3 == 0 });
        }
        let mut frame = [0u8; EXPORT_FRAME_CAP];
        let len = export_sanitized(&a, &mut frame);
        assert!(core::str::from_utf8(&frame[..len]).is_ok());
    }
}
