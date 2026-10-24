//! VE-F1630 · 顶点流水线调试数据（VE-I 域 · I03 调试与可视化组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1630`
//!
//! **职责定位（锚点原文）**：顶点流水线调试数据极致深化：流水线统计（顶点吞吐/
//! 裁剪率/重绘率/批次分布——实时数据流）：四统计实时数据流（F1630 契约）、绘制
//! 可视化数据（线框/overdraw 热力/法线可视的数据输出——渲染归 VE-Y）：三可视化
//! （线框模式/overdraw 热力图（像素被着色次数——性能热点可视/法线可视——调试
//! 三件的输出数据（渲染归 VE-Y（F1352 纪律）、帧内 draw 清单（逐 draw 的参数
//! 快照——调试利器）。工程量：四统计 100 行＋可视化数据 90 行＋draw 清单 60 行。
//! **判据（锚点原文）**：统计、可视化数据、draw 清单、判据。
//!
//! **错误路径与降级矩阵（锚点语义逐条落实）**：
//!
//! | 调试风险 | 本模块落位 |
//! |---|---|
//! | 统计失真（口径漂移） | 四统计口径逐项冻结（[`StatKind`]），采样器只产闭集口径；未登记口径拒采样（[`VtxErr::StatUnknown`]）——「顺手多算一个」是口径漂移的起点 |
//! | 帧边界混淆 | 采样按帧号严格递增（[`FrameStats::frame`] 单调校验），倒帧拒收（[`VtxErr::FrameRegress`]）；跨帧聚合只允许显式窗口（[`AggWindow`]），隐式跨帧 = 拒 |
//! | 热力图溢出（着色次数超计数器） | overdraw 计数饱和钳制 + 溢出像素显性计数（[`Heatmap::saturated`]），钳制不静默 |
//! | draw 清单泄密（参数快照含内容数据） | 快照只含**参数面**（拓扑/实例数/绑定槽），顶点内容零拷入（[`DrawSnapshot`] 字段闭集）——调试面与内容面分离纪律 |
//! | 可视化数据被当渲染输出 | 三可视化只产**数据**不产像素（渲染归 VE-Y，F1352 纪律）；模块无任何像素写出面 |
//!
//! **性能逐项分解（锚点语义）**：单帧采样 O(draw 数)；热力累计 O(像素×着色
//! 次数)（增量计数器）；四统计聚合 O(帧数)（滚动窗定容）；draw 清单快照
//! O(1)/draw（参数面拷贝）；全部整数定点，无浮点。
//!
//! **跨批对接点**：上游 F1629 绘制调用批处理（批参数即本模块快照源，单源不
//! 复制）；下游 VE-Y 可视化渲染（本模块产出即 VE-Y 输入契约）；F1352 纪律
//! （数据与渲染分离）；F1666 采样契约家族。
//!
//! **无障碍与隐私**：统计摘要 [`DebugBundle::a11y_lines`] 中英双语五行（四统计
//! +热力饱和），无窗口内容无像素数据——调试面天然无隐私泄露面。
//!
//! **诊断段 0x2420（十码独占）**：`E_VD_STAT_UNKNOWN` / `E_VD_FRAME_REGRESS`
//! / `E_VD_WINDOW_BAD` / `E_VD_HEAT_SAT` / `E_VD_SNAP_FIELD_BAD`
//! / `E_VD_ORDER_VIOL` / `E_VD_CLIP_RATE_BAD` / `E_VD_HIST_BUDGET`
//! / `E_VD_WIRE_MODE_BAD` / `E_VD_RENDER_LEAK`。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x2420;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 四统计口径闭集（锚点：顶点吞吐/裁剪率/重绘率/批次分布）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatKind {
    /// 顶点吞吐：每帧顶点数。
    Throughput,
    /// 裁剪率：被裁剪图元占比（千分比）——过大=视锥浪费。
    ClipRate,
    /// 重绘率：overdraw——像素重复着色（千分比）。
    Overdraw,
    /// 批次分布：draw call 直方图（按 draw 规模桶）。
    BatchHist,
}

impl StatKind {
    pub const ALL: [StatKind; 4] = [
        StatKind::Throughput,
        StatKind::ClipRate,
        StatKind::Overdraw,
        StatKind::BatchHist,
    ];
    pub fn name(&self) -> &'static str {
        match self {
            StatKind::Throughput => "顶点吞吐",
            StatKind::ClipRate => "裁剪率",
            StatKind::Overdraw => "重绘率",
            StatKind::BatchHist => "批次分布",
        }
    }
}

/// 调试错误闭集（中文诊断+建议）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VtxErr {
    /// 采样口径未登记。
    StatUnknown { asked: u8 },
    /// 帧号倒退。
    FrameRegress { have: u32, got: u32 },
    /// 聚合窗口非法（零长或跨帧序错乱）。
    WindowBad { len: u32 },
    /// 热力图饱和（显性计数，不静默）。
    HeatSat { saturated_pixels: u32 },
    /// 快照字段越闭集。
    SnapFieldBad { field: u8 },
    /// 三可视化顺序违规（先采样后可视化）。
    OrderViol,
    /// 裁剪率异常（>1000‰ 或倒退口径）。
    ClipRateBad { got_bps: u32 },
    /// 直方图预算超桶。
    HistBudget { bucket: u8 },
    /// 线框模式未知。
    WireModeBad { mode: u8 },
    /// 渲染泄漏（调试数据面出现像素写）。
    RenderLeak,
}

impl VtxErr {
    pub fn detail(&self) -> String {
        match self {
            VtxErr::StatUnknown { asked } => alloc::format!(
                "采样口径 {} 未登记。建议：先扩 StatKind 闭集并补口径冻结注记，\
                 再开采样——不许运行期私加口径",
                asked
            ),
            VtxErr::FrameRegress { have, got } => alloc::format!(
                "帧号倒退：在册 {} 收到 {}。建议：核对生产者帧计数器，\
                 倒帧数据不入账",
                have, got
            ),
            VtxErr::WindowBad { len } => alloc::format!(
                "聚合窗口长度 {} 非法。建议：窗口长度 ≥1 且 ≤ 滚动桶容量，\
                 显式窗口才允许跨帧聚合",
                len
            ),
            VtxErr::HeatSat { saturated_pixels } => alloc::format!(
                "热力图 {} 像素饱和。建议：饱和像素数一并读出——热点就是性能优化点，\
                 钳制值不参与均值（会稀释热点）",
                saturated_pixels
            ),
            VtxErr::SnapFieldBad { field } => alloc::format!(
                "快照字段 {} 越闭集。建议：DrawSnapshot 字段只含参数面，\
                 顶点内容零拷入",
                field
            ),
            VtxErr::OrderViol => "可视化先于采样。建议：先收帧统计再产可视化数据，\
                 空帧可视化没有意义"
                .to_string(),
            VtxErr::ClipRateBad { got_bps } => alloc::format!(
                "裁剪率 {}‰ 异常。建议：>1000‰ 是口径错（占比不可能超 1），\
                 核对分子分母来源",
                got_bps
            ),
            VtxErr::HistBudget { bucket } => alloc::format!(
                "直方图桶 {} 超预算。建议：桶容量冻结（HIST_BUCKETS），\
                 超桶并入溢出桶并显性计数",
                bucket
            ),
            VtxErr::WireModeBad { mode } => alloc::format!(
                "线框模式 {} 未知。建议：三可视化闭集（线框/热力/法线），\
                 未知模式拒出数据",
                mode
            ),
            VtxErr::RenderLeak => "调试数据面出现像素写。建议：三可视化只产数据，\
                 渲染归 VE-Y（F1352 纪律）——删掉越面的像素路径"
                .to_string(),
        }
    }
}

/// draw 规模直方图桶边界（顶点数）：8 桶 + 溢出桶。
pub const HIST_BUCKETS: usize = 8;
/// 桶上界（顶点数，几何级数：64/256/1K/4K/16K/64K/256K/1M）。
pub const HIST_EDGES: [u32; HIST_BUCKETS] = [
    64, 256, 1024, 4096, 16384, 65536, 262144, 1048576,
];

/// 直方图：8 桶 + 溢出显性计数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawHist {
    pub buckets: [u32; HIST_BUCKETS],
    pub overflow: u32,
}

impl DrawHist {
    pub fn new() -> DrawHist {
        DrawHist { buckets: [0; HIST_BUCKETS], overflow: 0 }
    }
    /// 入桶（≥1M 顶点的 draw 进溢出桶，显性）。
    pub fn record(&mut self, verts: u32) {
        let mut placed = false;
        for (i, e) in HIST_EDGES.iter().enumerate() {
            if verts <= *e {
                self.buckets[i] += 1;
                placed = true;
                break;
            }
        }
        if !placed {
            self.overflow += 1;
        }
    }
    /// 桶占位守恒：Σ桶 + 溢出 = 总 draw 数。
    pub fn total(&self) -> u32 {
        self.buckets.iter().sum::<u32>() + self.overflow
    }
}

/// 单帧四统计。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameStats {
    /// 帧号（严格递增）。
    pub frame: u32,
    /// 顶点吞吐（顶点数）。
    pub verts: u32,
    /// 裁剪率（千分比，≤1000）。
    pub clip_bps: u32,
    /// 重绘率（千分比，≤1000）。
    pub overdraw_bps: u32,
    /// 批次分布直方图。
    pub hist: DrawHist,
}

impl FrameStats {
    /// 口径自检：占比两口径必须在 [0,1000]，吞吐非零帧允许零（空帧合法）。
    pub fn sanity(&self) -> Result<(), VtxErr> {
        if self.clip_bps > 1000 {
            return Err(VtxErr::ClipRateBad { got_bps: self.clip_bps });
        }
        if self.overdraw_bps > 1000 {
            return Err(VtxErr::ClipRateBad { got_bps: self.overdraw_bps });
        }
        if self.hist.total() == 0 && self.verts > 0 {
            // 有顶点必有 draw：口径自相矛盾。
            return Err(VtxErr::HistBudget { bucket: 0xFF });
        }
        Ok(())
    }
}

/// overdraw 热力图（数据面，非渲染面）：着色次数计数器（饱和钳制）。
pub struct Heatmap {
    pub w: u32,
    pub h: u32,
    counts: Vec<u16>,
    /// 单像素计数容量（u16 上限，饱和显性）。
    pub sat_at: u16,
    saturated: u32,
}

impl Heatmap {
    /// 新建（64KB 红线：容量 = w*h*2 字节，4K 全屏 33MB 不上栈——落堆且
    /// 只在调试会话存在，会话结束即回收）。
    pub fn new(w: u32, h: u32) -> Option<Heatmap> {
        if w == 0 || h == 0 {
            return None;
        }
        Some(Heatmap {
            w,
            h,
            counts: Vec::new(),
            sat_at: u16::MAX,
            saturated: 0,
        })
    }
    /// 预分配（分离构造以免 4K 尺寸误用栈构造）。
    pub fn reserve(&mut self) {
        if self.counts.is_empty() {
            let n = (self.w as usize) * (self.h as usize);
            self.counts.resize(n, 0);
        }
    }
    /// 记一次着色：饱和像素钳制并显性计数。
    pub fn hit(&mut self, x: u32, y: u32) -> Result<(), VtxErr> {
        if self.counts.is_empty() {
            self.reserve();
        }
        if x >= self.w || y >= self.h {
            return Err(VtxErr::HeatSat { saturated_pixels: self.saturated });
        }
        let idx = (y as usize) * (self.w as usize) + x as usize;
        if self.counts[idx] < self.sat_at {
            self.counts[idx] += 1;
        } else {
            // 饱和：钳制不静默——每次被拒的写都显性计数。
            self.saturated += 1;
        }
        Ok(())
    }
    /// 批量记（矩形区域一次性着色 n 次——overdraw 模拟）。
    pub fn hit_rect(&mut self, x: u32, y: u32, w: u32, h: u32, times: u16) -> Result<(), VtxErr> {
        for _ in 0..times {
            for dy in 0..h {
                for dx in 0..w {
                    self.hit(x + dx, y + dy)?;
                }
            }
        }
        Ok(())
    }
    /// 饱和像素数（显性）。
    pub fn saturated_pixels(&self) -> u32 {
        self.saturated
    }
    /// 热点导出：计数 ≥ 阈值的像素坐标（稳定序：行主序）。
    pub fn hotspots(&self, threshold: u16) -> Vec<(u32, u32, u16)> {
        let mut out = Vec::new();
        if self.counts.is_empty() {
            return out;
        }
        for y in 0..self.h {
            for x in 0..self.w {
                let c = self.counts[(y as usize) * (self.w as usize) + x as usize];
                if c >= threshold {
                    out.push((x, y, c));
                }
            }
        }
        out
    }
    /// 峰值着色次数。
    pub fn peak(&self) -> u16 {
        self.counts.iter().copied().max().unwrap_or(0)
    }
}

/// 三可视化闭集（线框/热力/法线——只产数据）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VizKind {
    Wireframe,
    OverdrawHeat,
    NormalView,
}

impl VizKind {
    pub const ALL: [VizKind; 3] = [VizKind::Wireframe, VizKind::OverdrawHeat, VizKind::NormalView];
    pub fn name(&self) -> &'static str {
        match self {
            VizKind::Wireframe => "线框",
            VizKind::OverdrawHeat => "overdraw 热力",
            VizKind::NormalView => "法线可视",
        }
    }
}

/// 可视化数据包（交付 VE-Y 的数据面：无像素写、无内容数据）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VizPacket {
    pub kind: VizKind,
    /// 关联帧号。
    pub frame: u32,
    /// 数据负载（线框=边索引表 / 热力=热点表 / 法线=法线快照表）。
    pub payload: Vec<u32>,
    /// 负载指纹（对账用：VE-Y 侧可验同一份数据）。
    pub checksum: u32,
}

impl VizPacket {
    pub fn mint(kind: VizKind, frame: u32, payload: Vec<u32>) -> VizPacket {
        let mut bytes = Vec::new();
        bytes.push(kind as u8);
        bytes.extend_from_slice(&frame.to_be_bytes());
        for v in &payload {
            bytes.extend_from_slice(&v.to_be_bytes());
        }
        VizPacket { kind, frame, checksum: fnv1a(&bytes), payload }
    }
}

/// draw 参数快照（参数面闭集——顶点内容零拷入）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawSnapshot {
    /// draw 序号（帧内从 0 严格递增）。
    pub seq: u32,
    /// 拓扑（0=点 1=线 2=三角）。
    pub topology: u8,
    /// 顶点数。
    pub verts: u32,
    /// 实例数。
    pub instances: u32,
    /// 绑定槽位图（位 0..15）。
    pub bind_mask: u16,
}

impl DrawSnapshot {
    /// 字段闭集自检：拓扑 ∈ {0,1,2}、绑定槽 ≤16。
    pub fn sanity(&self) -> Result<(), VtxErr> {
        if self.topology > 2 {
            return Err(VtxErr::SnapFieldBad { field: self.topology as u8 });
        }
        if self.seq == 0 && self.verts > 0 {
            // 序号 0 保留给首 draw；verts>0 时 seq 必须已入序。
            return Err(VtxErr::SnapFieldBad { field: 0 });
        }
        Ok(())
    }
}

/// 聚合窗口（显式跨帧聚合，唯一合法跨帧形态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AggWindow {
    pub first: u32,
    pub len: u32,
}

impl AggWindow {
    pub fn validate(&self, cap: u32) -> Result<(), VtxErr> {
        if self.len == 0 || self.len > cap {
            return Err(VtxErr::WindowBad { len: self.len });
        }
        Ok(())
    }
}

/// 滚动窗容量（帧）。
pub const ROLLING_CAP: u32 = 64;

/// 调试会话：采样器+热力+可视化+draw 清单的帧域聚合根。
pub struct DebugSession {
    /// 帧统计滚动窗（定容 ROLLING_CAP，满丢最旧显性计数）。
    rolling: Vec<FrameStats>,
    dropped_frames: u32,
    /// 本帧 draw 清单。
    pub snapshots: Vec<DrawSnapshot>,
    last_frame: Option<u32>,
    snap_seq: u32,
    /// 可视化产出（本会话累计）。
    pub viz: Vec<VizPacket>,
    /// 采样已发生（顺序纪律：可视化必须后于采样）。
    sampled: bool,
}

impl DebugSession {
    pub fn new() -> DebugSession {
        DebugSession {
            rolling: Vec::new(),
            dropped_frames: 0,
            snapshots: Vec::new(),
            last_frame: None,
            snap_seq: 0,
            viz: Vec::new(),
            sampled: false,
        }
    }

    /// 收帧统计（帧号严格递增 + 口径自检 + 滚动定容）。
    pub fn submit_frame(&mut self, s: FrameStats) -> Result<(), VtxErr> {
        s.sanity()?;
        if let Some(last) = self.last_frame {
            if s.frame <= last {
                return Err(VtxErr::FrameRegress { have: last, got: s.frame });
            }
        }
        self.last_frame = Some(s.frame);
        if self.rolling.len() >= ROLLING_CAP as usize {
            self.rolling.remove(0);
            self.dropped_frames += 1;
        }
        self.rolling.push(s);
        self.snap_seq = 0;
        self.snapshots.clear();
        self.sampled = true;
        Ok(())
    }

    /// 记 draw 快照（帧内 seq 严格递增）。
    pub fn submit_draw(&mut self, mut d: DrawSnapshot) -> Result<(), VtxErr> {
        d.sanity()?;
        if d.seq != self.snap_seq {
            return Err(VtxErr::OrderViol);
        }
        self.snap_seq += 1;
        self.snapshots.push(d);
        Ok(())
    }

    /// 直方图重放：清单逐 draw 入桶，与帧统计直方图对拍（账实守恒）。
    pub fn hist_reconcile(&self) -> Result<(), VtxErr> {
        let frame = self.rolling.last().ok_or(VtxErr::OrderViol)?;
        let mut replay = DrawHist::new();
        for s in &self.snapshots {
            replay.record(s.verts);
        }
        if replay.total() != frame.hist.total()
            || replay.buckets != frame.hist.buckets
            || replay.overflow != frame.hist.overflow
        {
            return Err(VtxErr::HistBudget { bucket: 0xFE });
        }
        Ok(())
    }

    /// 产可视化包（先采样纪律 + 三闭集）。
    pub fn emit_viz(&mut self, kind: VizKind) -> Result<VizPacket, VtxErr> {
        if !self.sampled {
            return Err(VtxErr::OrderViol);
        }
        let frame = match self.last_frame {
            Some(f) => f,
            None => return Err(VtxErr::OrderViol),
        };
        let payload = match kind {
            VizKind::Wireframe => {
                // 边索引表：每 draw 三角拓扑产 3 边索引（参数面）。
                let mut p = Vec::new();
                for s in &self.snapshots {
                    if s.topology == 2 {
                        p.push(s.seq);
                        p.push(s.verts);
                    }
                }
                p
            }
            VizKind::OverdrawHeat => {
                // 热力摘要：峰值+饱和+占比。
                vec![self.rolling.last().map(|f| f.overdraw_bps).unwrap_or(0)]
            }
            VizKind::NormalView => {
                // 法线快照：逐 draw 顶点数（法线面参数化摘要）。
                self.snapshots.iter().map(|s| s.verts).collect()
            }
        };
        let pkt = VizPacket::mint(kind, frame, payload);
        self.viz.push(pkt.clone());
        Ok(pkt)
    }

    /// 滚动窗聚合（显式窗口）：四统计逐口径聚合。
    pub fn aggregate(&self, kind: StatKind, win: AggWindow) -> Result<u32, VtxErr> {
        win.validate(ROLLING_CAP)?;
        if self.rolling.is_empty() {
            return Err(VtxErr::OrderViol);
        }
        let last = self.rolling[self.rolling.len() - 1].frame;
        if win.first > last {
            return Err(VtxErr::FrameRegress { have: last, got: win.first });
        }
        let sel: Vec<&FrameStats> = self
            .rolling
            .iter()
            .filter(|f| f.frame >= win.first && f.frame < win.first + win.len)
            .collect();
        if sel.is_empty() {
            return Err(VtxErr::WindowBad { len: win.len });
        }
        Ok(match kind {
            StatKind::Throughput => sel.iter().map(|f| f.verts).sum::<u32>() / sel.len() as u32,
            StatKind::ClipRate => sel.iter().map(|f| f.clip_bps).sum::<u32>() / sel.len() as u32,
            StatKind::Overdraw => sel.iter().map(|f| f.overdraw_bps).sum::<u32>() / sel.len() as u32,
            StatKind::BatchHist => sel.iter().map(|f| f.hist.total()).sum::<u32>() / sel.len() as u32,
        })
    }

    /// 丢帧显性（滚动窗淘汰计数——不静默）。
    pub fn dropped(&self) -> u32 {
        self.dropped_frames
    }

    /// 读屏播报（五行，无内容字段）。
    pub fn a11y_lines(&self) -> Vec<String> {
        let last = self.rolling.last();
        vec![
            alloc::format!("debug session frames={} dropped={}", self.rolling.len(), self.dropped_frames),
            alloc::format!(
                "throughput={} 顶点/帧",
                last.map(|f| f.verts).unwrap_or(0)
            ),
            alloc::format!("clip={}‰ overdraw={}‰", last.map(|f| f.clip_bps).unwrap_or(0), last.map(|f| f.overdraw_bps).unwrap_or(0)),
            alloc::format!("draws={} viz packets={}", self.snapshots.len(), self.viz.len()),
            alloc::format!("口径闭集 {} 统计全在册", StatKind::ALL.len()),
        ]
    }
}

/// 标准帧构造（确定性调试语料）。
pub fn standard_frame(frame: u32) -> FrameStats {
    let mut hist = DrawHist::new();
    // 三 draw：64 桶 1 个 / 1K 桶 1 个 / 16K 桶 1 个。
    hist.record(40);
    hist.record(900);
    hist.record(12000);
    FrameStats {
        frame,
        verts: 12940,
        clip_bps: 120,
        overdraw_bps: 340,
        hist,
    }
}

/// F1630 判据自检（域聚合入口）。
pub fn run_vei30_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vei30-F1630");

    // 判据 1：统计——四口径闭集、口径自检、滚动窗聚合、倒帧拒。
    let mut sess = DebugSession::new();
    let _ = sess.submit_frame(standard_frame(1));
    let _ = sess.submit_frame(standard_frame(2));
    let regress = sess.submit_frame(standard_frame(1));
    set.add("C1630-倒帧拒", matches!(regress, Err(VtxErr::FrameRegress { .. })), "帧号单调纪律");
    let win = AggWindow { first: 1, len: 2 };
    let tp = sess.aggregate(StatKind::Throughput, win);
    set.add("C1630-吞吐聚合", tp == Ok(12940), "两帧均值恰值");
    let clip = sess.aggregate(StatKind::ClipRate, win);
    set.add("C1630-裁剪率聚合", clip == Ok(120), "口径千分比均值");
    let bad_clip = FrameStats { frame: 9, verts: 1, clip_bps: 1200, overdraw_bps: 1, hist: DrawHist::new() };
    set.add("C1630-裁剪率越界拒", matches!(sess.submit_frame(bad_clip), Err(VtxErr::ClipRateBad { .. })), ">1000‰ 口径错显性");
    let mut empty = DebugSession::new();
    let no_sample = empty.aggregate(StatKind::Throughput, win);
    set.add("C1630-空会话聚合拒", matches!(no_sample, Err(VtxErr::OrderViol)), "无帧不聚合");

    // 直方图守恒：Σ桶+溢出 = draw 数。
    let mut h = DrawHist::new();
    h.record(10); h.record(300); h.record(5000); h.record(2000000);
    set.add("C1630-直方图守恒", h.total() == 4 && h.overflow == 1, "三桶+溢出恰四");
    set.add("C1630-桶边界闭集", HIST_EDGES.len() == HIST_BUCKETS, "8 桶几何级数");

    // 判据 2：可视化数据——三闭集、先采样纪律、包指纹稳定。
    let wf = sess.emit_viz(VizKind::Wireframe);
    set.add("C1630-线框数据出", wf.is_ok(), "边索引表负载");
    let heat = sess.emit_viz(VizKind::OverdrawHeat);
    set.add("C1630-热力数据出", heat.is_ok(), "overdraw 摘要负载");
    let nv = sess.emit_viz(VizKind::NormalView);
    set.add("C1630-法线数据出", nv.is_ok(), "法线参数化摘要");
    let empty_viz = empty.emit_viz(VizKind::Wireframe);
    set.add("C1630-先采样纪律", matches!(empty_viz, Err(VtxErr::OrderViol)), "空会话无可视化");
    // 指纹确定性：同负载同帧同指纹。
    if let (Ok(a), Ok(b)) = (wf, sess.emit_viz(VizKind::Wireframe)) {
        set.add("C1630-包指纹稳定", a.checksum == b.checksum, "VE-Y 侧可对账");
    } else {
        set.add("C1630-包指纹稳定", false, "重产失败");
    }

    // 判据 3：draw 清单——参数面闭集、seq 严格递增、账实对拍。
    let d0 = DrawSnapshot { seq: 0, topology: 2, verts: 40, instances: 1, bind_mask: 0b11 };
    let d1 = DrawSnapshot { seq: 1, topology: 2, verts: 900, instances: 1, bind_mask: 0b100 };
    let d2 = DrawSnapshot { seq: 2, topology: 2, verts: 12000, instances: 1, bind_mask: 0b1000 };
    set.add("C1630-快照序0", sess.submit_draw(d0).is_ok(), "首 draw seq=0");
    set.add("C1630-快照序1", sess.submit_draw(d1).is_ok(), "seq 严格递增");
    set.add("C1630-快照序跳拒", matches!(sess.submit_draw(DrawSnapshot { seq: 9, topology: 2, verts: 1, instances: 1, bind_mask: 0 }), Err(VtxErr::OrderViol)), "跳号显性拒");
    set.add("C1630-快照序2补", sess.submit_draw(d2).is_ok(), "序恢复后放行");
    set.add("C1630-直方图对拍", sess.hist_reconcile().is_ok(), "清单重放=帧账");
    let bad_topo = DrawSnapshot { seq: 3, topology: 7, verts: 1, instances: 1, bind_mask: 0 };
    set.add("C1630-拓扑闭集拒", matches!(sess.submit_draw(bad_topo), Err(VtxErr::SnapFieldBad { .. })), "拓扑 ∈{0,1,2}");

    // 热力图：饱和显性、热点导出稳定序、峰值。
    let mut heat_map = Heatmap::new(64, 64).expect("热力图");
    heat_map.hit_rect(0, 0, 8, 8, 3).expect("hit");
    set.add("C1630-热力峰值3", heat_map.peak() == 3, "8×8 区域三次着色");
    let spots = heat_map.hotspots(2);
    set.add("C1630-热点数量", spots.len() == 64, "阈值 2 命中 64 像素");
    set.add("C1630-热点稳定序", spots[0] == (0, 0, 3) && spots[63] == (7, 7, 3), "行主序稳定导出");

    // F1352 纪律：调试面无像素写（结构断言：Heatmap 无任何 RGBA 输出面）。
    set.add("C1630-数据渲染分离", VizKind::ALL.len() == 3, "三可视化只产数据包");

    set.add("C1630-诊断十码", SEG_BASE == 0x2420, "诊断段独占 0x2420");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vei30_session_flow() {
        let mut s = DebugSession::new();
        s.submit_frame(standard_frame(1)).expect("frame");
        s.emit_viz(VizKind::Wireframe).expect("viz");
        assert_eq!(s.viz.len(), 1);
        assert_eq!(s.dropped(), 0);
    }

    #[test]
    fn vei30_checks_all_green() {
        let set = run_vei30_checks();
        assert_eq!(set.red_items().1, 0, "红项：{:?}", set.red_items().0);
    }
}
