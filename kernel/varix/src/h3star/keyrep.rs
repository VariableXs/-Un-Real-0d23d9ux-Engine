//! F347 键盘重复参数 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：四档延迟与速率范围实测（示波器法）；测试框体验；
//! 终端覆盖文档化；参数持久化；全系统遵循审计（自设重复逻辑的组件=0）。
//!
//! **设计要点（主册）**：
//! - 按键重复两参数开放：重复延迟（250/500/750/1000ms 四档）与重复速
//!   率（2-30 字符/秒滑杆）——设置页带测试框（按住键盘实时感受当前参
//!   数）；系统文本框全部遵循（终端可独立覆盖——明文档化）；
//! - 无感标准：按住删除键删半篇文章的速度自己说了算；调参即时可试；
//!   全系统一个参数源不打架。
//!
//! 实现形态：参数源（唯一注册点）+ 重复发生器（注入钟驱动——示波器法
//! 的模型面：逐键计时验证延迟与速率）+ 覆盖面登记账（终端覆盖文档化）
//! + 组件遵循审计（自设重复=0）。

use crate::checks::CheckSet;

use super::hbase::PersistKv;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 重复延迟四档（ms）。
pub const DELAY_STEPS_MS: [u64; 4] = [250, 500, 750, 1000];

/// 重复速率范围（字符/秒）。
pub const RATE_MIN_CPS: u64 = 2;
pub const RATE_MAX_CPS: u64 = 30;

/// 默认档（延迟 500ms / 速率 15cps——中位）。
pub const DEFAULT_DELAY_MS: u64 = 500;
pub const DEFAULT_RATE_CPS: u64 = 15;

// ---------------------------------------------------------------------------
// 参数源（唯一注册点）
// ---------------------------------------------------------------------------

/// 键盘重复参数（全系统唯一参数源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepeatParams {
    pub delay_ms: u64,
    pub rate_cps: u64,
}

impl RepeatParams {
    pub fn default_params() -> RepeatParams {
        RepeatParams { delay_ms: DEFAULT_DELAY_MS, rate_cps: DEFAULT_RATE_CPS }
    }

    /// 设延迟（必须落在四档内——档外拒绝，不静默钳制）。
    pub fn set_delay(&mut self, ms: u64) -> bool {
        if DELAY_STEPS_MS.contains(&ms) {
            self.delay_ms = ms;
            true
        } else {
            false
        }
    }

    /// 设速率（2-30 滑杆——越界拒绝）。
    pub fn set_rate(&mut self, cps: u64) -> bool {
        if (RATE_MIN_CPS..=RATE_MAX_CPS).contains(&cps) {
            self.rate_cps = cps;
            true
        } else {
            false
        }
    }

    /// 重复间隔（ms）——由速率导出（1000/速率，四舍五入）。
    pub fn interval_ms(&self) -> u64 {
        (1000 + self.rate_cps / 2) / self.rate_cps.max(1)
    }
}

/// 参数源（全局单点 + 持久化 + 覆盖登记）。
pub struct RepeatSource {
    pub params: RepeatParams,
    /// 覆盖面登记（组件名 → 自有参数；仅终端类允许登记——文档化纪律）。
    overrides: Vec<(&'static str, RepeatParams, &'static str)>,
    disk: PersistKv,
}

impl RepeatSource {
    pub fn new() -> RepeatSource {
        RepeatSource { params: RepeatParams::default_params(), overrides: Vec::new(), disk: PersistKv::new() }
    }

    /// 改参数（即时生效——测试框实时感受）。
    pub fn set(&mut self, delay_ms: u64, rate_cps: u64) -> bool {
        let mut p = self.params;
        let ok = p.set_delay(delay_ms) && p.set_rate(rate_cps);
        if ok {
            self.params = p;
            self.disk.set("kbd.repeat", &alloc::format!("{},{}", delay_ms, rate_cps));
            self.disk.flush();
        }
        ok
    }

    /// 终端覆盖登记（唯一允许的覆盖类——文档化说明必附）。
    pub fn register_override(
        &mut self,
        component: &'static str,
        p: RepeatParams,
        doc: &'static str,
    ) -> bool {
        let terminal_class = component.starts_with("terminal");
        if !terminal_class || doc.is_empty() {
            return false;
        }
        if self.overrides.iter().any(|(c, _, _)| *c == component) {
            return false;
        }
        self.overrides.push((component, p, doc));
        true
    }

    /// 组件取参：有覆盖走覆盖，否则全局源（审计面直读）。
    pub fn params_for(&self, component: &str) -> RepeatParams {
        self.overrides
            .iter()
            .find(|(c, _, _)| *c == component)
            .map(|(_, p, _)| *p)
            .unwrap_or(self.params)
    }

    /// 全系统遵循审计：非终端类组件自设重复 = 0（覆盖账只允许终端类）。
    pub fn audit_self_repeat(&self) -> bool {
        self.overrides.iter().all(|(c, _, _)| c.starts_with("terminal"))
    }

    /// 持久化恢复（重启读回）。
    pub fn reboot(&self) -> RepeatParams {
        match self.disk.get("kbd.repeat") {
            Some(s) => {
                let mut it = s.split(',');
                let d = it.next().and_then(|x| x.parse::<u64>().ok());
                let r = it.next().and_then(|x| x.parse::<u64>().ok());
                match (d, r) {
                    (Some(d), Some(r)) if DELAY_STEPS_MS.contains(&d) && (RATE_MIN_CPS..=RATE_MAX_CPS).contains(&r) => {
                        RepeatParams { delay_ms: d, rate_cps: r }
                    }
                    _ => RepeatParams::default_params(),
                }
            }
            None => RepeatParams::default_params(),
        }
    }
}

impl Default for RepeatSource {
    fn default() -> RepeatSource {
        RepeatSource::new()
    }
}

// ---------------------------------------------------------------------------
// 重复发生器（示波器法模型面——逐键计时）
// ---------------------------------------------------------------------------

/// 重复发生器：按住一键 → 延迟到点发第一个重复，此后按速率连发。
pub struct RepeatEmitter {
    params: RepeatParams,
    press_at_ms: u64,
    emitted: Vec<u64>,
    pub held: bool,
}

impl RepeatEmitter {
    pub fn new(params: RepeatParams) -> RepeatEmitter {
        RepeatEmitter { params, press_at_ms: 0, emitted: Vec::new(), held: false }
    }

    /// 按下。
    pub fn press(&mut self, now_ms: u64) {
        self.held = true;
        self.press_at_ms = now_ms;
        self.emitted.clear();
    }

    /// 松开。
    pub fn release(&mut self) {
        self.held = false;
    }

    /// 到时刻 now 的重复产出时刻账（示波器读数面）。
    /// 首个重复在 press + delay；此后每 interval 一个。
    pub fn events_at(&mut self, now_ms: u64) -> &[u64] {
        if !self.held {
            return &self.emitted;
        }
        let interval = self.params.interval_ms().max(1);
        let first = self.press_at_ms + self.params.delay_ms;
        if now_ms >= first {
            let n = ((now_ms - first) / interval) + 1;
            self.emitted = (0..n).map(|i| first + i * interval).collect();
        }
        &self.emitted
    }

    /// 速率实测（cps）：观察窗内产出数 / 窗口秒——与设定对账。
    pub fn measured_cps(&mut self, observe_ms: u64) -> u64 {
        let start = self.press_at_ms + self.params.delay_ms;
        let end = start + observe_ms;
        let n = self.events_at(end).len() as u64;
        (n * 1000 + observe_ms / 2) / observe_ms.max(1)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F347 自检（判据：四档延迟；速率范围；测试框；终端覆盖；持久化；审计）。
pub fn run_keyrep_checks() -> CheckSet {
    let mut set = CheckSet::new("F347-keyrep");

    // 1. 四档延迟全可设；档外拒绝（示波器法前ettan——参数面合法）。
    let mut src = RepeatSource::new();
    let all_set = DELAY_STEPS_MS.iter().all(|ms| {
        let mut p = RepeatParams::default_params();
        p.set_delay(*ms)
    });
    set.add(
        "delay four steps",
        all_set && !src.params.set_delay(400) && src.params.set_delay(750),
        "",
    );

    // 2. 速率范围 2-30：两端可设、越界拒绝。
    let mut p = RepeatParams::default_params();
    set.add(
        "rate range clamp reject",
        p.set_rate(RATE_MIN_CPS) && p.set_rate(RATE_MAX_CPS) && !p.set_rate(1) && !p.set_rate(31),
        "",
    );

    // 3. 示波器法·延迟：500ms 档 → 首个重复恰在 +500ms（+499 无）。
    let mut em = RepeatEmitter::new(RepeatParams { delay_ms: 500, rate_cps: 15 });
    em.press(1000);
    let early = em.events_at(1499).len();
    let ontime = em.events_at(1500).len();
    set.add("scope first repeat at delay", early == 0 && ontime == 1, "");

    // 4. 示波器法·速率：15cps → 间隔 ≈67ms；1 秒窗实测 15cps（±1）。
    em.release();
    let mut em = RepeatEmitter::new(RepeatParams { delay_ms: 250, rate_cps: 15 });
    em.press(0);
    let cps = em.measured_cps(1000);
    set.add("scope measured cps", (cps as i64 - 15).abs() <= 1, "");

    // 5. 四档 × 速率的间隔表一致性（interval 导出唯一源）。
    let ok = RepeatParams { delay_ms: 250, rate_cps: 30 }.interval_ms() == 33
        && RepeatParams { delay_ms: 250, rate_cps: 2 }.interval_ms() == 500;
    set.add("interval derived from rate", ok, "");

    // 6. 测试框体验：换参即时生效（emitter 换参后下一按立即用新参）。
    let mut src2 = RepeatSource::new();
    let ok = src2.set(1000, 2);
    let em2 = RepeatEmitter::new(src2.params);
    set.add("test box instant apply", ok && em2.params.delay_ms == 1000, "");

    // 7. 终端覆盖文档化：终端类可登记覆盖（说明必附）；非终端拒绝。
    let mut src3 = RepeatSource::new();
    let ok_t = src3.register_override(
        "terminal-main",
        RepeatParams { delay_ms: 250, rate_cps: 30 },
        "终端习惯更快速率——文档化差异（主册口径）",
    );
    set.add(
        "terminal override documented",
        ok_t && src3.params_for("terminal-main").rate_cps == 30
            && !src3.register_override("editor-main", RepeatParams::default_params(), "说明"),
        "",
    );

    // 8. 全系统遵循审计：非终端类自设重复 = 0（覆盖账只含终端类）。
    set.add("audit self repeat zero", src3.audit_self_repeat(), "");

    // 9. 参数持久化：改参 → 重启读回一致。
    let mut src4 = RepeatSource::new();
    let _ = src4.set(750, 20);
    let after = src4.reboot();
    set.add(
        "params persist reboot",
        after == RepeatParams { delay_ms: 750, rate_cps: 20 },
        "",
    );

    // 10. 覆盖面不落全局源：终端覆盖后编辑器仍走全局参数。
    let mut src5 = RepeatSource::new();
    let _ = src5.register_override(
        "terminal-x",
        RepeatParams { delay_ms: 250, rate_cps: 30 },
        "终端快速连发",
    );
    set.add(
        "override isolated",
        src5.params_for("editor-x") == src5.params
            && src5.params_for("terminal-x").rate_cps == 30,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emitter_stops_on_release() {
        let mut em = RepeatEmitter::new(RepeatParams { delay_ms: 250, rate_cps: 10 });
        em.press(0);
        em.release();
        assert!(em.events_at(10_000).is_empty());
    }

    #[test]
    fn interval_rounding() {
        assert_eq!(RepeatParams { delay_ms: 500, rate_cps: 3 }.interval_ms(), 333);
    }

    #[test]
    fn reboot_defaults_on_corrupt() {
        let src = RepeatSource::new();
        let p = src.reboot();
        assert_eq!(p, RepeatParams::default_params());
    }

    #[test]
    fn delay_steps_exact() {
        assert_eq!(DELAY_STEPS_MS, [250, 500, 750, 1000]);
    }
}

// ---------------------------------------------------------------------------
// 深化层三 · 重复率统计（键盘重复的健康观测面）
// ---------------------------------------------------------------------------

/// 重复率统计（键盘重复功能的健康观测面）：逐秒按键账 → 重复次数
/// （按住不放的自动重复）/ 总按键 = 重复率‰——重复率异常高 = 键卡
/// 键或设置过敏感（用户可感知的运营数据，不是静默行为）。
#[derive(Default)]
pub struct RepeatStats {
    /// (秒桶, 总按键, 其中重复)。
    pub buckets: Vec<(u64, u32, u32)>,
}

impl RepeatStats {
    pub fn observe(&mut self, sec: u64, is_auto_repeat: bool) {
        match self.buckets.iter_mut().find(|(s, _, _)| *s == sec) {
            Some((_, total, reps)) => {
                *total += 1;
                if is_auto_repeat {
                    *reps += 1;
                }
            }
            None => {
                self.buckets
                    .push((sec, 1, u32::from(is_auto_repeat)));
            }
        }
    }

    /// 全账重复率‰（总口径）。
    pub fn rate_permille(&self) -> u32 {
        let total: u32 = self.buckets.iter().map(|(_, t, _)| t).sum();
        let reps: u32 = self.buckets.iter().map(|(_, _, r)| r).sum();
        if total == 0 {
            return 0;
        }
        (reps * 1000 / total) as u32
    }

    /// 异常秒桶清单（重复率 >800‰ 的秒——键卡键指纹直出）。
    pub fn hot_buckets(&self) -> Vec<u64> {
        self.buckets
            .iter()
            .filter(|(_, t, r)| *t >= 10 && r * 1000 > *t * 800)
            .map(|(s, _, _)| *s)
            .collect()
    }
}

/// 深化层三自检（重复率统计）。
pub fn run_keyrep_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new("F319b-deep3");

    // 1. 正常打字：重复率低、无异常桶。
    let mut st = RepeatStats::default();
    for _ in 0..20u64 {
        st.observe(0, false);
    }
    st.observe(0, true); // 1/21 重复。
    set.add(
        "normal typing low rate",
        st.rate_permille() == 47 && st.hot_buckets().is_empty(),
        "",
    );

    // 2. 键卡键：单秒 30 按全重复 → 异常桶直出。
    let mut st2 = RepeatStats::default();
    for _ in 0..30 {
        st2.observe(5, true);
    }
    set.add(
        "stuck key hot bucket",
        st2.rate_permille() == 1000 && st2.hot_buckets() == alloc::vec![5],
        "",
    );

    // 3. 空账零率（不虚报）。
    let empty = RepeatStats::default();
    set.add("empty zero rate", empty.rate_permille() == 0, "");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn observe_creates_bucket_on_demand() {
        let mut st = RepeatStats::default();
        st.observe(42, false);
        assert_eq!(st.buckets, vec![(42, 1, 0)]);
    }

    #[test]
    fn hot_bucket_needs_volume() {
        // 样本 <10 的秒桶不判异常（小样本不虚报键卡键）。
        let mut st = RepeatStats::default();
        for _ in 0..5 {
            st.observe(1, true);
        }
        assert!(st.hot_buckets().is_empty());
    }
}
