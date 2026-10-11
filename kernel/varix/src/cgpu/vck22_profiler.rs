//! CGPU-F1703 · 异构性能剖析
//!
//! 判据锚点（施工书原文摘录）：
//! 剖析：剖析器（异构剖析（引擎级采样/交接热点——剖析器（热点报告（交接/等待/
//! 切换 TOP——热点报告；火焰（异构火焰（时间线火焰融合——融合视图；建议（规则化
//! 建议（异构 8 条——规则库复用扩展；测试（剖析器/报告/融合/规则四组）。判据：
//! 剖析器、热点、融合、规则 8、四组、判据。
//!
//! 诊断码段 0x8A01~0x8A05（独占）：
//!   0x8A01 剖析采样失格（样本数不足出热点/引擎序越界/时长负值）
//!   0x8A02 热点报告失序（TOP 排序不单调/报告空窗）
//!   0x8A03 火焰融合失真（融合深度越栈限/子段超父段时长）
//!   0x8A04 建议规则失锚（建议未挂规则库 id/规则不在册 8 条）
//!   0x8A05 段越界（判码落 0x8A00..0x8B00 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

use alloc::vec::Vec;

/// 模块版本。
pub const PROFILER_VERSION: &str = "K22-profiler-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0x8A01~0x8A05）
// ---------------------------------------------------------------------------

pub const XC_SAMPLE_MALFORMED: u32 = 0x8A01;
pub const XC_HOTSET_UNORDERED: u32 = 0x8A02;
pub const XC_FLAME_DISTORTED: u32 = 0x8A03;
pub const XC_RULE_UNANCHORED: u32 = 0x8A04;
pub const XC_SEG_OOR: u32 = 0x8A05;

// ---------------------------------------------------------------------------
// 一、剖析器：引擎级采样 + 交接热点
// ---------------------------------------------------------------------------

/// 剖析样本（引擎级——一引擎一帧一次采样）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    /// 引擎序 0..=3。
    pub engine: u8,
    /// 帧号。
    pub frame: u32,
    /// 占用 µs。
    pub busy_us: u64,
    /// 该帧交接次数（跨引擎 handoff 计数）。
    pub handoffs: u32,
    /// 该帧等待 µs。
    pub wait_us: u64,
}

/// 剖析器：采样入册 + 引擎聚合 + 三类热点 TOP。
pub struct Profiler {
    samples: Vec<Sample>,
}

impl Profiler {
    pub fn new() -> Self {
        Profiler { samples: Vec::new() }
    }

    /// 入样：引擎序越界/时长越帧预算（12500µs）拒。
    pub fn feed(&mut self, s: Sample) -> Result<(), u32> {
        if s.engine > 3 || s.busy_us > 12_500 || s.wait_us > 12_500 {
            return Err(XC_SAMPLE_MALFORMED);
        }
        self.samples.push(s);
        Ok(())
    }

    /// 样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 引擎聚合：某引擎全部样本合计（交接 µs 语义=交接次数×常量折算不入聚合，
    /// 聚合三轴=busy/wait/handoffs 原始量）。
    pub fn engine_agg(&self, engine: u8) -> (u64, u64, u64) {
        let mut busy = 0;
        let mut wait = 0;
        let mut ho = 0u64;
        for s in &self.samples {
            if s.engine == engine {
                busy += s.busy_us;
                wait += s.wait_us;
                ho += s.handoffs as u64;
            }
        }
        (busy, wait, ho)
    }

    /// 热点报告：按轴取 TOP-N 引擎（判据"交接/等待/切换 TOP"）。
    /// 轴 0=交接次数 / 1=等待 µs / 2=切换（占用µs 倒序=最忙即切换压力热点）。
    pub fn hot_top(&self, axis: usize, n: usize) -> Result<Vec<(u8, u64)>, u32> {
        if axis > 2 || n == 0 || self.samples.is_empty() {
            return Err(XC_HOTSET_UNORDERED);
        }
        let mut rows: Vec<(u8, u64)> = Vec::new();
        let mut eng = 0;
        while eng <= 3 {
            let agg = self.engine_agg(eng);
            let v = match axis {
                0 => agg.2,
                1 => agg.1,
                _ => agg.0,
            };
            rows.push((eng, v));
            eng += 1;
        }
        // 选择排序降序（TOP-N 截断）。
        let mut i = 0;
        while i < n.min(rows.len()) {
            let mut best = i;
            let mut j = i + 1;
            while j < rows.len() {
                if rows[j].1 > rows[best].1 {
                    best = j;
                }
                j += 1;
            }
            let t = rows[i];
            rows[i] = rows[best];
            rows[best] = t;
            i += 1;
        }
        rows.truncate(n);
        // 报告失序守卫：输出必须降序单调（并列允许）。
        let mut k = 1;
        while k < rows.len() {
            if rows[k].1 > rows[k - 1].1 {
                return Err(XC_HOTSET_UNORDERED);
            }
            k += 1;
        }
        Ok(rows)
    }
}

// ---------------------------------------------------------------------------
// 二、火焰融合视图
// ---------------------------------------------------------------------------

/// 火焰节点（跨引擎时间线融合：父子段时长包含关系）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FlameNode {
    pub engine: u8,
    /// 段名（闭集引用 C 域段枚举）。
    pub seg: &'static str,
    pub start_us: u64,
    pub end_us: u64,
    /// 深度（0=根帧）。
    pub depth: u8,
}

/// 火焰融合器：深度栈限 + 包含校验。
pub struct FlameMerger {
    nodes: Vec<FlameNode>,
}

/// 栈深上限（融合视图最深 4 层——帧/引擎/段/交接）。
pub const FLAME_MAX_DEPTH: u8 = 4;

impl FlameMerger {
    pub fn new() -> Self {
        FlameMerger { nodes: Vec::new() }
    }

    /// 入节点：深度越栈限拒 / 时长负拒 / 子段超父段拒（同引擎深度-1 包含检查）。
    pub fn add(&mut self, n: FlameNode) -> Result<(), u32> {
        if n.depth >= FLAME_MAX_DEPTH || n.end_us <= n.start_us || n.engine > 3 {
            return Err(XC_FLAME_DISTORTED);
        }
        if n.depth > 0 {
            // 父查找：同引擎、深度-1、时间包含。
            let mut parent = None;
            for p in &self.nodes {
                if p.engine == n.engine
                    && p.depth == n.depth - 1
                    && p.start_us <= n.start_us
                    && n.end_us <= p.end_us
                {
                    parent = Some(*p);
                }
            }
            if parent.is_none() {
                return Err(XC_FLAME_DISTORTED);
            }
        }
        self.nodes.push(n);
        Ok(())
    }

    /// 节点数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 融合失真自查：任一子段不超父段时长（全表复核）。
    pub fn integrity(&self) -> Result<(), u32> {
        for n in &self.nodes {
            if n.depth > 0 {
                let mut covered = false;
                for p in &self.nodes {
                    if p.engine == n.engine
                        && p.depth == n.depth - 1
                        && p.start_us <= n.start_us
                        && n.end_us <= p.end_us
                    {
                        covered = true;
                    }
                }
                if !covered {
                    return Err(XC_FLAME_DISTORTED);
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、规则化建议库（异构 8 条闭集）
// ---------------------------------------------------------------------------

/// 规则库 8 条闭集（判据"异构 8 条——规则库复用扩展"）。
pub const RULES: [&str; 8] = [
    "R1-npu-underuse",     // NPU 利用率低：推理任务迁移建议
    "R2-handoff-heavy",    // 交接热点：合并小任务降交接
    "R3-wait-cpu-on-gpu",  // CPU 长 GPU fence：预提交流水
    "R4-dsp-idle-while-queue", // DSP 空转而队列深：音频任务再分配
    "R5-mgpu-imbalance",   // 多 GPU 失衡：分割比调权
    "R6-bandwidth-clash",  // 带宽互扰：错峰调度
    "R7-thermal-throttle", // 热降档：能效路由换点
    "R8-fallback-storm",   // 回退风暴：能力探测刷新
];

/// 建议行：挂规则 id（不在册 8 条即 0x8A04）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Advice {
    pub rule_id: &'static str,
    pub engine: u8,
    /// 建议正文短句。
    pub text: &'static str,
}

/// 建议器。
pub struct Adviser;

impl Adviser {
    /// 规则在册校验。
    pub fn rule_exists(id: &str) -> bool {
        let mut found = false;
        let mut i = 0;
        while i < RULES.len() {
            if RULES[i] == id {
                found = true;
            }
            i += 1;
        }
        found
    }

    /// 产建议：规则不在册拒。
    pub fn emit(rule_id: &'static str, engine: u8, text: &'static str) -> Result<Advice, u32> {
        if !Self::rule_exists(rule_id) || engine > 3 {
            return Err(XC_RULE_UNANCHORED);
        }
        Ok(Advice { rule_id, engine, text })
    }

    /// 规则库容量恰 8（判据"异构 8 条"）。
    pub const fn rule_count() -> usize {
        8
    }
}

// ---------------------------------------------------------------------------
// 自检（四组：剖析器/报告/融合/规则）
// ---------------------------------------------------------------------------

/// 剖析引擎。
pub struct ProfEngine {
    pub p: Profiler,
    pub flame: FlameMerger,
}

impl ProfEngine {
    pub fn new() -> Self {
        ProfEngine { p: Profiler::new(), flame: FlameMerger::new() }
    }

    /// 样本种子：GPU 忙 4200µs/交接 3 次、NPU 忙 2600µs/交接 1 次、CPU 忙 800µs。
    pub fn seed(&mut self) -> Result<(), u32> {
        self.p.feed(Sample { engine: 1, frame: 1, busy_us: 4200, handoffs: 3, wait_us: 0 })?;
        self.p.feed(Sample { engine: 1, frame: 2, busy_us: 4000, handoffs: 2, wait_us: 100 })?;
        self.p.feed(Sample { engine: 2, frame: 1, busy_us: 2600, handoffs: 1, wait_us: 400 })?;
        self.p.feed(Sample { engine: 0, frame: 1, busy_us: 800, handoffs: 0, wait_us: 1800 })?;
        Ok(())
    }

    /// 组一：剖析器——采样入册 + 越界拒 + 越预算拒 + 聚合对拍。
    pub fn selfcheck_sampler(&mut self) -> Result<(), u32> {
        self.seed()?;
        if self.p.len() != 4 {
            return Err(XC_SAMPLE_MALFORMED);
        }
        // 引擎越界拒。
        if self.p.feed(Sample { engine: 4, frame: 9, busy_us: 1, handoffs: 0, wait_us: 0 }).is_ok() {
            return Err(XC_SAMPLE_MALFORMED);
        }
        // 越帧预算拒。
        if self.p.feed(Sample { engine: 1, frame: 9, busy_us: 12_501, handoffs: 0, wait_us: 0 }).is_ok() {
            return Err(XC_SAMPLE_MALFORMED);
        }
        // 聚合对拍：GPU busy=8200 wait=100 ho=5。
        let agg = self.p.engine_agg(1);
        if agg != (8200, 100, 5) {
            return Err(XC_SAMPLE_MALFORMED);
        }
        Ok(())
    }

    /// 组二：热点报告——三轴 TOP 有序 + 空窗拒 + 失序守卫。
    pub fn selfcheck_hotset(&mut self) -> Result<(), u32> {
        self.seed()?;
        // 交接轴 TOP2：GPU(5) > NPU(1)。
        let top = self.p.hot_top(0, 2)?;
        if top.len() != 2 || top[0].0 != 1 || top[0].1 != 5 {
            return Err(XC_HOTSET_UNORDERED);
        }
        // 等待轴 TOP1：CPU(1800)。
        let top1 = self.p.hot_top(1, 1)?;
        if top1[0].0 != 0 || top1[0].1 != 1800 {
            return Err(XC_HOTSET_UNORDERED);
        }
        // 空窗拒。
        let empty = Profiler::new();
        if empty.hot_top(0, 1).is_ok() {
            return Err(XC_HOTSET_UNORDERED);
        }
        // 轴越界拒。
        if self.p.hot_top(3, 1).is_ok() {
            return Err(XC_HOTSET_UNORDERED);
        }
        Ok(())
    }

    /// 组三：火焰融合——包含层级 + 子超父拒 + 越栈限拒。
    pub fn selfcheck_flame(&mut self) -> Result<(), u32> {
        // 根：GPU 帧。
        self.flame.add(FlameNode { engine: 1, seg: "frame", start_us: 0, end_us: 4200, depth: 0 })?;
        // 子：GPU render 段。
        self.flame.add(FlameNode { engine: 1, seg: "render", start_us: 100, end_us: 3800, depth: 1 })?;
        // 孙：GPU render 内 handoff 片。
        self.flame.add(FlameNode { engine: 1, seg: "handoff", start_us: 200, end_us: 400, depth: 2 })?;
        // 子超父反例：render 内段超出 render 终点。
        if self.flame.add(FlameNode { engine: 1, seg: "render", start_us: 3900, end_us: 4300, depth: 2 }).is_ok() {
            return Err(XC_FLAME_DISTORTED);
        }
        // 越栈限反例：depth=4。
        if self.flame.add(FlameNode { engine: 1, seg: "x", start_us: 0, end_us: 10, depth: 4 }).is_ok() {
            return Err(XC_FLAME_DISTORTED);
        }
        // 孤儿反例：NPU depth=1 无父。
        if self.flame.add(FlameNode { engine: 2, seg: "compute", start_us: 100, end_us: 200, depth: 1 }).is_ok() {
            return Err(XC_FLAME_DISTORTED);
        }
        // 全表完整性。
        self.flame.integrity()?;
        if self.flame.len() != 3 {
            return Err(XC_FLAME_DISTORTED);
        }
        Ok(())
    }

    /// 组四：规则库——8 条闭集 + 建议挂锚 + 野规则拒。
    pub fn selfcheck_rules(&mut self) -> Result<(), u32> {
        if Adviser::rule_count() != 8 {
            return Err(XC_RULE_UNANCHORED);
        }
        // 全部规则 id 均在册（库自洽）。
        for r in RULES {
            if !Adviser::rule_exists(r) {
                return Err(XC_RULE_UNANCHORED);
            }
        }
        // 合法建议。
        let a = Adviser::emit("R2-handoff-heavy", 1, "merge small tasks")?;
        if a.engine != 1 {
            return Err(XC_RULE_UNANCHORED);
        }
        // 野规则拒。
        if Adviser::emit("R9-wild", 1, "x").is_ok() {
            return Err(XC_RULE_UNANCHORED);
        }
        // 引擎越界拒。
        if Adviser::emit("R1-npu-underuse", 5, "x").is_ok() {
            return Err(XC_RULE_UNANCHORED);
        }
        Ok(())
    }

    /// 全量自检入口（四组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        Self::segment_guard()?;
        self.selfcheck_sampler()?;
        self.selfcheck_hotset()?;
        self.selfcheck_flame()?;
        self.selfcheck_rules()?;
        Ok(())
    }

    /// 段对账：五码全落 0x8A00..0x8B00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_SAMPLE_MALFORMED,
            XC_HOTSET_UNORDERED,
            XC_FLAME_DISTORTED,
            XC_RULE_UNANCHORED,
            XC_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x8A00 || codes[i] >= 0x8B00 {
                return Err(XC_SAMPLE_MALFORMED);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_SAMPLE_MALFORMED);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
