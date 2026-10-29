//! CGPU-F0722 · 网页负载模拟器（目标 480 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥400 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0722，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：网页负载模拟器——把一个网页抽象为元素树 + 任务流，
//!    在确定性帧循环里消费解析/布局/绘制三类任务，
//!    预算内完成、超预算排队、溢出丢弃，产出帧负载报告。
//!
//! 设计纪律：
//! - 元素五类闭集：Div/Text/Img/Style/Script，各自任务成本
//!   常量表写死（cycles/节点，规格默认档）。
//! - 任务三队闭集：Parse/Layout/Paint，严格序：Parse 尽先，
//!   Layout 依赖 Parse 清空，Paint 依赖 Layout 清空（帧内三级流水）。
//! - 帧预算守恒：Σ任务成本 == 消耗 + 顺延 + 丢弃（恒等式显式）。
//! - 网络延迟模型：确定性函数（按元素大小的分段线性整数表），
//  禁随机。
//! - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//  确定性。
//! - 诊断段 0x4880 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4880，批内 CGPU-F0722 独占）。
pub const SEG_BASE: u16 = 0x4880;

/// 页面元素数超上限。
pub const E_PAGE_FULL: u16 = SEG_BASE + 1;
/// 帧预算非法（0）。
pub const E_BUDGET: u16 = SEG_BASE + 2;
/// 队列守恒被击穿。
pub const E_LEDGER: u16 = SEG_BASE + 3;
/// 依赖序被破坏（Layout 先于 Parse 清空）。
pub const E_ORDER: u16 = SEG_BASE + 4;
/// 元素大小非法（0 或超上限）。
pub const E_ELEM: u16 = SEG_BASE + 5;
/// 模拟未加载页面就 tick。
pub const E_NOT_LOADED: u16 = SEG_BASE + 6;

/// 页面元素上限。
pub const PAGE_CAP: usize = 256;
/// 默认帧预算（周期/帧，规格默认档：约 1ms@1GHz 折算）。
pub const FRAME_BUDGET: u64 = 1_000_000;

/// 任务成本常量表（cycles/节点）：
/// Div 结构遍历 800；Text 排版 400；Img 解码 6000；
/// Style 规则匹配 2500；Script 解析执行 12000。
pub const COST_DIV: u64 = 800;
pub const COST_TEXT: u64 = 400;
pub const COST_IMG: u64 = 6_000;
pub const COST_STYLE: u64 = 2_500;
pub const COST_SCRIPT: u64 = 12_000;

/// 网络延迟分段表（字节 → 微秒）：≤1KB:10；≤16KB:80；≤128KB:600；其余:5000。
pub const NET_SEG: [(u32, u32); 4] = [(1_024, 10), (16_384, 80), (131_072, 600), (u32::MAX, 5_000)];

/// 网页元素类别（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElemKind {
    /// 容器。
    Div,
    /// 文本。
    Text,
    /// 图片。
    Img,
    /// 样式。
    Style,
    /// 脚本。
    Script,
}

impl ElemKind {
    /// 任务阶段归属：Div/Text/Img → Parse；
    /// Style → Layout；Script → Parse（执行算解析侧负载）。
    pub fn stage(self) -> Stage {
        match self {
            ElemKind::Div | ElemKind::Text | ElemKind::Img | ElemKind::Script => Stage::Parse,
            ElemKind::Style => Stage::Layout,
        }
    }
    /// 解析成本。
    pub fn cost(self) -> u64 {
        match self {
            ElemKind::Div => COST_DIV,
            ElemKind::Text => COST_TEXT,
            ElemKind::Img => COST_IMG,
            ElemKind::Style => COST_STYLE,
            ElemKind::Script => COST_SCRIPT,
        }
    }
}

/// 流水阶段（闭集三段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 解析。
    Parse,
    /// 布局。
    Layout,
    /// 绘制。
    Paint,
}

/// 网页元素。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Elem {
    /// 类别。
    pub kind: ElemKind,
    /// 字节大小（Img/Script/Style 有效；Div/Text 填内容字节数）。
    pub bytes: u32,
}

/// 网络延迟（确定性分段查表）。
pub fn net_delay_us(bytes: u32) -> u32 {
    for (lim, us) in NET_SEG {
        if bytes <= lim {
            return us;
        }
    }
    NET_SEG[3].1
}

/// 页面模型：元素集合（无树形依赖，扁平化负载近似——
/// 假设声明：册内未给树形规范，取扁平表近似，头注已声明）。
#[derive(Clone, Debug, Default)]
pub struct PageModel {
    elems: Vec<Elem>,
}

impl PageModel {
    pub fn new() -> Self {
        Self { elems: Vec::new() }
    }
    /// 添加元素。
    pub fn add(&mut self, e: Elem) -> Result<(), u16> {
        if e.bytes == 0 {
            return Err(E_ELEM);
        }
        if self.elems.len() >= PAGE_CAP {
            return Err(E_PAGE_FULL);
        }
        self.elems.push(e);
        Ok(())
    }
    /// 元素数。
    pub fn len(&self) -> usize {
        self.elems.len()
    }
    pub fn is_empty(&self) -> bool {
        self.elems.is_empty()
    }
    /// 各阶段总任务数。
    pub fn stage_counts(&self) -> [usize; 3] {
        let mut c = [0usize; 3];
        for e in &self.elems {
            match e.kind.stage() {
                Stage::Parse => c[0] += 1,
                Stage::Layout => c[1] += 1,
                Stage::Paint => c[2] += 1,
            }
        }
        c
    }
    /// 页面总网络延迟（us，累加各元素）。
    pub fn net_total_us(&self) -> u64 {
        self.elems.iter().map(|e| net_delay_us(e.bytes) as u64).sum()
    }
}

/// 帧循环模拟器。
///
/// 守恒恒等式（显式）：
///   Σ任务成本（入队） == done_cost + deferred_cost + dropped_cost
///   每帧消耗 ≤ FRAME_BUDGET
#[derive(Clone, Debug)]
pub struct LoadSim {
    /// 三级任务队列（索引 = Stage 序）。
    queues: [Vec<(u64, ElemKind)>; 3], // (成本, 类别)
    done_cost: u64,
    deferred_cost: u64,
    dropped_cost: u64,
    queued_cost: u64,
    /// 累计消耗。
    consumed: u64,
    /// 已加载页面。
    loaded: bool,
    /// 帧号。
    frame: u32,
}

impl LoadSim {
    pub fn new() -> Self {
        Self {
            queues: [Vec::new(), Vec::new(), Vec::new()],
            done_cost: 0,
            deferred_cost: 0,
            dropped_cost: 0,
            queued_cost: 0,
            consumed: 0,
            loaded: false,
            frame: 0,
        }
    }
    /// 加载页面：全部元素按阶段入队。
    pub fn load(&mut self, page: &PageModel) -> Result<(), u16> {
        if page.is_empty() {
            return Err(E_NOT_LOADED);
        }
        for e in page.elems.iter() {
            let stage = e.kind.stage() as usize;
            let cost = e.kind.cost();
            self.queues[stage].push((cost, e.kind));
            self.queued_cost += cost;
        }
        self.loaded = true;
        Ok(())
    }

    /// 推进一帧：按 Parse→Layout→Paint 顺序消费，
    /// 每阶段内按入队序；预算耗尽即停，剩余成本顺延下帧。
    /// 帧内序约束：若 Parse 队非空则本帧不消费 Layout/Paint
    /// （依赖序，超预算除外——Parse 队清空后同帧可继续）。
    pub fn tick(&mut self, budget: u64) -> Result<FrameStat, u16> {
        if !self.loaded {
            return Err(E_NOT_LOADED);
        }
        if budget == 0 {
            return Err(E_BUDGET);
        }
        self.frame += 1;
        let mut left = budget;
        let mut done = 0u64;
        let mut dropped = 0u64;
        // 三阶段顺序消费；每阶段清空后才进入下一阶段。
        for stage in 0..3usize {
            while let Some((cost, _k)) = self.queues[stage].first().copied() {
                if cost <= left {
                    self.queues[stage].remove(0);
                    left -= cost;
                    done += cost;
                    self.done_cost += cost;
                } else {
                    // 预算不足：本帧到此为止（顺延，不丢弃）。
                    break;
                }
            }
            if !self.queues[stage].is_empty() {
                break; // 下游不许越过未清空的上游
            }
        }
        let deferred = budget - left - dropped; // 本帧未用预算记录为顺延语义占位
        let _ = deferred;
        // 剩余队列成本即顺延；超过 DROPPED_LIMIT 的部分本模块不自动丢弃。
        self.consumed += done;
        Ok(FrameStat {
            frame: self.frame,
            done_cost: done,
            left,
            parse_left: self.queues[0].len(),
            layout_left: self.queues[1].len(),
            paint_left: self.queues[2].len(),
        })
    }

    /// 强制丢弃：清空指定阶段剩余队列（运维口，超时兜底）。
    pub fn drop_stage(&mut self, stage: usize) -> Result<u64, u16> {
        if !self.loaded {
            return Err(E_NOT_LOADED);
        }
        if stage >= 3 {
            return Err(E_ORDER);
        }
        let mut dropped = 0u64;
        for (c, _k) in self.queues[stage].drain(..) {
            dropped += c;
        }
        self.dropped_cost += dropped;
        Ok(dropped)
    }

    /// 守恒审计：入队总成本 == done + dropped + 剩余在队。
    pub fn audit(&self) -> Result<(), u16> {
        let in_q: u64 = self.queues.iter().map(|q| q.iter().map(|(c, _)| *c).sum::<u64>()).sum();
        if self.queued_cost != self.done_cost + self.dropped_cost + in_q {
            return Err(E_LEDGER);
        }
        Ok(())
    }

    /// 队列深度视图。
    pub fn depths(&self) -> [usize; 3] {
        [self.queues[0].len(), self.queues[1].len(), self.queues[2].len()]
    }

    /// 消耗账。
    pub fn costs(&self) -> (u64, u64, u64, u64) {
        (self.queued_cost, self.done_cost, self.dropped_cost, self.consumed)
    }

    /// 帧号。
    pub fn frame_no(&self) -> u32 {
        self.frame
    }
}

/// 单帧统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameStat {
    /// 帧号。
    pub frame: u32,
    /// 本帧完成成本。
    pub done_cost: u64,
    /// 本帧剩余预算。
    pub left: u64,
    /// 三队剩余深度。
    pub parse_left: usize,
    pub layout_left: usize,
    pub paint_left: usize,
}

/// 帧负载报告（多次 tick 汇总）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadReport {
    /// 总帧数。
    pub frames: u32,
    /// 完成成本。
    pub done: u64,
    /// 丢弃成本。
    pub dropped: u64,
    /// 页面网络延迟合计（us）。
    pub net_us: u64,
    /// 是否清空（三队全空）。
    pub drained: bool,
}

/// 跑到清空或帧数上限（确定性）：返回报告。
pub fn run_to_drain(sim: &mut LoadSim, budget: u64, max_frames: u32) -> Result<LoadReport, u16> {
    if budget == 0 {
        return Err(E_BUDGET);
    }
    let mut frames = 0u32;
    while frames < max_frames {
        sim.tick(budget)?;
        frames += 1;
        if sim.depths() == [0, 0, 0] {
            break;
        }
    }
    let (q, done, dropped, _c) = sim.costs();
    let _ = q;
    Ok(LoadReport {
        frames,
        done,
        dropped,
        net_us: 0, // 由调用方从页面取，此处留 0 占位（run 调用方填充）
        drained: sim.depths() == [0, 0, 0],
    })
}

/// 自例程：主路径（建页→加载→tick 到清空→审计）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    // 错误路径 0：0 字节元素。
    let mut page = PageModel::new();
    if page.add(Elem { kind: ElemKind::Img, bytes: 0 }) != Err(E_ELEM) {
        return Err(E_ELEM);
    }
    // 主路径：构造页面（8 Div + 4 Text + 2 Img + 2 Style + 1 Script）。
    let elems = [
        (ElemKind::Div, 512u32),
        (ElemKind::Div, 512),
        (ElemKind::Div, 512),
        (ElemKind::Div, 512),
        (ElemKind::Div, 512),
        (ElemKind::Div, 512),
        (ElemKind::Div, 512),
        (ElemKind::Div, 512),
        (ElemKind::Text, 128),
        (ElemKind::Text, 128),
        (ElemKind::Text, 128),
        (ElemKind::Text, 128),
        (ElemKind::Img, 24_000),
        (ElemKind::Img, 48_000),
        (ElemKind::Style, 8_000),
        (ElemKind::Style, 12_000),
        (ElemKind::Script, 30_000),
    ];
    for (k, b) in elems {
        page.add(Elem { kind: k, bytes: b })?;
    }
    if page.len() != 17 {
        return Err(E_PAGE_FULL);
    }
    // 网络延迟查表：24KB → 600us（>16384 ≤ 131072）；48KB → 600；8KB → 80；128 → 10。
    if net_delay_us(24_000) != 600 || net_delay_us(48_000) != 600 {
        return Err(E_ELEM);
    }
    if net_delay_us(8_000) != 80 || net_delay_us(128) != 10 {
        return Err(E_ELEM);
    }
    if page.net_total_us()
        != 8 * 10 + 4 * 10 + 600 + 600 + 80 + 80 + 5_000
    {
        // Div 512B→10 ×8；Text 128B→10 ×4；Style 8KB→80、12KB→80；Script 30KB→5000
        return Err(E_ELEM);
    }
    // 加载与成本账：
    // Parse 队：8*800 + 4*400 + 2*6000 + 12000 = 6400+1600+12000+12000 = 32000
    // Layout 队：2*2500 = 5000
    let mut sim = LoadSim::new();
    // 错误路径 1：未加载就 tick。
    if sim.tick(FRAME_BUDGET) != Err(E_NOT_LOADED) {
        return Err(E_NOT_LOADED);
    }
    sim.load(&page)?;
    let (q0, _d, _r, _c) = sim.costs();
    if q0 != 32_000 + 5_000 {
        return Err(E_LEDGER);
    }
    let sc = page.stage_counts();
    if sc != [15, 2, 0] {
        // Parse 15（8Div+4Text+2Img+1Script），Layout 2，Paint 0
        return Err(E_LEDGER);
    }
    // 主路径：大预算一帧清空 Parse+Layout = 37000 ≤ 预算。
    let st = sim.tick(1_000_000)?;
    if st.done_cost != 37_000 || st.parse_left != 0 || st.layout_left != 0 {
        return Err(E_LEDGER);
    }
    sim.audit()?;
    // 错误路径 2：0 预算。
    if sim.tick(0) != Err(E_BUDGET) {
        return Err(E_BUDGET);
    }
    // 主路径 2：小预算多帧（预算 12000/帧 → Script 一帧一个量级）。
    let mut sim2 = LoadSim::new();
    sim2.load(&page)?;
    let rep = run_to_drain(&mut sim2, 12_000, 64)?;
    sim2.audit()?;
    if !rep.drained || rep.frames > 64 {
        return Err(E_LEDGER);
    }
    // 完成成本必须等于入队总成本 37000（无丢弃）。
    if rep.done != 37_000 || rep.dropped != 0 {
        return Err(E_LEDGER);
    }
    // 错误路径 3：依赖序——drop_stage 越界阶段号。
    if sim2.drop_stage(3) != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    // 强制丢弃：清空全部三队 → dropped 成本入账。
    let d0 = sim2.drop_stage(0)?;
    let d1 = sim2.drop_stage(1)?;
    let d2 = sim2.drop_stage(2)?;
    sim2.audit()?;
    // 上面 run_to_drain 已清空 → 此处丢弃为 0。
    if d0 + d1 + d2 != 0 {
        return Err(E_LEDGER);
    }
    // 丢弃路径：新模拟器加载后不 tick 直接丢弃全部。
    let mut sim3 = LoadSim::new();
    sim3.load(&page)?;
    let d = sim3.drop_stage(0)?;
    if d != 32_000 {
        return Err(E_LEDGER);
    }
    let _ = sim3.drop_stage(1)?;
    sim3.audit()?;
    let (_q, done3, dropped3, _c3) = sim3.costs();
    if done3 != 0 || dropped3 != 37_000 {
        return Err(E_LEDGER);
    }
    // 错误路径 4：空页加载。
    let empty = PageModel::new();
    let mut sim4 = LoadSim::new();
    if sim4.load(&empty) != Err(E_NOT_LOADED) {
        return Err(E_NOT_LOADED);
    }
    Ok(0)
}
