//! CGPU-F1700 · 异构时间线调试器
//!
//! 判据锚点（施工书原文摘录）：
//! 时间线调试：统一时间线（全引擎任务 Gantt（K04 对齐视图产品化——产品化（
//! 时间线 UI 数据接口；跨引擎追踪（任务跨引擎全旅程追踪（提交→排队→执行→
//! 交接→完成——旅程追踪（旅程可视化；交互（时间轴缩放/过滤/选择——交互规范；
//! 与 M/O（跨卷时间线衔接（VE 时间线对齐——跨卷对齐预留；测试（产品化/旅程/
//! 交互/衔接四组）。判据：Gantt、旅程追踪、交互、跨卷、四组、判据。
//!
//! 诊断码段 0x8701~0x8705（独占）：
//!   0x8701 Gantt 产品化失格（条目越窗/重叠同引擎同时刻/负时长）
//!   0x8702 旅程断链（五阶段缺项/时间戳回退/交接无对端）
//!   0x8703 交互越范（缩放比越界/过滤引擎不在册/选择落空不显性）
//!   0x8704 跨卷衔接失锚（VE 时间线锚点漂移/基准时钟不统一）
//!   0x8705 段越界（判码落 0x8700..0x8800 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

use alloc::vec::Vec;

/// 模块版本。
pub const TL_DEBUG_VERSION: &str = "K19-tldebug-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0x8701~0x8705）
// ---------------------------------------------------------------------------

pub const XC_GANTT_MALFORMED: u32 = 0x8701;
pub const XC_JOURNEY_BROKEN: u32 = 0x8702;
pub const XC_INTERACT_OOR: u32 = 0x8703;
pub const XC_XVOL_UNANCHORED: u32 = 0x8704;
pub const XC_SEG_OOR: u32 = 0x8705;

// ---------------------------------------------------------------------------
// 一、统一时间线 Gantt（K04 对齐视图产品化）
// ---------------------------------------------------------------------------

/// 时间线条目（单引擎单任务一段占用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GanttBar {
    /// 任务 id。
    pub task: u32,
    /// 占用引擎（0=CPU 1=GPU 2=NPU 3=DSP——vck18 Eng 序复用）。
    pub engine: usize,
    /// 起点 µs（统一单调时钟，F1442 复用）。
    pub start_us: u64,
    /// 终点 µs（>start）。
    pub end_us: u64,
}

impl GanttBar {
    pub fn span_us(&self) -> u64 {
        self.end_us.saturating_sub(self.start_us)
    }
}

/// Gantt 产品：条目在册 + 窗口查询 + 同引擎重叠检测。
pub struct Gantt {
    bars: Vec<GanttBar>,
}

impl Gantt {
    pub fn new() -> Self {
        Gantt { bars: Vec::new() }
    }

    /// 入条目：负时长拒（0x8701）；同引擎时间重叠拒（Gantt 语义：一引擎
    /// 同一刻最多一段占用——超订阅任务走排队表示，不画重叠）。
    pub fn add(&mut self, bar: GanttBar) -> Result<(), u32> {
        if bar.end_us <= bar.start_us || bar.engine > 3 {
            return Err(XC_GANTT_MALFORMED);
        }
        for b in &self.bars {
            if b.engine == bar.engine && bar.start_us < b.end_us && b.start_us < bar.end_us {
                return Err(XC_GANTT_MALFORMED);
            }
        }
        self.bars.push(bar);
        Ok(())
    }

    /// 窗口查询（UI 数据接口核心）：返回与 [w0,w1) 相交的全部条目（值拷贝
    /// 出栈，UI 侧不需要借构内部）。
    pub fn window(&self, w0: u64, w1: u64) -> Vec<GanttBar> {
        let mut out = Vec::new();
        if w1 <= w0 {
            return out;
        }
        for b in &self.bars {
            if b.start_us < w1 && w0 < b.end_us {
                out.push(*b);
            }
        }
        out
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.bars.len()
    }

    /// 查条目。
    pub fn bar(&self, idx: usize) -> Option<&GanttBar> {
        self.bars.get(idx)
    }
}

// ---------------------------------------------------------------------------
// 二、旅程追踪：提交→排队→执行→交接→完成 五阶段
// ---------------------------------------------------------------------------

/// 旅程阶段五闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Submit,
    Queue,
    Execute,
    Handoff,
    Done,
}

impl Stage {
    pub const ALL: [Stage; 5] =
        [Stage::Submit, Stage::Queue, Stage::Execute, Stage::Handoff, Stage::Done];
    pub const fn ordinal(self) -> usize {
        match self {
            Stage::Submit => 0,
            Stage::Queue => 1,
            Stage::Execute => 2,
            Stage::Handoff => 3,
            Stage::Done => 4,
        }
    }
    pub const fn name(self) -> &'static str {
        match self {
            Stage::Submit => "submit",
            Stage::Queue => "queue",
            Stage::Execute => "execute",
            Stage::Handoff => "handoff",
            Stage::Done => "done",
        }
    }
}

/// 单任务旅程：五阶段时间戳（统一单调时钟 µs）+ 交接对端（跨引擎任务才有）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Journey {
    pub task: u32,
    /// 五阶段时间戳（按 Stage::ALL 序）。
    pub ts: [u64; 5],
    /// 交接对端任务（Handoff 段跨引擎时的下游任务 id；纯本引擎任务 None）。
    pub handoff_to: Option<u32>,
    /// 起引擎/终引擎（跨引擎旅程标注）。
    pub from_engine: usize,
    pub to_engine: usize,
}

/// 旅程账：录入 + 闭合校验 + 可视化数据出口。
pub struct JourneyLedger {
    rows: Vec<Journey>,
}

impl JourneyLedger {
    pub fn new() -> Self {
        JourneyLedger { rows: Vec::new() }
    }

    /// 录入旅程：五阶段时间戳单调不减校验（回退=0x8702）+ 引擎序合法。
    pub fn enroll(&mut self, j: Journey) -> Result<(), u32> {
        if j.from_engine > 3 || j.to_engine > 3 {
            return Err(XC_JOURNEY_BROKEN);
        }
        let mut i = 1;
        while i < 5 {
            if j.ts[i] < j.ts[i - 1] {
                return Err(XC_JOURNEY_BROKEN);
            }
            i += 1;
        }
        // 跨引擎旅程必须声明对端；本引擎旅程不得有对端（对端=交接证据）。
        if j.from_engine != j.to_engine && j.handoff_to.is_none() {
            return Err(XC_JOURNEY_BROKEN);
        }
        if j.from_engine == j.to_engine && j.handoff_to.is_some() {
            return Err(XC_JOURNEY_BROKEN);
        }
        self.rows.push(j);
        Ok(())
    }

    /// 任务总耗时（Done-Submit）。
    pub fn span_us(&self, task: u32) -> Result<u64, u32> {
        for j in &self.rows {
            if j.task == task {
                return Ok(j.ts[4].saturating_sub(j.ts[0]));
            }
        }
        Err(XC_JOURNEY_BROKEN)
    }

    /// 各阶段停留 µs（Queue-Submit / Execute-Queue / Handoff-Execute /
    /// Done-Handoff——交接即排队让位，Done 即旅程终点）。
    pub fn stage_breakdown(&self, task: u32) -> Result<[u64; 4], u32> {
        for j in &self.rows {
            if j.task == task {
                return Ok([
                    j.ts[1].saturating_sub(j.ts[0]),
                    j.ts[2].saturating_sub(j.ts[1]),
                    j.ts[3].saturating_sub(j.ts[2]),
                    j.ts[4].saturating_sub(j.ts[3]),
                ]);
            }
        }
        Err(XC_JOURNEY_BROKEN)
    }

    /// 旅程可视化数据出口：全部旅程行（呈现面直接读）。
    pub fn snapshot(&self) -> &[Journey] {
        &self.rows
    }

    /// 跨引擎旅程计数（交接语义的量化呈现）。
    pub fn cross_engine_count(&self) -> usize {
        self.rows.iter().filter(|j| j.from_engine != j.to_engine).count()
    }
}

// ---------------------------------------------------------------------------
// 三、交互规范：缩放 / 过滤 / 选择
// ---------------------------------------------------------------------------

/// 视口交互状态（判据"时间轴缩放/过滤/选择——交互规范"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Viewport {
    /// 窗口起点 µs。
    pub w0: u64,
    /// 窗口宽 µs（缩放=改宽度，>0）。
    pub width_us: u64,
    /// 过滤引擎位图（bit0..3=CPU/GPU/NPU/DSP；0=全显）。
    pub engine_mask: u8,
}

impl Viewport {
    /// 缩放：倍率万分比（5000=缩小一半 / 20000=放大两倍），越界拒。
    pub fn zoom(&mut self, factor_bp: u32) -> Result<(), u32> {
        if factor_bp < 1_000 || factor_bp > 100_000 {
            return Err(XC_INTERACT_OOR);
        }
        let nw = self.width_us * factor_bp as u64 / 10_000;
        if nw == 0 {
            return Err(XC_INTERACT_OOR);
        }
        self.width_us = nw;
        Ok(())
    }

    /// 过滤：引擎位图校验（>0x0F 越界拒）。
    pub fn set_engine_mask(&mut self, mask: u8) -> Result<(), u32> {
        if mask > 0x0F {
            return Err(XC_INTERACT_OOR);
        }
        self.engine_mask = mask;
        Ok(())
    }

    /// 选择：命中查询——条目中心点落在窗口内且引擎过过滤闸才算选中。
    pub fn select(&self, b: &GanttBar) -> Result<u32, u32> {
        let mid = b.start_us + b.span_us() / 2;
        let in_win = mid >= self.w0 && mid < self.w0 + self.width_us;
        let pass = self.engine_mask == 0 || (self.engine_mask >> b.engine) & 1 == 1;
        if in_win && pass {
            Ok(b.task)
        } else {
            Err(XC_INTERACT_OOR)
        }
    }
}

// ---------------------------------------------------------------------------
// 四、跨卷时间线衔接（M/O 预留：VE 时间线对齐）
// ---------------------------------------------------------------------------

/// 跨卷桥：K07 时间线与 VE 时间线的对齐锚。
pub struct XVolBridge {
    /// 对齐基准时钟 id（两卷必须同源——F1442 单调时钟）。
    pub clock_id: &'static str,
    /// VE 侧锚点偏移 µs（K07 时基→VE 时基平移量，预留位 0=同源无平移）。
    pub ve_offset_us: u64,
    /// 桥已立。
    pub anchored: bool,
}

impl XVolBridge {
    pub fn build() -> Self {
        XVolBridge { clock_id: "F1442_MONO", ve_offset_us: 0, anchored: true }
    }

    /// 衔接校验：锚立 + 时钟同源 + 偏移有界（平移不得超过 1s——跨卷窗口
    /// 对齐精度红线）。
    pub fn verified(&self) -> Result<(), u32> {
        if !self.anchored || self.clock_id.is_empty() {
            return Err(XC_XVOL_UNANCHORED);
        }
        if self.ve_offset_us > 1_000_000 {
            return Err(XC_XVOL_UNANCHORED);
        }
        Ok(())
    }

    /// K07 时间戳 → VE 时间戳（平移，预留接口）。
    pub fn to_ve_time(&self, t_us: u64) -> u64 {
        t_us.saturating_add(self.ve_offset_us)
    }
}

// ---------------------------------------------------------------------------
// 自检（四组：产品化/旅程/交互/衔接）
// ---------------------------------------------------------------------------

/// 时间线调试引擎。
pub struct TlDebugEngine {
    pub gantt: Gantt,
    pub journeys: JourneyLedger,
}

impl TlDebugEngine {
    pub fn new() -> Self {
        TlDebugEngine { gantt: Gantt::new(), journeys: JourneyLedger::new() }
    }

    /// 样本帧：GPU 渲染段 + CPU 提交段 + NPU 推理段（无重叠合法布局）。
    pub fn seed_sample(&mut self) -> Result<(), u32> {
        self.gantt.add(GanttBar { task: 1, engine: 1, start_us: 0, end_us: 4200 })?;
        self.gantt.add(GanttBar { task: 2, engine: 0, start_us: 0, end_us: 800 })?;
        self.gantt.add(GanttBar { task: 3, engine: 2, start_us: 800, end_us: 3400 })?;
        Ok(())
    }

    /// 组一：Gantt 产品化——窗口查询 + 同引擎重叠拒 + 负时长拒。
    pub fn selfcheck_product(&mut self) -> Result<(), u32> {
        self.seed_sample()?;
        if self.gantt.len() != 3 {
            return Err(XC_GANTT_MALFORMED);
        }
        // 窗口 [0, 1000)：命中 task2(CPU 全段) 与 task1(GPU 头段) 与 task3(NPU 头段)。
        let hits = self.gantt.window(0, 1000);
        if hits.len() != 3 {
            return Err(XC_GANTT_MALFORMED);
        }
        // 同引擎重叠反例：GPU [1000,2000) 与已录 [0,4200) 重叠。
        if self.gantt.add(GanttBar { task: 9, engine: 1, start_us: 1000, end_us: 2000 }).is_ok() {
            return Err(XC_GANTT_MALFORMED);
        }
        // 负时长反例。
        if self.gantt.add(GanttBar { task: 9, engine: 3, start_us: 5000, end_us: 4000 }).is_ok() {
            return Err(XC_GANTT_MALFORMED);
        }
        // 引擎越界反例。
        if self.gantt.add(GanttBar { task: 9, engine: 4, start_us: 5000, end_us: 6000 }).is_ok() {
            return Err(XC_GANTT_MALFORMED);
        }
        Ok(())
    }

    /// 组二：旅程——五阶段闭合 + 回退拒 + 跨引擎对端强制 + 阶段分解。
    pub fn selfcheck_journey(&mut self) -> Result<(), u32> {
        // 本引擎任务：CPU 提交→完成。
        self.journeys.enroll(Journey {
            task: 2,
            ts: [0, 100, 300, 300, 800],
            handoff_to: None,
            from_engine: 0,
            to_engine: 0,
        })?;
        // 跨引擎任务：GPU 提交→执行→交接给 NPU→完成。
        self.journeys.enroll(Journey {
            task: 1,
            ts: [0, 50, 400, 800, 4200],
            handoff_to: Some(3),
            from_engine: 1,
            to_engine: 2,
        })?;
        // 回退反例（Execute < Queue）。
        if self
            .journeys
            .enroll(Journey { task: 9, ts: [0, 500, 400, 600, 700], handoff_to: None, from_engine: 0, to_engine: 0 })
            .is_ok()
        {
            return Err(XC_JOURNEY_BROKEN);
        }
        // 跨引擎无对端反例。
        if self
            .journeys
            .enroll(Journey { task: 9, ts: [0, 100, 200, 300, 400], handoff_to: None, from_engine: 1, to_engine: 2 })
            .is_ok()
        {
            return Err(XC_JOURNEY_BROKEN);
        }
        // 阶段分解对拍：task1 = 队50/执行350/交接400/完成3400。
        let bd = self.journeys.stage_breakdown(1)?;
        if bd != [50, 350, 400, 3400] {
            return Err(XC_JOURNEY_BROKEN);
        }
        // 跨引擎计数=1。
        if self.journeys.cross_engine_count() != 1 {
            return Err(XC_JOURNEY_BROKEN);
        }
        // 总耗时对拍。
        if self.journeys.span_us(1)? != 4200 {
            return Err(XC_JOURNEY_BROKEN);
        }
        Ok(())
    }

    /// 组三：交互——缩放越界拒 + 合法缩放 + 过滤 + 选择命中/落空。
    pub fn selfcheck_interact(&mut self) -> Result<(), u32> {
        self.seed_sample()?;
        let mut vp = Viewport { w0: 0, width_us: 12_500, engine_mask: 0 };
        // 缩放越界反例（<10%）。
        if vp.zoom(500).is_ok() {
            return Err(XC_INTERACT_OOR);
        }
        // 合法缩放：放大两倍。
        vp.zoom(20_000)?;
        if vp.width_us != 25_000 {
            return Err(XC_INTERACT_OOR);
        }
        // 过滤越界反例。
        if vp.set_engine_mask(0x1F).is_ok() {
            return Err(XC_INTERACT_OOR);
        }
        // 过滤 GPU：mask=0b0010。
        vp.set_engine_mask(0b0010)?;
        vp.width_us = 12_500;
        // 选择 task1（GPU 渲染条）命中。
        match self.gantt.bar(0) {
            Some(b) => {
                if vp.select(b)? != 1 {
                    return Err(XC_INTERACT_OOR);
                }
            }
            None => return Err(XC_INTERACT_OOR),
        }
        // 选择 task2（CPU 条）被过滤落空。
        match self.gantt.bar(1) {
            Some(b) => {
                if vp.select(b).is_ok() {
                    return Err(XC_INTERACT_OOR);
                }
            }
            None => return Err(XC_INTERACT_OOR),
        }
        Ok(())
    }

    /// 组四：跨卷衔接——锚校验 + 偏移平移 + 越限拒。
    pub fn selfcheck_xvol(&mut self) -> Result<(), u32> {
        let bridge = XVolBridge::build();
        bridge.verified()?;
        // 同源无平移：恒等。
        if bridge.to_ve_time(1234) != 1234 {
            return Err(XC_XVOL_UNANCHORED);
        }
        // 带平移：锚定后平移语义成立。
        let mut b2 = XVolBridge::build();
        b2.ve_offset_us = 500;
        if b2.to_ve_time(1000) != 1500 {
            return Err(XC_XVOL_UNANCHORED);
        }
        // 越限反例：偏移 >1s 拒。
        b2.ve_offset_us = 1_000_001;
        if b2.verified().is_ok() {
            return Err(XC_XVOL_UNANCHORED);
        }
        // 断锚反例。
        let mut b3 = XVolBridge::build();
        b3.anchored = false;
        if b3.verified().is_ok() {
            return Err(XC_XVOL_UNANCHORED);
        }
        Ok(())
    }

    /// 全量自检入口（四组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        Self::segment_guard()?;
        self.selfcheck_product()?;
        self.selfcheck_journey()?;
        self.selfcheck_interact()?;
        self.selfcheck_xvol()?;
        Ok(())
    }

    /// 段对账：五码全落 0x8700..0x8800 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_GANTT_MALFORMED,
            XC_JOURNEY_BROKEN,
            XC_INTERACT_OOR,
            XC_XVOL_UNANCHORED,
            XC_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x8700 || codes[i] >= 0x8800 {
                return Err(XC_GANTT_MALFORMED);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_GANTT_MALFORMED);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
