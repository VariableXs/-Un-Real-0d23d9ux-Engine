//! F177 能力执法可视化（secstar · G-G-07）——看得见的守护才是守护。
//!
//! 主册判据（验收标准第一句）：
//! **四执法点各注入拦截样本通知准确；聚合逻辑正确；总闸全效。**
//!
//! 功能定义（G-G-07）：四个执法点（B-15xx）拦截事件进通知中心（可关）：
//! 每条「XX 应用尝试 YY（已按规则处理）」+规则名链接（F037 透明化同族）
//! ——安全系统的工作让用户看得见。
//!
//! 【交互设计】通知样式低存在感：无声音、低优先级（F077 只进历史档默认）、
//! 文案三要素；聚合同应用 5 分钟窗口为一条计数通知；设置页总开关+逐类开关
//! （四执法点独立）。
//! 【数据与存储】拦截事件审计日志（F194 序号链）；通知为视图非数据本体。
//! 【状态与异常】拦截风暴（>50 次/分钟）→ 静默聚合为一条+建议隔离档
//! （F038 升档引导）；执法点自身异常 → 诊断报备（执法失效是 P0 事件——
//! F142 安全通道评估）。
//! 【设计细节】通知文案模板：「{应用} 尝试 {能力}，已按「{规则名}」处理」
//! ——三要素齐且无恐吓词；执法点标签枚举固定（文件越界/网络越权/设备直访/
//! 特权调用）；灰字色=次要令牌（F151）——存在但不施压；关闭总闸时执法照常
//! （只是不说——执法与播报解耦）。
//!
//! 零堆纪律：定长事件环 + 定长通知视图，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 聚合窗口 5 分钟（同应用同点合并为一条计数通知）。
pub const AGGREGATE_WINDOW_MS: u64 = 5 * 60_000;
/// 拦截风暴阈值：>50 次/分钟 → 静默聚合+升档建议。
pub const STORM_PER_MIN: u64 = 50;
/// 风暴判定窗 60s。
pub const STORM_WINDOW_MS: u64 = 60_000;
/// 通知视图容量。
pub const NOTIF_CAP: usize = 32;
/// 事件环容量（审计日志 F194 的视图源；本体在 F194 序号链）。
pub const EVENT_CAP: usize = 256;
/// 审计序号链起点（F194 消费——拦截事件全部入链）。
pub const AUDIT_SEQ_START: u64 = 1;
/// 文案缓冲容量。
pub const TEXT_CAP: usize = 96;

// ---------------------------------------------------------------------------
// 执法点
// ---------------------------------------------------------------------------

/// 四执法点（标签枚举固定——主册设计细节）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnforcePoint {
    /// 文件越界。
    FileBoundary = 0,
    /// 网络越权。
    NetUnauthorized = 1,
    /// 设备直访。
    DeviceDirect = 2,
    /// 特权调用。
    PrivilegedCall = 3,
}

pub const POINT_N: usize = 4;

impl EnforcePoint {
    pub fn label(self) -> &'static str {
        match self {
            EnforcePoint::FileBoundary => "文件越界",
            EnforcePoint::NetUnauthorized => "网络越权",
            EnforcePoint::DeviceDirect => "设备直访",
            EnforcePoint::PrivilegedCall => "特权调用",
        }
    }
}

/// 拦截事件（视图源；审计本体入 F194 序号链）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InterceptEvent {
    pub seq: u64,
    pub ms: u64,
    pub app_id: u32,
    pub point: EnforcePoint,
    /// 规则名编号（规则名链接 → F037 透明化面）。
    pub rule_id: u16,
}

/// 聚合通知（视图非数据本体——通知中心 F077 低存在感档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AggNotice {
    pub app_id: u32,
    pub point: EnforcePoint,
    /// 窗口内计数。
    pub count: u32,
    /// 首次拦截时刻（窗口锚）。
    pub window_start_ms: u64,
    /// 风暴标记（>50/分钟 → 附隔离档建议）。
    pub storm: bool,
}

/// 拦截处理结果（执法与播报解耦——Blocked 恒定，通知与开关有关）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InterceptResult {
    /// 恒 true——执法永不被开关关闭。
    pub blocked: bool,
    /// 本次拦截是否入审计序号链（恒 true——审计不随开关）。
    pub audited: bool,
    /// 审计序号。
    pub seq: u64,
    /// 是否产生了通知（开关与聚合共同决定）。
    pub notified: bool,
}

// ---------------------------------------------------------------------------
// 执法器
// ---------------------------------------------------------------------------

/// 能力执法可视化器。
pub struct CapEnforce {
    /// 总开关（关 = 执法照常、播报停止——解耦是设计不是巧合）。
    pub master_on: bool,
    /// 逐点开关（四点独立）。
    pub point_on: [bool; POINT_N],
    /// 事件环。
    pub events: [Option<InterceptEvent>; EVENT_CAP],
    pub ev_head: usize,
    pub ev_n: usize,
    seq: u64,
    /// 聚合通知视图（5min 窗 / 应用×点聚合）。
    pub notices: [Option<AggNotice>; NOTIF_CAP],
    pub notice_n: usize,
    /// 执法点自身异常旗（P0 —— F142 通道评估）。
    pub point_fault: [bool; POINT_N],
    pub diag_p0_reported: bool,
    /// 风暴锁存（进入风暴态后聚合为一条+建议）。
    pub storm_latched: bool,
}

impl CapEnforce {
    pub const fn new() -> Self {
        CapEnforce {
            master_on: true,
            point_on: [true; POINT_N],
            events: [const { None }; EVENT_CAP],
            ev_head: 0,
            ev_n: 0,
            seq: AUDIT_SEQ_START - 1,
            notices: [const { None }; NOTIF_CAP],
            notice_n: 0,
            point_fault: [false; POINT_N],
            diag_p0_reported: false,
            storm_latched: false,
        }
    }

    /// 拦截主入口：执法恒定生效，播报按开关与聚合规则。
    pub fn intercept(&mut self, app_id: u32, point: EnforcePoint, rule_id: u16, ms: u64) -> InterceptResult {
        // 审计先行（F194 序号链——执法与播报解耦的账目侧）。
        self.seq += 1;
        self.push_event(InterceptEvent { seq: self.seq, ms, app_id, point, rule_id });
        // 风暴判定（>50 次/分钟）。
        if self.rate_last_min(ms) > STORM_PER_MIN {
            self.storm_latched = true;
        }
        // 播报判定：总闸 + 逐点开关。
        let pi = point as usize;
        let notify = self.master_on && self.point_on[pi];
        if notify {
            self.aggregate(app_id, point, ms);
        }
        InterceptResult { blocked: true, audited: true, seq: self.seq, notified: notify }
    }

    /// 事件率（最近 1 分钟窗口内计数）。
    pub fn rate_last_min(&self, now_ms: u64) -> u64 {
        self.events
            .iter()
            .flatten()
            .filter(|e| now_ms.saturating_sub(e.ms) <= STORM_WINDOW_MS)
            .count() as u64
    }

    /// 聚合：同应用同点 5min 窗口一条计数通知。
    fn aggregate(&mut self, app_id: u32, point: EnforcePoint, ms: u64) {
        for slot in self.notices[..self.notice_n].iter_mut() {
            if let Some(n) = slot {
                if n.app_id == app_id && n.point == point && ms.saturating_sub(n.window_start_ms) <= AGGREGATE_WINDOW_MS {
                    n.count += 1;
                    n.storm = self.storm_latched;
                    return;
                }
            }
        }
        if self.notice_n < NOTIF_CAP {
            self.notices[self.notice_n] = Some(AggNotice { app_id, point, count: 1, window_start_ms: ms, storm: self.storm_latched });
            self.notice_n += 1;
        }
    }

    /// 通知文案（模板三要素齐且无恐吓词；填入定长缓冲）。
    /// 返回长度。模板：「{应用} 尝试 {能力}，已按「{规则名}」处理」。
    pub fn notice_text(&self, n: &AggNotice, app_name: &[u8], rule_name: &[u8], out: &mut [u8; TEXT_CAP]) -> usize {
        let mut l = 0;
        let push = |b: &[u8], out: &mut [u8; TEXT_CAP], l: &mut usize| {
            for byte in b {
                if *l < TEXT_CAP {
                    out[*l] = *byte;
                    *l += 1;
                }
            }
        };
        push(app_name, out, &mut l);
        // 中文字节走 str.as_bytes()（byte-string 字面量限 ASCII）。
        push(" 尝试 ".as_bytes(), out, &mut l);
        push(n.point.label().as_bytes(), out, &mut l);
        if n.count > 1 {
            // 计数后缀（聚合计数通知——风暴合并的诚实口径）。
            push("×".as_bytes(), out, &mut l);
            let mut buf = [0u8; 12];
            let mut m = 0;
            let mut c = n.count;
            if c == 0 {
                buf[0] = b'0';
                m = 1;
            }
            while c > 0 {
                buf[m] = b'0' + (c % 10) as u8;
                m += 1;
                c /= 10;
            }
            let mut i = m;
            while i > 0 {
                i -= 1;
                push(&buf[i..i + 1], out, &mut l);
            }
        }
        push("，已按「".as_bytes(), out, &mut l);
        push(rule_name, out, &mut l);
        push("」处理".as_bytes(), out, &mut l);
        if n.storm {
            push("（高频拦截，建议调高隔离档）".as_bytes(), out, &mut l);
        }
        l
    }

    /// 执法点自身异常（执法失效是 P0 —— F142 安全通道评估）。
    pub fn mark_point_fault(&mut self, point: EnforcePoint) {
        let pi = point as usize;
        if !self.point_fault[pi] {
            self.point_fault[pi] = true;
            self.diag_p0_reported = true;
        }
    }

    /// 执法点恢复（修复后解除 P0 态——报备留痕不撤销）。
    pub fn clear_point_fault(&mut self, point: EnforcePoint) {
        self.point_fault[point as usize] = false;
    }

    fn push_event(&mut self, ev: InterceptEvent) {
        if self.ev_n < EVENT_CAP {
            self.events[(self.ev_head + self.ev_n) % EVENT_CAP] = Some(ev);
            self.ev_n += 1;
        } else {
            self.events[self.ev_head] = Some(ev);
            self.ev_head = (self.ev_head + 1) % EVENT_CAP;
        }
    }

    /// 审计序号单调性核对（F194 链头）。
    pub fn audit_seq(&self) -> u64 {
        self.seq
    }

    /// 通知视图重算（诊断中心/通知中心共用——视图非数据本体）。
    pub fn rebuild_notices(&mut self, now_ms: u64) {
        // 事件快照（定长栈拷贝——解除 self.events 不可变借用后再聚合）。
        let mut snap: [(u32, EnforcePoint, u64); EVENT_CAP] = [(0, EnforcePoint::FileBoundary, 0); EVENT_CAP];
        let mut snap_n = 0usize;
        for e in self.events.iter().flatten() {
            if now_ms.saturating_sub(e.ms) > AGGREGATE_WINDOW_MS {
                continue;
            }
            if self.master_on && self.point_on[e.point as usize] && snap_n < EVENT_CAP {
                snap[snap_n] = (e.app_id, e.point, e.ms);
                snap_n += 1;
            }
        }
        self.notices = [const { None }; NOTIF_CAP];
        self.notice_n = 0;
        for (app_id, point, ms) in snap[..snap_n].iter() {
            self.aggregate(*app_id, *point, *ms);
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
#[inline(never)]
pub fn run_capenforce_checks() -> CheckSet {
    let mut cs = CheckSet::new("F177-capenforce");

    // 1) 四执法点各注入拦截样本：通知准确（点标签正确落通知）。
    let mut ce = CapEnforce::new();
    let points = [EnforcePoint::FileBoundary, EnforcePoint::NetUnauthorized, EnforcePoint::DeviceDirect, EnforcePoint::PrivilegedCall];
    let mut all_notified = true;
    for (i, p) in points.iter().enumerate() {
        let r = ce.intercept(100 + i as u32, *p, 10 + i as u16, 1_000 + i as u64);
        all_notified &= r.blocked && r.notified && r.audited;
    }
    cs.add("four_points_notified", all_notified && ce.notice_n == 4, "");

    // 2) 聚合逻辑：同应用同点 5min 窗内 4 次拦截 → 1 条计数 4。
    let mut ce2 = CapEnforce::new();
    for i in 0..4 {
        ce2.intercept(7, EnforcePoint::FileBoundary, 1, 10_000 + i as u64 * 30_000);
    }
    let agg_ok = ce2.notice_n == 1 && ce2.notices[0].map(|n| n.count == 4).unwrap_or(false);
    cs.add("aggregate_5min_window", agg_ok, "");

    // 3) 不同应用/不同点不合并（4 拦截 → 4 条）。
    let mut ce3 = CapEnforce::new();
    ce3.intercept(1, EnforcePoint::FileBoundary, 1, 1_000);
    ce3.intercept(2, EnforcePoint::FileBoundary, 1, 1_000);
    ce3.intercept(1, EnforcePoint::NetUnauthorized, 1, 1_000);
    cs.add("no_cross_merge", ce3.notice_n == 3, "");

    // 4) 总闸全效：关总闸 → 执法照常（blocked+审计），通知停止。
    let mut ce4 = CapEnforce::new();
    ce4.master_on = false;
    let r = ce4.intercept(9, EnforcePoint::DeviceDirect, 3, 2_000);
    cs.add("master_off_enforce_continues", r.blocked && r.audited && !r.notified && ce4.notice_n == 0, "");

    // 5) 逐点开关：关单点 → 该点静默，他点照播。
    let mut ce5 = CapEnforce::new();
    ce5.point_on[EnforcePoint::PrivilegedCall as usize] = false;
    let r1 = ce5.intercept(5, EnforcePoint::PrivilegedCall, 1, 1_000);
    let r2 = ce5.intercept(5, EnforcePoint::FileBoundary, 1, 1_100);
    cs.add("per_point_switch", !r1.notified && r2.notified, "");

    // 6) 拦截风暴 >50/分钟 → 静默聚合 + 建议隔离档。
    let mut ce6 = CapEnforce::new();
    for i in 0..60 {
        ce6.intercept(77, EnforcePoint::NetUnauthorized, 2, 5_000 + i as u64 * 500);
    }
    let storm_note = ce6.notices[..ce6.notice_n].iter().flatten().find(|n| n.app_id == 77);
    cs.add("storm_aggregates_with_suggestion", ce6.storm_latched && storm_note.map(|n| n.storm && n.count >= 50).unwrap_or(false), "");

    // 7) 执法点异常 → P0 诊断报备（F142 通道）。
    let mut ce7 = CapEnforce::new();
    ce7.mark_point_fault(EnforcePoint::DeviceDirect);
    cs.add("point_fault_p0_reported", ce7.point_fault[2] && ce7.diag_p0_reported, "");

    // 8) 审计序号单调链（F194 消费——逐条递增无空洞）。
    let mut ce8 = CapEnforce::new();
    for i in 0..10 {
        ce8.intercept(3, EnforcePoint::FileBoundary, 1, i as u64 * 100);
    }
    // 定长扫描（零堆）：逐对相邻比较，序号必须逐条 +1 且起点为 AUDIT_SEQ_START。
    let mut prev: Option<u64> = None;
    let mut monotonic = true;
    let mut counted = 0usize;
    for e in ce8.events.iter().flatten() {
        if let Some(p) = prev {
            if e.seq != p + 1 {
                monotonic = false;
            }
        } else if e.seq != AUDIT_SEQ_START {
            monotonic = false;
        }
        prev = Some(e.seq);
        counted += 1;
    }
    cs.add("audit_seq_monotonic", monotonic && counted == 10 && ce8.audit_seq() == 10, "");

    // 9) 文案模板三要素（应用/能力/规则）+ 计数后缀。
    let mut ce9 = CapEnforce::new();
    for i in 0..3 {
        ce9.intercept(42, EnforcePoint::FileBoundary, 8, i as u64 * 1000);
    }
    let mut buf = [0u8; TEXT_CAP];
    let n = ce9.notices[0].unwrap();
    let len = ce9.notice_text(&n, "老编辑器".as_bytes(), "沙盒外文件拒读".as_bytes(), &mut buf);
    let text = core::str::from_utf8(&buf[..len]).unwrap_or("");
    cs.add(
        "notice_text_template",
        text.contains("老编辑器") && text.contains("文件越界") && text.contains("沙盒外文件拒读") && text.contains("×3") && !text.contains("!!"),
        "",
    );

    // 10) 视图重算与账本一致（通知为视图非数据本体——重建后计数不变）。
    ce9.rebuild_notices(3_000);
    cs.add("notice_is_view_rebuildable", ce9.notice_n == 1 && ce9.notices[0].map(|n| n.count == 3).unwrap_or(false), "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforcement_never_disabled_by_switches() {
        // 执法与播报解耦的完整矩阵：总闸×逐点 4 态，blocked+audited 恒真。
        for master in [true, false] {
            for pi in 0..POINT_N {
                let mut ce = CapEnforce::new();
                ce.master_on = master;
                ce.point_on[pi] = false;
                let r = ce.intercept(1, EnforcePoint::FileBoundary, 1, 0);
                assert!(r.blocked, "执法永不关");
                assert!(r.audited, "审计永不关");
                assert_eq!(r.notified, master && pi != EnforcePoint::FileBoundary as usize);
            }
        }
    }

    #[test]
    fn aggregate_window_boundary() {
        // 聚合窗边界：恰 5min 内合并、超 5min 新开一条。
        let mut ce = CapEnforce::new();
        ce.intercept(1, EnforcePoint::FileBoundary, 1, 0);
        ce.intercept(1, EnforcePoint::FileBoundary, 1, AGGREGATE_WINDOW_MS);
        assert_eq!(ce.notice_n, 1, "恰 5min 窗内合并");
        let mut ce2 = CapEnforce::new();
        ce2.intercept(1, EnforcePoint::FileBoundary, 1, 0);
        ce2.intercept(1, EnforcePoint::FileBoundary, 1, AGGREGATE_WINDOW_MS + 1);
        assert_eq!(ce2.notice_n, 2, "超窗新开");
    }

    #[test]
    fn storm_boundary_50_per_min() {
        // 风暴边界：恰 50 次/分不触发；51 次触发。
        let mut ce = CapEnforce::new();
        for i in 0..50 {
            ce.intercept(1, EnforcePoint::FileBoundary, 1, i * 1_000);
        }
        assert!(!ce.storm_latched, "恰 50/分不触发");
        ce.intercept(1, EnforcePoint::FileBoundary, 1, 50_000);
        assert!(ce.storm_latched, "51 次触发");
    }

    #[test]
    fn p0_recovers_but_trace_remains() {
        // 执法点故障 → 恢复：故障旗清除，P0 报备留痕（不可抵赖）。
        let mut ce = CapEnforce::new();
        ce.mark_point_fault(EnforcePoint::PrivilegedCall);
        assert!(ce.diag_p0_reported);
        ce.clear_point_fault(EnforcePoint::PrivilegedCall);
        assert!(!ce.point_fault[3], "故障旗清除");
        assert!(ce.diag_p0_reported, "P0 报备留痕不撤销");
    }
}
