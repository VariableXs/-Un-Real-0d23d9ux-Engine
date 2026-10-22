//! cga153_regressgate.rs —— CGPU-F0153 · 性能回归门固化
//!
//! A 域性能回归门的长期化固化（F0060/F0105/F0116 基准收编）：
//! 门禁配置终版（阈值表冻结 + 指纹签章）、免打扰调优（防误报的
//! 容差校准——噪声分位建模）、误报率台账、维护手册生成面、
//! CI 验证状态机。
//! 设计立场：门禁的价值 = 拦住真回归 − 误报骚扰成本。
//! 容差太紧 = 天天误报被绕过；太松 = 拦不住。校准是数据驱动的。
//! 诊断码段 0x0380（本席 WB1022 分配表，全域唯一）。
//! no_std + alloc 零 unwrap、零 panic 路径。

use alloc::string::String;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0380;

/// 门禁指标类别（与 F0060/F0105/F0116 三册基准一一对应）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetricKind {
    /// 吞吐基准（F0060）。
    Throughput,
    /// 延迟分位基准（F0105）。
    Latency,
    /// 混合负载收益基准（F0116）。
    MixedGain,
}

impl MetricKind {
    /// 全类别。
    pub const ALL: [MetricKind; 3] = [
        MetricKind::Throughput,
        MetricKind::Latency,
        MetricKind::MixedGain,
    ];

    /// 序号。
    pub fn rank(self) -> u8 {
        match self {
            MetricKind::Throughput => 0,
            MetricKind::Latency => 1,
            MetricKind::MixedGain => 2,
        }
    }
}

/// 单指标门禁配置（终版形态：冻结后指纹签章）。
#[derive(Clone, Debug)]
pub struct GateRule {
    /// 指标类别。
    pub kind: MetricKind,
    /// 回归判定阈值（相对基准的千分比劣化上限）。
    pub regress_permille: u32,
    /// 容差（千分比）：劣化在 (regress-容差, regress] 区间内
    /// 需连续 hit_confirm 次才判真回归（防单次噪声误报）。
    pub tolerance_permille: u32,
    /// 灰区确认次数。
    pub hit_confirm: u8,
    /// 冻结签章（None=未冻结）。
    pub seal: Option<u64>,
}

impl GateRule {
    /// 结构自检：容差必须 < 阈值（否则灰区吞掉判定区），
    /// 确认次数 ≥1 且 ≤7。
    pub fn validate(&self) -> Result<(), u16> {
        if self.regress_permille == 0 || self.regress_permille > 500 {
            return Err(SEG_BASE + 1);
        }
        if self.tolerance_permille >= self.regress_permille {
            return Err(SEG_BASE + 2);
        }
        if self.hit_confirm == 0 || self.hit_confirm > 7 {
            return Err(SEG_BASE + 3);
        }
        Ok(())
    }
}

/// 一次门禁采样。
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// 指标类别。
    pub kind: MetricKind,
    /// 本次相对基准的劣化千分比（正=劣化，负=改善）。
    pub degrade_permille: i64,
}

impl Sample {
    /// 合法性：劣化幅度物理界 ±100%。
    fn plausible(&self) -> bool {
        self.degrade_permille.abs() <= 1000
    }
}

/// 单指标观测窗（灰区计数器 + 噪声分位建模数据）。
#[derive(Clone, Debug)]
pub struct MetricWindow {
    /// 规则。
    pub rule: GateRule,
    /// 灰区连续命中计数。
    gray_hits: u8,
    /// 历史采样（噪声建模用，滚动窗）。
    history: Vec<i64>,
    /// 历史上限条数。
    cap: usize,
    /// 已判定的真回归次数。
    true_regress: u32,
    /// 已确认过的误报次数（灰区计数被噪声打断归零）。
    false_alarms: u32,
}

impl MetricWindow {
    /// 新窗。
    pub fn new(rule: GateRule, cap: usize) -> Result<Self, u16> {
        rule.validate()?;
        if cap < 8 {
            return Err(SEG_BASE + 4);
        }
        Ok(MetricWindow {
            rule,
            gray_hits: 0,
            history: Vec::new(),
            cap,
            true_regress: 0,
            false_alarms: 0,
        })
    }

    /// 真回归计数。
    pub fn regress_count(&self) -> u32 {
        self.true_regress
    }

    /// 误报计数。
    pub fn false_alarm_count(&self) -> u32 {
        self.false_alarms
    }

    /// 喂样（判据「CI 验证」的采样面）：三区判定。
    /// 硬红区（>阈值）立即回归；灰区连续 hit_confirm 次判回归；
    /// 其余清零灰区计数（若此前灰区计数 ≥2，计入一次误报链）。
    pub fn feed(&mut self, s: Sample) -> Result<GateVerdict, u16> {
        if s.kind != self.rule.kind || !s.plausible() {
            return Err(SEG_BASE + 5);
        }
        self.push_history(s.degrade_permille)?;
        let d = s.degrade_permille;
        let th = self.rule.regress_permille as i64;
        let tol = self.rule.tolerance_permille as i64;
        if d > th {
            self.true_regress = self.true_regress.saturating_add(1);
            self.gray_hits = 0;
            return Ok(GateVerdict::Regress);
        }
        if d > th - tol {
            self.gray_hits = self.gray_hits.saturating_add(1);
            if self.gray_hits >= self.rule.hit_confirm {
                self.true_regress = self.true_regress.saturating_add(1);
                self.gray_hits = 0;
                return Ok(GateVerdict::Regress);
            }
            return Ok(GateVerdict::Gray);
        }
        if self.gray_hits >= 2 {
            self.false_alarms = self.false_alarms.saturating_add(1);
        }
        self.gray_hits = 0;
        Ok(GateVerdict::Pass)
    }

    /// 历史滚动窗。
    fn push_history(&mut self, v: i64) -> Result<(), u16> {
        if self.history.len() >= self.cap {
            // 滚动淘汰最旧一半：批量淘汰比逐条 shift 便宜。
            let keep_from = self.cap / 2;
            let kept: Vec<i64> = self.history[keep_from..].to_vec();
            self.history = kept;
        }
        self.history.push(v);
        Ok(())
    }

    /// 噪声分位建模（判据「容差校准」的数据面）：
    /// 返回历史样本的 P50 与 P95 劣化值。校准律：容差应 ≥ P95
    /// 才能压住误报——不满足时给出建议容差。
    pub fn noise_profile(&self) -> Option<(i64, i64)> {
        if self.history.len() < 8 {
            return None;
        }
        let mut s = self.history.clone();
        s.sort_unstable();
        let p50 = s[s.len() / 2];
        let p95 = s[(s.len() * 95) / 100];
        Some((p50, p95))
    }

    /// 容差校准建议（判据「容差校准」的裁决面）：
    /// P95 噪声 ≥ 现容差 ⇒ 建议抬到 P95+缓冲；P95 远小于容差
    /// （< 一半）⇒ 建议收紧到 P95+缓冲（门禁更灵）。
    pub fn tolerance_advice(&self) -> Option<u32> {
        let (_, p95) = self.noise_profile()?;
        let buf = (p95.abs() / 10).max(5);
        if p95 < 0 {
            // 噪声全是改善方向，现有容差够用。
            return Some(self.rule.tolerance_permille);
        }
        let need = (p95 + buf) as u32;
        if need > self.rule.tolerance_permille {
            Some(need.min(self.rule.regress_permille.saturating_sub(1)))
        } else if need * 2 < self.rule.tolerance_permille {
            Some(need.max(1))
        } else {
            Some(self.rule.tolerance_permille)
        }
    }

    /// 误报率（千分比）：误报链 /（误报链 + 真回归），无样本返回 None。
    pub fn false_alarm_permille(&self) -> Option<u32> {
        let total = self.false_alarms + self.true_regress;
        if total == 0 {
            return None;
        }
        Some((self.false_alarms * 1000) / total)
    }
}

/// 门禁判定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateVerdict {
    /// 通过。
    Pass,
    /// 灰区（待确认，不拦截）。
    Gray,
    /// 回归（拦截）。
    Regress,
}

/// 回归门总闸（判据「门禁配置终版」的载体）。
pub struct RegressGate {
    /// 三指标观测窗。
    windows: Vec<MetricWindow>,
    /// 终版冻结签章（None=配置未冻结，CI 不得引用）。
    frozen: Option<u64>,
    /// CI 验证状态机状态：0=未验 1=过跑 2=拦截演练过 3=终验通过。
    ci_state: u8,
    /// 误报率红线（千分比）：校准后误报率超线即要求再调优。
    fa_redline: u32,
}

impl RegressGate {
    /// 建闸：三类指标窗齐备才许建（缺一即拒——单指标门禁是假门禁）。
    pub fn new(rules: [GateRule; 3], cap: usize, fa_redline: u32) -> Result<Self, u16> {
        let mut windows = Vec::new();
        for i in 0..3 {
            if rules[i].kind.rank() != i as u8 {
                return Err(SEG_BASE + 6);
            }
            windows.push(MetricWindow::new(rules[i].clone(), cap)?);
        }
        if fa_redline == 0 || fa_redline > 500 {
            return Err(SEG_BASE + 7);
        }
        Ok(RegressGate {
            windows,
            frozen: None,
            ci_state: 0,
            fa_redline,
        })
    }

    /// 冻结终版（判据「门禁配置终版」）：冻结后规则不可变——
    /// 变更走「解冻 + 重签」全流程，签章覆盖全规则指纹。
    pub fn freeze(&mut self, config_canon: &[u8]) -> Result<u64, u16> {
        if self.frozen.is_some() {
            return Err(SEG_BASE + 8);
        }
        for w in &self.windows {
            w.rule.validate()?;
        }
        let mut fp: u64 = 0xcbf29ce484222325;
        for w in &self.windows {
            fp = fp.wrapping_mul(0x100000001b3) ^ (w.rule.regress_permille as u64);
            fp = fp.wrapping_mul(0x100000001b3) ^ (w.rule.tolerance_permille as u64);
            fp = fp.wrapping_mul(0x100000001b3) ^ (w.rule.hit_confirm as u64);
        }
        for &b in config_canon {
            fp = fp.wrapping_mul(0x100000001b3) ^ (b as u64);
        }
        if fp == 0 {
            return Err(SEG_BASE + 9);
        }
        self.frozen = Some(fp);
        Ok(fp)
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen.is_some()
    }

    /// 终版签章。
    pub fn seal(&self) -> Option<u64> {
        self.frozen
    }

    /// CI 喂样（判据「CI 验证」）：未冻结的闸一律拒收样——
    /// 未终版的配置跑 CI 是无效验证。
    pub fn ci_feed(&mut self, s: Sample) -> Result<GateVerdict, u16> {
        if self.frozen.is_none() {
            return Err(SEG_BASE + 10);
        }
        let w = self
            .windows
            .iter_mut()
            .find(|w| w.rule.kind == s.kind)
            .ok_or(SEG_BASE + 11)?;
        w.feed(s)
    }

    /// CI 演练状态推进（0→1 跑通→2 拦截演练→3 终验）：必须逐级，
    /// 拦截演练要求曾至少判过一次 Regress（没拦过的门不算验过）。
    pub fn ci_advance(&mut self) -> Result<u8, u16> {
        match self.ci_state {
            0 => self.ci_state = 1,
            1 => {
                let hit = self.windows.iter().any(|w| w.regress_count() > 0);
                if !hit {
                    return Err(SEG_BASE + 12);
                }
                self.ci_state = 2;
            }
            2 => {
                for w in &self.windows {
                    if let Some(fa) = w.false_alarm_permille() {
                        if fa > self.fa_redline {
                            return Err(SEG_BASE + 13);
                        }
                    }
                }
                self.ci_state = 3;
            }
            _ => return Err(SEG_BASE + 14),
        }
        Ok(self.ci_state)
    }

    /// CI 终验状态。
    pub fn ci_state(&self) -> u8 {
        self.ci_state
    }

    /// 全局误报率（判据「误报率」）：三窗误报合计 / 判定合计。
    pub fn global_false_alarm_permille(&self) -> Option<u32> {
        let fa: u32 = self.windows.iter().map(|w| w.false_alarm_count()).sum();
        let tr: u32 = self.windows.iter().map(|w| w.regress_count()).sum();
        let total = fa + tr;
        if total == 0 {
            return None;
        }
        Some((fa * 1000) / total)
    }

    /// 免打扰调优（判据「门禁的免打扰调优」）：对全部有噪样本的
    /// 窗套用容差建议并重签——调优必须重新冻结（旧签章作废）。
    pub fn retune(&mut self, config_canon: &[u8]) -> Result<u64, u16> {
        if self.frozen.is_none() {
            return Err(SEG_BASE + 15);
        }
        let mut changed = false;
        for w in self.windows.iter_mut() {
            if let Some(adv) = w.tolerance_advice() {
                if adv != w.rule.tolerance_permille {
                    w.rule.tolerance_permille = adv;
                    changed = true;
                }
            }
        }
        self.frozen = None;
        if changed {
            self.ci_state = 0;
            self.freeze(config_canon)
        } else {
            // 无需调优也要求重签（签章覆盖调优动作本身，可审计）。
            self.freeze(config_canon)
        }
    }

    /// 维护手册生成（判据「门禁维护手册」）：纯账面投影——
    /// 三窗规则 + 红线 + CI 状态 + 误报统计，行式文本。
    pub fn manual(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(String::from("== CGPU 性能回归门 · 维护手册 =="));
        for w in &self.windows {
            let line = match w.rule.kind {
                MetricKind::Throughput => "吞吐门",
                MetricKind::Latency => "延迟门",
                MetricKind::MixedGain => "混合收益门",
            };
            let mut s = String::from(line);
            s.push_str(": 阈值 ");
            push_u64(&mut s, w.rule.regress_permille as u64);
            s.push_str("‰ 容差 ");
            push_u64(&mut s, w.rule.tolerance_permille as u64);
            s.push_str("‰ 确认 ");
            push_u64(&mut s, w.rule.hit_confirm as u64);
            s.push_str(" 次");
            out.push(s);
        }
        let mut s = String::from("误报红线: ");
        push_u64(&mut s, self.fa_redline as u64);
        s.push_str("‰");
        out.push(s);
        let mut s = String::from("CI 状态: ");
        push_u64(&mut s, self.ci_state as u64);
        out.push(s);
        match self.global_false_alarm_permille() {
            Some(fa) => {
                let mut s = String::from("全局误报率: ");
                push_u64(&mut s, fa as u64);
                s.push_str("‰");
                out.push(s);
            }
            None => out.push(String::from("全局误报率: 无样本")),
        }
        match self.frozen {
            Some(fp) => {
                let mut s = String::from("终版签章: ");
                push_u64(&mut s, fp);
                out.push(s);
            }
            None => out.push(String::from("终版签章: 未冻结")),
        }
        out
    }
}

impl Default for RegressGate {
    fn default() -> Self {
        // 不可直接使用：三类规则必须显式给出。占位 Default 仅满足
        // 纪律约定，建闸必须走 new()（带全规则校验）。
        RegressGate {
            windows: Vec::new(),
            frozen: None,
            ci_state: 0,
            fa_redline: 300,
        }
    }
}

/// 无格式化宏环境下的十进制写入（no_std 手写 itoa）。
fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    // buf 剩余部分全为 ASCII，直接按字节追加安全。
    let bytes = &buf[i..];
    s.push_str(core::str::from_utf8(bytes).unwrap_or(""));
}

/// 门禁配置审计（判据支撑）：未冻结的闸 + CI 终验未过 + 误报超线，
/// 三者任一成立即配置未达标（返回 Err 带定位码）。
pub fn gate_audit(g: &RegressGate) -> Result<(), u16> {
    if !g.is_frozen() {
        return Err(SEG_BASE + 16);
    }
    if g.ci_state() < 3 {
        return Err(SEG_BASE + 17);
    }
    if let Some(fa) = g.global_false_alarm_permille() {
        if fa > 500 {
            return Err(SEG_BASE + 18);
        }
    }
    Ok(())
}
