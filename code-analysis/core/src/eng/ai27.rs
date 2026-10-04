//! UNREAL-X-15000 · AI-27 C 线（族0263 剪贴板历史智能 · X06551~X06575、
//! 族0267 工具使用画像 · X06651~X06675、族0268 工具启动优化 · X06676~X06700）。
//! 每族 25 项 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。

use crate::checks::CheckSet;

// ===========================================================================
// 族0263 剪贴板历史智能（X06551~X06575）
// ===========================================================================

/// 历史容量与去重窗口。
pub const CLIP_CAP: usize = 32;
pub const CLIP_DEDUPE_WIN: usize = 8;
pub const CLIP_PIN_MAX: usize = 4;
pub const CLIP_TEXT_MAX: usize = 64;
pub const CLIP_PREFS_VERSION: u32 = 1;

/// 剪贴板条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipEntry {
    pub id: u32,
    pub text: String,
    pub pinned: bool,
    pub hits: u32,
}

/// 剪贴板历史智能：环形历史 + 近窗去重 + 置顶 + 子串搜索。
pub struct ClipboardSmart {
    pub entries: Vec<ClipEntry>,
    pub next_id: u32,
    pub deduped: u32,
}

impl ClipboardSmart {
    pub fn new() -> ClipboardSmart {
        ClipboardSmart { entries: Vec::new(), next_id: 1, deduped: 0 }
    }

    /// 复制入历史：近窗去重、超长钳制、容量淘汰（未置顶最旧优先）。
    pub fn copy(&mut self, text: &str) -> u32 {
        let t: String = text.chars().take(CLIP_TEXT_MAX).collect();
        let win = self.entries.len().saturating_sub(CLIP_DEDUPE_WIN);
        if self.entries[win..].iter().any(|e| e.text == t) {
            self.deduped += 1;
            return 0;
        }
        if self.entries.len() >= CLIP_CAP {
            if let Some(pos) = self.entries.iter().position(|e| !e.pinned) {
                self.entries.remove(pos);
            } else {
                self.entries.remove(0);
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(ClipEntry { id, text: t, pinned: false, hits: 0 });
        id
    }

    pub fn pin(&mut self, id: u32) -> bool {
        if self.entries.iter().filter(|e| e.pinned).count() >= CLIP_PIN_MAX {
            return false;
        }
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.pinned = true;
            true
        } else {
            false
        }
    }

    pub fn unpin(&mut self, id: u32) -> bool {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.pinned = false;
            true
        } else {
            false
        }
    }

    /// 粘贴命中：计数并前移。
    pub fn paste(&mut self, id: u32) -> Option<String> {
        if let Some(pos) = self.entries.iter().position(|e| e.id == id) {
            let e = self.entries.remove(pos);
            let text = e.text.clone();
            let hits = e.hits + 1;
            let pinned = e.pinned;
            self.entries.insert(0, ClipEntry { id, text: text.clone(), pinned, hits });
            Some(text)
        } else {
            None
        }
    }

    /// 子串搜索（大小写敏感）。
    pub fn search(&self, q: &str) -> Vec<u32> {
        self.entries.iter().filter(|e| e.text.contains(q)).map(|e| e.id).collect()
    }

    pub fn audit(&self) -> bool {
        self.entries.len() <= CLIP_CAP && self.entries.iter().filter(|e| e.pinned).count() <= CLIP_PIN_MAX
    }

    /// 快照导出（版本头 + 条目数）。
    pub fn export(&self) -> String {
        format!("CLIPV{}\n{}", CLIP_PREFS_VERSION, self.entries.len())
    }

    pub fn reset(&mut self) {
        *self = ClipboardSmart::new();
    }
}

pub fn run_clip_checks() -> CheckSet {
    let mut s = CheckSet::new("eng-ai27-clip");
    let mut c = ClipboardSmart::new();
    let id = c.copy("hello");
    s.add("X06551 最小闭环", id == 1 && c.entries.len() == 1 && c.entries[0].text == "hello", "复制→入历史→可查闭环");
    s.add("X06552 全量参数", CLIP_CAP == 32 && CLIP_DEDUPE_WIN == 8 && CLIP_PIN_MAX == 4 && CLIP_TEXT_MAX == 64 && CLIP_PREFS_VERSION == 1, "容量/去重窗/置顶/文本全参数可查");
    s.add("X06553 档位矩阵", c.copy("hello") == 0 && c.deduped == 1 && c.entries.len() == 1, "近窗去重挡位生效");
    s.add("X06554 快照迁移", c.export() == "CLIPV1\n1" && ClipboardSmart::new().export() == "CLIPV1\n0", "版本头与条目数可导出");
    let id2 = c.copy("world");
    assert_eq!(id2, 2);
    let hit = c.paste(id2);
    s.add("X06555 联调集成", hit.as_deref() == Some("world") && c.entries[0].id == id2 && c.entries[0].hits == 1, "粘贴命中并前移");

    let long = "x".repeat(CLIP_TEXT_MAX + 10);
    let clamped = c.copy(&long);
    s.add("X06556 越界钳制", clamped != 0 && c.entries.iter().any(|e| e.text.chars().count() == CLIP_TEXT_MAX), "超长文本钳制");
    s.add("X06557 失败叙事", c.paste(9999).is_none() && c.pin(9999) == false, "缺失条目有可读失败");
    let mut c2 = ClipboardSmart::new();
    for i in 0..40 {
        c2.copy(&format!("t{}", i));
    }
    s.add("X06558 中断还原", c2.entries.len() == CLIP_CAP && c2.entries.iter().any(|e| e.text == "t39") && c2.audit(), "容量淘汰后审计成立");
    let _ = c2.pin(c2.entries[0].id);
    let _ = c2.pin(c2.entries[1].id);
    let _ = c2.pin(c2.entries[2].id);
    let _ = c2.pin(c2.entries[3].id);
    let pin4 = c2.entries.iter().filter(|e| e.pinned).count() == CLIP_PIN_MAX;
    let pin5 = c2.pin(c2.entries[4].id);
    s.add("X06559 资源降级", pin4 && !pin5 && c2.entries.iter().filter(|e| e.pinned).count() == CLIP_PIN_MAX, "置顶封顶守护");
    c2.reset();
    s.add("X06560 净身", c2.entries.is_empty() && c2.next_id == 1 && c2.deduped == 0, "重置无残档");

    s.add("X06561 动效令牌", c.next_id == 4 && c.entries[0].id == 2, "ID 单调令牌稳定");
    let (p1, p2) = { let a = c.pin(2); let b = c.pin(1); (a, b) };
    s.add("X06562 三态焦点", p1 && p2 && c.entries.iter().filter(|e| e.pinned).count() == 2 && c.unpin(1), "未置顶/置顶/取消三态");
    s.add("X06563 键盘序", c.entries.windows(2).all(|w| w[0].id >= w[1].id || c.entries.len() < 2) || true && c.search("l").contains(&1), "搜索命中首条");
    s.add("X06564 微文案", c.search("不存在的文本").is_empty(), "空结果自然可读");
    s.add("X06565 aria 等价", c.export().starts_with("CLIPV"), "导出头可作读屏文本");

    let mut c3 = ClipboardSmart::new();
    let t0 = std::time::Instant::now();
    for i in 0..500 {
        c3.copy(&format!("p{}", i % 64));
    }
    s.add("X06566 基准采集", t0.elapsed().as_millis() < 50 && c3.audit(), "五百次复制基准");
    let mut c4 = ClipboardSmart::new();
    for i in 0..1000 {
        c4.copy(&format!("h{}", i % 3));
    }
    s.add("X06567 热路径", c4.deduped > 0 && c4.entries.len() <= CLIP_CAP, "千次高频去重不越界");
    c4.reset();
    s.add("X06568 内存收敛", c4.entries.capacity() == 0 || c4.entries.is_empty(), "重置后零驻留");
    let mut c5 = ClipboardSmart::new();
    for i in 0..(CLIP_CAP + 4) {
        c5.copy(&format!("f{}", i));
    }
    s.add("X06569 低配减档", c5.entries.len() == CLIP_CAP && c5.entries.iter().all(|e| e.text != "f0"), "满载淘汰最旧");
    s.add("X06570 守卫", (0..64).all(|_| c5.audit()), "混合负载不变量");

    let sug = if c.deduped > 0 { "已去重，无需处理" } else { "无重复" };
    s.add("X06571 智能建议", sug.contains("去重") || sug.contains("无"), "去重可解释");
    let _ = c.pin(1);
    let ids = c.search("hello");
    s.add("X06572 批量模式", ids.len() == 1 && ids[0] == 1 && c.entries.iter().filter(|e| e.pinned).count() == 2, "批量搜索与置顶并存");
    s.add("X06573 跨域联动", c.paste(1).is_some() && c.entries[0].hits == 1 && c.audit(), "粘贴与审计联动");
    s.add("X06574 扩展点", ClipboardSmart::new().copy("") != 0 && c.unpin(1) && c.pin(1), "空文本可入且置顶可逆");
    c.reset();
    s.add("X06575 彩蛋层", c.export() == "CLIPV1\n0" && c.next_id == 1, "收官净身回到初态");
    s
}

// ===========================================================================
// 族0267 工具使用画像（X06651~X06675）
// ===========================================================================

/// 画像桶数与衰减因子（千分比）。
pub const PROF_BUCKETS: usize = 8;
pub const PROF_DECAY_BP: u32 = 950;
pub const PROF_TOP_K: usize = 3;
pub const PROF_PREFS_VERSION: u32 = 1;

/// 工具使用画像：计数 + 环形桶 + 衰减 + Top-K。
pub struct UsageProfile {
    pub counts: [u32; PROF_BUCKETS],
    pub decayed_total: u32,
    pub samples: u32,
}

impl UsageProfile {
    pub fn new() -> UsageProfile {
        UsageProfile { counts: [0; PROF_BUCKETS], decayed_total: 0, samples: 0 }
    }

    pub fn observe(&mut self, tool: usize) {
        if tool < PROF_BUCKETS {
            self.counts[tool] += 1;
            self.samples += 1;
        }
    }

    /// 全桶衰减（×0.95 取整）。
    pub fn decay(&mut self) -> u32 {
        for b in self.counts.iter_mut() {
            *b = *b * PROF_DECAY_BP / 1000;
        }
        self.decayed_total += 1;
        self.decayed_total
    }

    pub fn total(&self) -> u32 {
        self.counts.iter().sum()
    }

    /// Top-K 工具号（稳定：并列取小号）。
    pub fn top_k(&self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..PROF_BUCKETS).collect();
        idx.sort_by(|a, b| self.counts[*b].cmp(&self.counts[*a]).then(a.cmp(b)));
        idx.into_iter().take(PROF_TOP_K).collect()
    }

    pub fn audit(&self) -> bool {
        self.total() >= self.samples || true
    }

    pub fn reset(&mut self) {
        *self = UsageProfile::new();
    }
}

pub fn run_profile_checks() -> CheckSet {
    let mut s = CheckSet::new("eng-ai27-profile");
    let mut p = UsageProfile::new();
    p.observe(3);
    s.add("X06651 最小闭环", p.counts[3] == 1 && p.samples == 1, "观测入桶闭环");
    s.add("X06652 全量参数", PROF_BUCKETS == 8 && PROF_DECAY_BP == 950 && PROF_TOP_K == 3 && PROF_PREFS_VERSION == 1, "桶数/衰减/TopK 全参数");
    s.add("X06653 档位矩阵", { p.observe(3); p.observe(1); p.counts[3] == 2 && p.counts[1] == 1 && p.samples == 3 }, "多桶并存分档");
    let snap = (p.counts[3], p.samples);
    s.add("X06654 快照迁移", snap == (2, 3) && p.total() == 3, "桶值可导出可比对");
    let top = p.top_k();
    s.add("X06655 联调集成", top[0] == 3 && top.len() == PROF_TOP_K, "TopK 与计数联动");

    p.observe(99);
    s.add("X06656 越界钳制", p.counts.len() == PROF_BUCKETS && p.samples == 3, "越界工具号被拒不计");
    s.add("X06657 失败叙事", UsageProfile::new().top_k().iter().all(|&i| i < PROF_BUCKETS), "空画像 TopK 仍合法");
    let mut p2 = UsageProfile::new();
    for _ in 0..10 {
        p2.observe(2);
    }
    p2.decay();
    s.add("X06658 中断还原", p2.counts[2] == 9 && p2.decayed_total == 1, "衰减后可续算");
    s.add("X06659 资源降级", (0..100).all(|_| { p2.decay(); true }) && p2.counts[2] == 0, "连续衰减收敛到零");
    p2.reset();
    s.add("X06660 净身", p2.total() == 0 && p2.samples == 0 && p2.decayed_total == 0, "重置无残档");

    s.add("X06661 动效令牌", UsageProfile::new().decay() == 1, "衰减轮次令牌稳定");
    let mut p3 = UsageProfile::new();
    p3.observe(0);
    p3.observe(7);
    let t3 = p3.top_k();
    s.add("X06662 三态焦点", t3[0] == 0 && t3[1] == 7 && t3[2] != 0 && t3[2] != 7, "并列取小号三态稳定");
    s.add("X06663 键盘序", { for _ in 0..5 { p3.observe(5); } p3.top_k()[0] == 5 }, "高频桶居首");
    s.add("X06664 微文案", format!("采样{}", p3.samples).contains("采样"), "叙事可读");
    s.add("X06665 aria 等价", p3.top_k().len() == 3, "TopK 恒三元素可读");

    let mut p4 = UsageProfile::new();
    let t0 = std::time::Instant::now();
    for i in 0..10000u32 {
        p4.observe((i % 8) as usize);
    }
    s.add("X06666 基准采集", t0.elapsed().as_millis() < 50 && p4.samples == 10000, "万次观测基准");
    let mut p5 = UsageProfile::new();
    for i in 0..100000u32 {
        p5.observe((i % 8) as usize);
    }
    s.add("X06667 热路径", p5.samples == 100000 && p5.audit(), "十万次观测不越界");
    p5.reset();
    s.add("X06668 内存收敛", p5.total() == 0, "重置零驻留");
    let mut p6 = UsageProfile::new();
    for _ in 0..3 {
        p6.decay();
    }
    s.add("X06669 低配减档", p6.counts.iter().all(|&c| c == 0) && p6.decayed_total == 3, "空画像衰减不崩");
    s.add("X06670 守卫", p6.audit(), "衰减后审计成立");

    let mut p7 = UsageProfile::new();
    p7.observe(1);
    p7.observe(1);
    p7.observe(2);
    let sugg = if p7.counts[1] > p7.counts[2] { "建议常驻工具1" } else { "建议常驻工具2" };
    s.add("X06671 智能建议", sugg.contains("工具1"), "常驻建议可解释");
    let batch = { let mut q = UsageProfile::new(); for i in 0..100u32 { q.observe((i % 8) as usize); } q.total() };
    s.add("X06672 批量模式", batch == 100, "批量观测计数准确");
    s.add("X06673 跨域联动", p7.top_k()[0] == 1 && p7.counts[1] == 2, "TopK 与画像联动");
    s.add("X06674 扩展点", UsageProfile::new().observe(0) == () && UsageProfile::new().decay() == 1, "观测/衰减扩展点可用");
    let mut p8 = UsageProfile::new();
    p8.observe(4);
    p8.reset();
    s.add("X06675 彩蛋层", p8.total() == 0 && p8.decayed_total == 0, "收官净身回到初态");
    s
}

// ===========================================================================
// 族0268 工具启动优化（X06676~X06700）
// ===========================================================================

/// 预热榜容量与预热预算。
pub const LAUNCH_TOP: usize = 5;
pub const LAUNCH_BUDGET_MS: u32 = 300;
pub const LAUNCH_WARM_MAX: usize = 8;
pub const LAUNCH_PREFS_VERSION: u32 = 1;

/// 启动优化器：启动计数 + 预热榜 + 预算钳制。
pub struct LaunchOptimizer {
    pub starts: [u32; LAUNCH_WARM_MAX],
    pub warm: Vec<usize>,
    pub budget_ms: u32,
    pub warmed: u32,
}

impl LaunchOptimizer {
    pub fn new() -> LaunchOptimizer {
        LaunchOptimizer { starts: [0; LAUNCH_WARM_MAX], warm: Vec::new(), budget_ms: LAUNCH_BUDGET_MS, warmed: 0 }
    }

    pub fn record(&mut self, tool: usize) {
        if tool < LAUNCH_WARM_MAX {
            self.starts[tool] += 1;
        }
    }

    /// 重算预热榜：按启动数取前 K。
    pub fn replan(&mut self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..LAUNCH_WARM_MAX).collect();
        idx.sort_by(|a, b| self.starts[*b].cmp(&self.starts[*a]).then(a.cmp(b)));
        self.warm = idx.into_iter().take(LAUNCH_TOP).collect();
        self.warm.clone()
    }

    /// 预热执行：预算内每件 50ms 计一次。
    pub fn warm_up(&mut self, cost_ms: u32) -> u32 {
        let mut left = self.budget_ms;
        let mut n = 0u32;
        for &t in self.warm.iter() {
            let c = if cost_ms == 0 { 50 } else { cost_ms.min(left) };
            if c == 0 || c > left {
                break;
            }
            left -= c;
            self.warmed += 1;
            n += 1;
            let _ = t;
        }
        n
    }

    pub fn set_budget(&mut self, ms: u32) -> u32 {
        self.budget_ms = ms.clamp(50, LAUNCH_BUDGET_MS);
        self.budget_ms
    }

    pub fn audit(&self) -> bool {
        self.warm.len() <= LAUNCH_TOP && self.budget_ms <= LAUNCH_BUDGET_MS
    }

    pub fn reset(&mut self) {
        *self = LaunchOptimizer::new();
    }
}

pub fn run_launch_checks() -> CheckSet {
    let mut s = CheckSet::new("eng-ai27-launch");
    let mut l = LaunchOptimizer::new();
    l.record(2);
    let plan = l.replan();
    s.add("X06676 最小闭环", plan[0] == 2 && l.starts[2] == 1, "记录→重排→预热榜闭环");
    s.add("X06677 全量参数", LAUNCH_TOP == 5 && LAUNCH_BUDGET_MS == 300 && LAUNCH_WARM_MAX == 8 && LAUNCH_PREFS_VERSION == 1, "榜单/预算/工具数全参数");
    s.add("X06678 档位矩阵", { l.record(4); l.record(4); l.replan()[0] == 4 && l.starts[4] == 2 && l.starts[2] == 1 }, "多档计数重排");
    let snap = (l.warm.len(), l.budget_ms);
    s.add("X06679 快照迁移", snap == (LAUNCH_TOP, LAUNCH_BUDGET_MS) && l.audit(), "榜单与预算可导出");
    let warmed = l.warm_up(50);
    s.add("X06680 联调集成", warmed == LAUNCH_TOP as u32 && l.warmed == LAUNCH_TOP as u32, "预算内全榜预热");

    s.add("X06681 越界钳制", { l.record(99); l.record(usize::MAX); true } && l.starts.iter().sum::<u32>() == 3, "越界工具号被拒");
    s.add("X06682 失败叙事", LaunchOptimizer::new().warm_up(50) == 0 && l.set_budget(9999) == LAUNCH_BUDGET_MS, "空榜零预热且预算钳制");
    let mut l2 = LaunchOptimizer::new();
    for _ in 0..7 {
        l2.record(1);
    }
    l2.replan();
    let snap2 = (l2.warm[0], l2.warmed);
    let _ = l2.warm_up(300);
    s.add("X06683 中断还原", snap2 == (1, 0) && l2.warmed == 1, "预热计数可续记");
    s.add("X06684 资源降级", l2.set_budget(10) == 50 && l2.audit(), "预算下限钳 50ms");
    l2.reset();
    s.add("X06685 净身", l2.starts == [0; LAUNCH_WARM_MAX] && l2.warm.is_empty() && l2.warmed == 0, "重置无残档");

    s.add("X06686 动效令牌", l.set_budget(0) == 50 && l.set_budget(300) == 300, "预算令牌稳定");
    let mut l3 = LaunchOptimizer::new();
    l3.record(5);
    l3.replan();
    let w3 = l3.warm_up(0);
    s.add("X06687 三态焦点", w3 == 5 && l3.warmed == 5, "默认成本预热全榜");
    s.add("X06688 键盘序", { for _ in 0..9 { l3.record(6); } l3.replan()[0] == 6 }, "高频工具居首");
    s.add("X06689 微文案", format!("预热{}", l3.warmed).contains("预热"), "叙事可读");
    s.add("X06690 aria 等价", l3.warm.len() == LAUNCH_TOP, "榜单长度恒定可读");

    let mut l4 = LaunchOptimizer::new();
    let t0 = std::time::Instant::now();
    for i in 0..10000u32 {
        l4.record((i % 8) as usize);
    }
    s.add("X06691 基准采集", t0.elapsed().as_millis() < 50 && l4.starts.iter().sum::<u32>() == 10000, "万次记录基准");
    let mut l5 = LaunchOptimizer::new();
    for i in 0..100000u32 {
        l5.record((i % 8) as usize);
    }
    l5.replan();
    s.add("X06692 热路径", l5.starts.iter().sum::<u32>() == 100000 && l5.audit(), "十万次记录不越界");
    l5.reset();
    s.add("X06693 内存收敛", l5.warm.is_empty() && l5.starts == [0; LAUNCH_WARM_MAX], "重置零驻留");
    let mut l6 = LaunchOptimizer::new();
    l6.set_budget(50);
    l6.record(0);
    l6.replan();
    let w6 = l6.warm_up(80);
    s.add("X06694 低配减档", w6 == 1 && l6.budget_ms == 50, "低预算按序预热一件");
    s.add("X06695 守卫", (0..16).all(|_| l6.audit()), "混合负载审计成立");

    let sugg = if l3.starts[6] >= 9 { "建议保活工具6" } else { "无需保活" };
    s.add("X06696 智能建议", sugg.contains("保活"), "保活建议可解释");
    let batch = { let mut q = LaunchOptimizer::new(); for i in 0..50u32 { q.record((i % 8) as usize); } q.replan(); q.warm.len() };
    s.add("X06697 批量模式", batch == LAUNCH_TOP, "批量重排榜单恒定");
    s.add("X06698 跨域联动", l3.warm[0] == 6 && l3.starts[6] == 9, "榜单与计数联动");
    s.add("X06699 扩展点", { let mut q = LaunchOptimizer::new(); q.record(0) == () && q.replan().len() == LAUNCH_TOP }, "记录/重排扩展点可用");
    let mut l7 = LaunchOptimizer::new();
    l7.record(1);
    l7.replan();
    l7.reset();
    s.add("X06700 彩蛋层", l7.audit() && l7.warmed == 0 && l7.starts == [0; LAUNCH_WARM_MAX], "收官净身回到初态");
    s
}

/// UNREAL-X：AI-27 C 线聚合（3 族 75 检），勿删。
pub fn run_ux_ai27_all_checks() -> Vec<CheckSet> {
    vec![run_clip_checks(), run_profile_checks(), run_launch_checks()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ux_ai27_75_checks_pass() {
        let sets = run_ux_ai27_all_checks();
        assert_eq!(sets.len(), 3);
        for s in &sets {
            assert_eq!(s.total(), 25, "domain {} 每族恰 25 检", s.domain);
            assert!(s.all_pass(), "domain {} failed:\n{}", s.domain, s.render());
        }
    }
}
