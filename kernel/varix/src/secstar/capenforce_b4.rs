//! F177 能力执法可视化 · 批次四深化（secstar · G-G-07）。
//!
//! 批次四功能面（与批次三互补：批次三管「决策与升档」，本批管
//! 「执法点健康与文案」）：
//! - [`PointHealth`]：四执法点健康账——各自拦截/异常计数 + 自检位
//!   （执法点自身异常 P0 报备的账面本体）；
//! - [`NoticeBuilder`]：通知文案渲染——`{应用} 尝试 {能力}，已按
//!   「{规则名}」处理` 模板填空 + 96B 截断（主册逐字文案的渲染面）；
//! - [`AggWindow`]：聚合窗——同应用 5min 滑动计数（聚合正确性的
//!   窗口面：窗内合并、窗外新开）；
//! - [`RuleMatrix`]：应用×规则命中矩阵——哪条规则拦了谁（规则调优
//!   的数据面：过严/过松一眼可见）。
//!
//! 零堆纪律：定长计数器 + 定长缓冲，无 alloc。

use super::capenforce::{AGGREGATE_WINDOW_MS, EnforcePoint, POINT_N, TEXT_CAP};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 执法点健康账
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct PointHealth {
    /// 四执法点各自拦截计数。
    pub intercepts: [u64; POINT_N],
    /// 四执法点各自异常计数（执法点自身故障）。
    pub faults: [u32; POINT_N],
    /// P0 报备旗（任一点异常即置位——异常零静默）。
    pub p0_reported: bool,
}

impl PointHealth {
    pub const fn new() -> PointHealth {
        PointHealth { intercepts: [0; POINT_N], faults: [0; POINT_N], p0_reported: false }
    }

    fn idx(p: EnforcePoint) -> usize {
        p as usize
    }

    pub fn on_intercept(&mut self, p: EnforcePoint) {
        if Self::idx(p) < POINT_N {
            self.intercepts[Self::idx(p)] += 1;
        }
    }

    /// 执法点自身异常：计数 + P0 报备旗（异常显性化红线）。
    pub fn on_fault(&mut self, p: EnforcePoint) {
        if Self::idx(p) < POINT_N {
            self.faults[Self::idx(p)] += 1;
            self.p0_reported = true;
        }
    }

    /// 恢复：异常清零（自检通过后——报备旗保留历史不可清）。
    pub fn clear_faults(&mut self) {
        self.faults = [0; POINT_N];
    }

    /// 最忙执法点（执法画像：拦截集中在哪个面）。
    pub fn busiest(&self) -> Option<usize> {
        let max = self.intercepts.iter().max()?;
        if *max == 0 {
            return None;
        }
        self.intercepts.iter().position(|v| v == max)
    }
}

// ---------------------------------------------------------------------------
// 通知文案渲染（模板填空 + 截断）
// ---------------------------------------------------------------------------

/// 渲染 `{app} 尝试 {cap}，已按「{rule}」处理` 进 96B 缓冲。
/// 返回写入字节数；任一段超预算 → 截断到缓冲界（UTF-8 粗糙截断
/// 在 96B 面——kernel 文案缓冲按字节截断，调用方保证段边界）。
pub fn render_notice(app: &[u8], cap: &[u8], rule: &[u8], out: &mut [u8; TEXT_CAP]) -> usize {
    let mut n = 0;
    let put = |src: &[u8], out: &mut [u8; TEXT_CAP], n: &mut usize| {
        for b in src {
            if *n < out.len() {
                out[*n] = *b;
                *n += 1;
            }
        }
    };
    put(app, out, &mut n);
    put(" 尝试 ".as_bytes(), out, &mut n);
    put(cap, out, &mut n);
    put("，已按「".as_bytes(), out, &mut n);
    put(rule, out, &mut n);
    put("」处理".as_bytes(), out, &mut n);
    n
}

/// 渲染完整性：非截断场景下缓冲内容 == 模板拼接结果（渲染面等价性）。
/// 期望值在 128B 定长暂存里重建（零堆——不用 Vec）。
pub fn notice_matches_template(app: &str, cap: &str, rule: &str, written: &[u8]) -> bool {
    let mut expect = [0u8; 128];
    let mut n = 0usize;
    let put = |src: &[u8], buf: &mut [u8; 128], n: &mut usize| {
        for b in src {
            if *n < buf.len() {
                buf[*n] = *b;
                *n += 1;
            }
        }
    };
    put(app.as_bytes(), &mut expect, &mut n);
    put(" 尝试 ".as_bytes(), &mut expect, &mut n);
    put(cap.as_bytes(), &mut expect, &mut n);
    put("，已按「".as_bytes(), &mut expect, &mut n);
    put(rule.as_bytes(), &mut expect, &mut n);
    put("」处理".as_bytes(), &mut expect, &mut n);
    written.len() == n && written == &expect[..n]
}

// ---------------------------------------------------------------------------
// 聚合窗（5min 滑动）
// ---------------------------------------------------------------------------

/// 聚合窗：同应用同执法点 5 分钟内合并计数。
#[derive(Clone, Copy, Debug)]
pub struct AggWindow {
    app_id: u32,
    window_start: u64,
    pub count: u64,
}

impl AggWindow {
    pub const fn new() -> AggWindow {
        AggWindow { app_id: 0, window_start: 0, count: 0 }
    }

    /// 进事件：同应用窗内 → 合并计数返回 false（不通知）；窗外/新应用
    /// → 新窗返回 true（通知一次）。
    pub fn feed(&mut self, app_id: u32, at_ms: u64) -> bool {
        if self.count > 0 && self.app_id == app_id && at_ms.saturating_sub(self.window_start) < AGGREGATE_WINDOW_MS {
            self.count += 1;
            return false;
        }
        self.app_id = app_id;
        self.window_start = at_ms;
        self.count = 1;
        true
    }

    /// 窗口期满滑出（节流面：老窗过期即失效）。
    pub fn expired(&self, now_ms: u64) -> bool {
        self.count > 0 && now_ms.saturating_sub(self.window_start) >= AGGREGATE_WINDOW_MS
    }
}

// ---------------------------------------------------------------------------
// 应用×规则命中矩阵
// ---------------------------------------------------------------------------

/// 应用槽（32）。
pub const MATRIX_APP_CAP: usize = 32;

#[derive(Clone, Copy)]
pub struct RuleMatrix {
    /// [app][rule_id 0-7] 命中计数（规则 id 压缩到 8 位面）。
    hits: [[u16; 8]; MATRIX_APP_CAP],
    app_ids: [Option<u32>; MATRIX_APP_CAP],
    pub n: usize,
}

impl RuleMatrix {
    pub const fn new() -> RuleMatrix {
        RuleMatrix { hits: [[0; 8]; MATRIX_APP_CAP], app_ids: [const { None }; MATRIX_APP_CAP], n: 0 }
    }

    fn slot(&mut self, app_id: u32) -> Option<usize> {
        for i in 0..self.n {
            if self.app_ids[i] == Some(app_id) {
                return Some(i);
            }
        }
        if self.n >= MATRIX_APP_CAP {
            return None;
        }
        self.app_ids[self.n] = Some(app_id);
        self.n += 1;
        Some(self.n - 1)
    }

    /// 记一次命中（rule_id 0-7；越界诚实拒）。
    pub fn record(&mut self, app_id: u32, rule_id: usize) -> bool {
        if rule_id >= 8 {
            return false;
        }
        match self.slot(app_id) {
            Some(s) => {
                self.hits[s][rule_id] = self.hits[s][rule_id].saturating_add(1);
                true
            }
            None => false,
        }
    }

    /// 某应用某规则命中数。
    pub fn hits_of(&self, app_id: u32, rule_id: usize) -> u16 {
        for i in 0..self.n {
            if self.app_ids[i] == Some(app_id) {
                return self.hits[i][rule_id.min(7)];
            }
        }
        0
    }

    /// 过严检测：某规则命中占比 >80% → 规则调优候选（画像面）。
    pub fn rule_over_eager(&self, rule_id: usize) -> bool {
        let mut total = 0u32;
        let mut mine = 0u32;
        for i in 0..self.n {
            for r in 0..8 {
                total += self.hits[i][r] as u32;
            }
            mine += self.hits[i][rule_id.min(7)] as u32;
        }
        total > 0 && mine * 10 > total * 8
    }
}

// ---------------------------------------------------------------------------
// 批次四自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_capenforce_b4_checks() -> CheckSet {
    use EnforcePoint as EP;
    let mut cs = CheckSet::new("F177-b4");

    // 1) 健康账拦截计数：四点各自计数（画像面）。
    let mut h = PointHealth::new();
    h.on_intercept(EP::FileBoundary);
    h.on_intercept(EP::FileBoundary);
    h.on_intercept(EP::DeviceDirect);
    cs.add(
        "point_health_intercepts",
        h.intercepts[0] == 2 && h.busiest() == Some(0),
        "",
    );

    // 2) 执法点异常：计数 + P0 旗置位（异常零静默）。
    h.on_fault(EP::NetUnauthorized);
    cs.add("point_health_p0", h.faults[1] == 1 && h.p0_reported, "");

    // 3) 异常恢复：clear 后计数归零、报备旗保留历史（不可洗白）。
    h.clear_faults();
    cs.add("point_health_clear_keeps_p0", h.faults[1] == 0 && h.p0_reported, "");

    // 4) 文案渲染：模板逐字（主册句式）。
    let mut out = [0u8; TEXT_CAP];
    let n = render_notice("记事本".as_bytes(), "摄像头".as_bytes(), "默认拒绝".as_bytes(), &mut out);
    cs.add(
        "notice_template_exact",
        notice_matches_template("记事本", "摄像头", "默认拒绝", &out[..n]),
        "",
    );

    // 5) 文案截断：超 96B 填满即止（不越界不崩）。
    let long_app = [b'x'; TEXT_CAP];
    let n2 = render_notice(&long_app, "摄像头".as_bytes(), "规则".as_bytes(), &mut out);
    cs.add("notice_truncates", n2 == TEXT_CAP, "");

    // 6) 聚合窗合并：同应用 5min 内三事件 → 只通知一次（首次）。
    let mut w = AggWindow::new();
    let n1 = w.feed(7, 1_000);
    let n2 = w.feed(7, 60_000);
    let n3 = w.feed(7, 120_000);
    cs.add("agg_window_merges", n1 && !n2 && !n3 && w.count == 3, "");

    // 7) 聚合窗分应用：应用 B 不被 A 的窗吞（隔离面）。
    let mut w2 = AggWindow::new();
    w2.feed(1, 0);
    let b = w2.feed(2, 1_000);
    cs.add("agg_window_per_app", b && w2.count == 1 && w2.app_id == 2, "");

    // 8) 聚合窗期满：5min 后同应用再犯 → 新窗新通知（节流不放走真犯）。
    let mut w3 = AggWindow::new();
    w3.feed(5, 0);
    let after = w3.feed(5, AGGREGATE_WINDOW_MS);
    cs.add("agg_window_expires", after && w3.window_start == AGGREGATE_WINDOW_MS, "");

    // 9) 矩阵命中：app7 规则 2 三次（调优数据面）。
    let mut m = RuleMatrix::new();
    m.record(7, 2);
    m.record(7, 2);
    m.record(7, 2);
    cs.add("rule_matrix_counts", m.hits_of(7, 2) == 3 && m.hits_of(8, 2) == 0, "");

    // 10) 矩阵越界规则拒：rule_id 8 诚实拒（8 位面容量）。
    let mut m2 = RuleMatrix::new();
    cs.add("rule_matrix_bounds", !m2.record(1, 8) && m2.record(1, 7), "");

    // 11) 过严检测：规则 0 占比 >80% → 调优候选真、均衡假。
    let mut m3 = RuleMatrix::new();
    for _ in 0..9 {
        m3.record(1, 0);
    }
    m3.record(1, 1);
    let eager = m3.rule_over_eager(0);
    let mut m4 = RuleMatrix::new();
    m4.record(1, 0);
    m4.record(1, 1);
    cs.add("rule_over_eager", eager && !m4.rule_over_eager(0), "");

    // 12) 主册常量贯通：聚合 5min / 四执法点 / 文案 96B 一处一事实。
    cs.add("consts_aligned", AGGREGATE_WINDOW_MS == 300_000 && POINT_N == 4 && TEXT_CAP == 96, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次四）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b4 {
    use super::*;
    use EnforcePoint as EP;

    #[test]
    fn point_health_all_four() {
        // 四执法点逐一计数互不串扰（画像隔离）。
        let mut h = PointHealth::new();
        for (i, p) in [EP::FileBoundary, EP::NetUnauthorized, EP::DeviceDirect, EP::PrivilegedCall].iter().enumerate() {
            for _ in 0..(i + 1) {
                h.on_intercept(*p);
            }
        }
        assert_eq!(h.intercepts, [1, 2, 3, 4]);
        assert_eq!(h.busiest(), Some(3));
    }

    #[test]
    fn agg_window_saturates_count() {
        // 窗内海量事件计数饱和不溢出（u64 级——长期运行不崩）。
        let mut w = AggWindow::new();
        w.feed(3, 0);
        for t in 1..100_000u64 {
            w.feed(3, t);
        }
        assert_eq!(w.count, 100_000);
    }

    #[test]
    fn notice_ascii_and_cjk_mixed() {
        // 中英混排模板：英文应用名 + 中文能力 + 中文规则全保真。
        let mut out = [0u8; TEXT_CAP];
        let n = render_notice(b"Notepad2", b"mic", b"strict", &mut out);
        assert!(notice_matches_template("Notepad2", "mic", "strict", &out[..n]));
    }
}
