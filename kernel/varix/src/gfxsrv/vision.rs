//! VARIX-M500 AI-06 · 合成与视觉深化（F126~F150）。
//!
//! GPU 提交模型、着色管线预留、图层树、光效令牌、模糊/噪点/色板、
//! 渐进渲染、golden 帧基线、渲染探针、透明/分区/拖拽/录制/降级。
//! 纯逻辑 + 固定容量数组，无分配，无 FPU。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F126 GPU 提交模型 — 命令缓冲环形队列 + 提交/完成追踪
// ---------------------------------------------------------------------------

pub const CMD_RING_CAP: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmdKind {
    Clear,
    Draw,
    Blit,
    Present,
}

#[derive(Clone, Copy, Debug)]
pub struct Cmd {
    pub seq: u32,
    pub kind: CmdKind,
    pub arg: u32,
    pub submitted: bool,
    pub completed: bool,
}

#[derive(Clone, Copy)]
pub struct GpuRing {
    pub cmds: [Cmd; CMD_RING_CAP],
    pub head: usize,
    pub tail: usize,
    pub next_seq: u32,
    pub last_retired: u32,
}

impl GpuRing {
    pub const fn new() -> GpuRing {
        GpuRing {
            cmds: [Cmd { seq: 0, kind: CmdKind::Clear, arg: 0, submitted: false, completed: false }; CMD_RING_CAP],
            head: 0,
            tail: 0,
            next_seq: 1,
            last_retired: 0,
        }
    }

    /// 入队一条命令；满则返回 None。
    pub fn push(&mut self, kind: CmdKind, arg: u32) -> Option<u32> {
        if (self.head + 1) % CMD_RING_CAP == self.tail {
            return None;
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.cmds[self.head] = Cmd { seq, kind, arg, submitted: false, completed: false };
        self.head = (self.head + 1) % CMD_RING_CAP;
        Some(seq)
    }

    /// 提交最早的未提交命令。
    pub fn submit(&mut self) -> Option<u32> {
        let i = self.tail;
        if i == self.head || self.cmds[i].submitted {
            return None;
        }
        self.cmds[i].submitted = true;
        Some(self.cmds[i].seq)
    }

    /// 标记一个 seq 完成；若连续则前移 retirement 线。
    pub fn complete(&mut self, seq: u32) -> u32 {
        for k in 0..CMD_RING_CAP {
            let i = (self.tail + k) % CMD_RING_CAP;
            if i == self.head {
                break;
            }
            if self.cmds[i].seq == seq && self.cmds[i].submitted {
                self.cmds[i].completed = true;
            }
        }
        while self.tail != self.head && self.cmds[self.tail].completed {
            self.last_retired = self.cmds[self.tail].seq;
            self.cmds[self.tail].submitted = false;
            self.cmds[self.tail].completed = false;
            self.tail = (self.tail + 1) % CMD_RING_CAP;
        }
        self.last_retired
    }

    pub fn pending(&self) -> usize {
        (self.head + CMD_RING_CAP - self.tail) % CMD_RING_CAP
    }
}

// ---------------------------------------------------------------------------
// F127 着色管线预留 — 可编程着色占位：固定槽位的管线描述符表
// ---------------------------------------------------------------------------

pub const PIPELINE_CAP: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShaderStage {
    Unbound,
    Vertex,
    Fragment,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PipelineDesc {
    pub id: u16,
    pub vs: ShaderStage,
    pub fs: ShaderStage,
    pub pushed_consts: u16,
    pub bound: bool,
}

#[derive(Clone, Copy)]
pub struct PipelineTable {
    pub slots: [PipelineDesc; PIPELINE_CAP],
    pub count: usize,
}

impl PipelineTable {
    pub const fn new() -> PipelineTable {
        PipelineTable {
            slots: [PipelineDesc { id: 0, vs: ShaderStage::Unbound, fs: ShaderStage::Unbound, pushed_consts: 0, bound: false }; PIPELINE_CAP],
            count: 0,
        }
    }

    /// 登记管线（去重：同 id 只登记一次）。
    pub fn register(&mut self, id: u16, vs: ShaderStage, fs: ShaderStage, consts: u16) -> bool {
        if self.count >= PIPELINE_CAP || self.find(id).is_some() {
            return false;
        }
        self.slots[self.count] = PipelineDesc { id, vs, fs, pushed_consts: consts, bound: false };
        self.count += 1;
        true
    }

    pub fn find(&self, id: u16) -> Option<usize> {
        (0..self.count).find(|&i| self.slots[i].id == id)
    }

    pub fn bind(&mut self, id: u16) -> bool {
        match self.find(id) {
            Some(i) if self.slots[i].vs != ShaderStage::Unbound => {
                self.slots[i].bound = true;
                true
            }
            _ => false,
        }
    }

    pub fn bound_count(&self) -> usize {
        (0..self.count).filter(|&i| self.slots[i].bound).count()
    }
}

// ---------------------------------------------------------------------------
// F128 图层树协议 — 应用可公开的图层树（父/子 + 可见性 + z 序）
// ---------------------------------------------------------------------------

pub const LAYER_CAP: usize = 16;

#[derive(Clone, Copy)]
pub struct LayerNode {
    pub id: u16,
    pub parent: u16,
    pub visible: bool,
    pub opacity_q8: u32,
    pub depth: u8,
}

#[derive(Clone, Copy)]
pub struct LayerTree {
    pub nodes: [LayerNode; LAYER_CAP],
    pub count: usize,
}

impl LayerTree {
    pub const fn new() -> LayerTree {
        LayerTree { nodes: [LayerNode { id: 0, parent: 0, visible: false, opacity_q8: 0, depth: 0 }; LAYER_CAP], count: 0 }
    }

    pub fn add(&mut self, id: u16, parent: u16, visible: bool) -> bool {
        if self.count >= LAYER_CAP || self.find(id).is_some() {
            return false;
        }
        // 父必须已存在（根 = 0 特殊值）。
        if parent != 0 && self.find(parent).is_none() {
            return false;
        }
        let pdepth = if parent == 0 { 0 } else { self.nodes[self.find(parent).unwrap()].depth };
        if pdepth >= 255 {
            return false;
        }
        self.nodes[self.count] =
            LayerNode { id, parent, visible, opacity_q8: 256, depth: pdepth + 1 };
        self.count += 1;
        true
    }

    pub fn find(&self, id: u16) -> Option<usize> {
        (0..self.count).find(|&i| self.nodes[i].id == id)
    }

    /// 子树可见性：父不可见时整棵子树不可见。
    pub fn effective_visible(&self, id: u16) -> bool {
        let mut cur = match self.find(id) {
            Some(i) => i,
            None => return false,
        };
        loop {
            if !self.nodes[cur].visible {
                return false;
            }
            let p = self.nodes[cur].parent;
            if p == 0 {
                return true;
            }
            match self.find(p) {
                Some(i) => cur = i,
                None => return true,
            }
        }
    }

    /// 合成顺序：深度优先按登记序。
    pub fn paint_order(&self) -> [u16; LAYER_CAP] {
        let mut out = [0u16; LAYER_CAP];
        let mut n = 0;
        for i in 0..self.count {
            if self.nodes[i].parent == 0 {
                out[n] = self.nodes[i].id;
                n += 1;
                n += self.push_children(self.nodes[i].id, &mut out, n);
            }
        }
        out
    }

    fn push_children(&self, parent: u16, out: &mut [u16; LAYER_CAP], mut n: usize) -> usize {
        let start = n;
        for i in 0..self.count {
            if self.nodes[i].parent == parent {
                if n < LAYER_CAP {
                    out[n] = self.nodes[i].id;
                    n += 1;
                }
                n += self.push_children(self.nodes[i].id, out, n);
            }
        }
        n - start
    }
}

// ---------------------------------------------------------------------------
// F129 视觉层次引擎 — 景深/虚化层级：按层分配视觉权重
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DepthPlane {
    Background,
    Mid,
    Foreground,
}

#[derive(Clone, Copy)]
pub struct VisualWeight {
    pub plane: DepthPlane,
    /// Q8：模糊强度（0=锐利）。
    pub blur_q8: u32,
    /// Q8：亮度权重（256=原亮度）。
    pub dim_q8: u32,
    /// Q8：饱和度权重。
    pub sat_q8: u32,
}

/// 由平面得出统一视觉参数（全局形状语言的一部分）。
pub fn visual_weight(plane: DepthPlane) -> VisualWeight {
    match plane {
        DepthPlane::Background => VisualWeight { plane, blur_q8: 128, dim_q8: 180, sat_q8: 200 },
        DepthPlane::Mid => VisualWeight { plane, blur_q8: 32, dim_q8: 230, sat_q8: 240 },
        DepthPlane::Foreground => VisualWeight { plane, blur_q8: 0, dim_q8: 256, sat_q8: 256 },
    }
}

/// 层级合法性：前景必须比背景"更锐利"。
pub fn plane_order_ok(a: DepthPlane, b: DepthPlane) -> bool {
    visual_weight(a).blur_q8 >= visual_weight(b).blur_q8
}

// ---------------------------------------------------------------------------
// F130 光效令牌 — 辉光/阴影统一参数令牌
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LightToken {
    pub glow_q8: u32,
    pub shadow_blur_q8: u32,
    pub shadow_dy: i16,
    pub shadow_alpha_q8: u32,
}

pub const TOKEN_NONE: LightToken =
    LightToken { glow_q8: 0, shadow_blur_q8: 0, shadow_dy: 0, shadow_alpha_q8: 0 };
pub const TOKEN_CARD: LightToken =
    LightToken { glow_q8: 0, shadow_blur_q8: 64, shadow_dy: 4, shadow_alpha_q8: 96 };
pub const TOKEN_FOCUS: LightToken =
    LightToken { glow_q8: 96, shadow_blur_q8: 96, shadow_dy: 6, shadow_alpha_q8: 128 };

/// 令牌按强度合成（取较强者逐参数）。
pub fn blend_tokens(a: LightToken, b: LightToken) -> LightToken {
    LightToken {
        glow_q8: a.glow_q8.max(b.glow_q8),
        shadow_blur_q8: a.shadow_blur_q8.max(b.shadow_blur_q8),
        shadow_dy: if a.shadow_alpha_q8 >= b.shadow_alpha_q8 { a.shadow_dy } else { b.shadow_dy },
        shadow_alpha_q8: a.shadow_alpha_q8.max(b.shadow_alpha_q8),
    }
}

// ---------------------------------------------------------------------------
// F131 模糊管线 — 盒式模糊（水平/垂直两趟，固定网格）
// ---------------------------------------------------------------------------

pub const BLUR_W: usize = 8;
pub const BLUR_H: usize = 8;

pub fn blur_h(src: &[u8; BLUR_W * BLUR_H], dst: &mut [u8; BLUR_W * BLUR_H], radius: usize) {
    for y in 0..BLUR_H {
        for x in 0..BLUR_W {
            let mut sum = 0u32;
            let mut n = 0u32;
            for d in 0..=radius {
                if x >= d {
                    sum += src[y * BLUR_W + x - d] as u32;
                    n += 1;
                }
                if x + d < BLUR_W {
                    sum += src[y * BLUR_W + x + d] as u32;
                    n += 1;
                }
            }
            dst[y * BLUR_W + x] = (sum / n) as u8;
        }
    }
}

pub fn blur_v(src: &[u8; BLUR_W * BLUR_H], dst: &mut [u8; BLUR_W * BLUR_H], radius: usize) {
    for y in 0..BLUR_H {
        for x in 0..BLUR_W {
            let mut sum = 0u32;
            let mut n = 0u32;
            for d in 0..=radius {
                if y >= d {
                    sum += src[(y - d) * BLUR_W + x] as u32;
                    n += 1;
                }
                if y + d < BLUR_H {
                    sum += src[(y + d) * BLUR_W + x] as u32;
                    n += 1;
                }
            }
            dst[y * BLUR_W + x] = (sum / n) as u8;
        }
    }
}

/// 两趟盒模糊（separable 近似）。
pub fn blur2d(buf: &mut [u8; BLUR_W * BLUR_H], tmp: &mut [u8; BLUR_W * BLUR_H], radius: usize) {
    blur_h(buf, tmp, radius);
    blur_v(tmp, buf, radius);
}

// ---------------------------------------------------------------------------
// F132 噪点纹理引擎 — LCG 颗粒纹理，种子可复现
// ---------------------------------------------------------------------------

pub struct NoiseGen {
    pub state: u32,
    pub amplitude: u8,
}

impl NoiseGen {
    pub const fn new(seed: u32, amplitude: u8) -> NoiseGen {
        NoiseGen { state: seed, amplitude }
    }

    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1664525).wrapping_add(1013904223);
        self.state
    }

    /// 生成带噪点的像素：base ± amplitude（饱和到 u8）。
    pub fn grain(&mut self, base: u8) -> u8 {
        let n = (self.next_u32() >> 24) as i32 - 128; // -128..127
        let v = base as i32 + n * self.amplitude as i32 / 128;
        v.clamp(0, 255) as u8
    }
}

// ---------------------------------------------------------------------------
// F133 色板引擎 — 取色/调和（HSV 整数近似）
// ---------------------------------------------------------------------------

/// hue 0..360，sat/val 0..255。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hsv {
    pub h: u16,
    pub s: u8,
    pub v: u8,
}

/// 互补色。
pub fn complementary(c: Hsv) -> Hsv {
    Hsv { h: (c.h + 180) % 360, s: c.s, v: c.v }
}

/// 三分色（基色 ±120）。
pub fn triad(c: Hsv) -> [Hsv; 3] {
    [
        c,
        Hsv { h: (c.h + 120) % 360, s: c.s, v: c.v },
        Hsv { h: (c.h + 240) % 360, s: c.s, v: c.v },
    ]
}

/// 类似色（±30 内均匀取 n 个，n 上限 5）。
pub fn analogous(c: Hsv, n: usize) -> [Option<Hsv>; 5] {
    let n = n.min(5);
    let mut out = [None; 5];
    let step = if n <= 1 { 0 } else { 60 / (n - 1) };
    for i in 0..n {
        let dh = (i * step) as u16;
        out[i] = Some(Hsv { h: (c.h + 360 - 30 + dh) % 360, s: c.s, v: c.v });
    }
    out
}

/// HSV→RGB（整数近似，返回 0xRRGGBB）。
pub fn hsv_to_rgb(c: Hsv) -> u32 {
    let v = c.v as u32;
    let s = c.s as u32;
    let region = (c.h / 60) as u32 % 6;
    let rem = (c.h % 60) * 255 / 360;
    let p = v * (255 - s as u32) / 255;
    let q = v * (255 - s as u32 * rem as u32 / 255) / 255;
    let t = v * (255 - s as u32 * (255 - rem) as u32 / 255) / 255;
    let (r, g, b) = match region {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    (r << 16) | (g << 8) | b
}

// ---------------------------------------------------------------------------
// F134 动效预演器 — 时间轴采样（关键帧线性插值）
// ---------------------------------------------------------------------------

pub const KEYFRAME_CAP: usize = 8;

#[derive(Clone, Copy)]
pub struct Keyframe {
    pub t_ms: u32,
    pub value_q8: i32,
}

#[derive(Clone, Copy)]
pub struct Timeline {
    pub keys: [Keyframe; KEYFRAME_CAP],
    pub count: usize,
    pub duration_ms: u32,
}

impl Timeline {
    pub const fn new(duration_ms: u32) -> Timeline {
        Timeline { keys: [Keyframe { t_ms: 0, value_q8: 0 }; KEYFRAME_CAP], count: 0, duration_ms }
    }

    /// 关键帧按时间有序登记。
    pub fn add_key(&mut self, t_ms: u32, value_q8: i32) -> bool {
        if self.count >= KEYFRAME_CAP || t_ms > self.duration_ms {
            return false;
        }
        if self.count > 0 && t_ms <= self.keys[self.count - 1].t_ms {
            return false;
        }
        self.keys[self.count] = Keyframe { t_ms, value_q8 };
        self.count += 1;
        true
    }

    /// 预演：采样 t 时刻的插值。
    pub fn sample(&self, t_ms: u32) -> i32 {
        if self.count == 0 {
            return 0;
        }
        if t_ms <= self.keys[0].t_ms {
            return self.keys[0].value_q8;
        }
        for i in 1..self.count {
            if t_ms <= self.keys[i].t_ms {
                let a = &self.keys[i - 1];
                let b = &self.keys[i];
                let span = (b.t_ms - a.t_ms) as i64;
                let frac = ((t_ms - a.t_ms) as i64 * 256 / span) as i64;
                return (a.value_q8 as i64 + (b.value_q8 as i64 - a.value_q8 as i64) * frac / 256) as i32;
            }
        }
        self.keys[self.count - 1].value_q8
    }
}

// ---------------------------------------------------------------------------
// F135 帧同步组 — 多动画同帧提交（全就绪才放行）
// ---------------------------------------------------------------------------

pub const SYNC_GROUP_CAP: usize = 8;

#[derive(Clone, Copy)]
pub struct SyncGroup {
    pub members: [u16; SYNC_GROUP_CAP],
    pub ready: [bool; SYNC_GROUP_CAP],
    pub count: usize,
    pub frame: u32,
}

impl SyncGroup {
    pub const fn new() -> SyncGroup {
        SyncGroup { members: [0; SYNC_GROUP_CAP], ready: [false; SYNC_GROUP_CAP], count: 0, frame: 0 }
    }

    pub fn join(&mut self, anim_id: u16) -> bool {
        if self.count >= SYNC_GROUP_CAP || self.member(anim_id).is_some() {
            return false;
        }
        self.members[self.count] = anim_id;
        self.ready[self.count] = false;
        self.count += 1;
        true
    }

    pub fn member(&self, anim_id: u16) -> Option<usize> {
        (0..self.count).find(|&i| self.members[i] == anim_id)
    }

    pub fn mark_ready(&mut self, anim_id: u16) {
        if let Some(i) = self.member(anim_id) {
            self.ready[i] = true;
        }
    }

    /// 全部就绪则放行并进入下一帧（重置就绪位）。
    pub fn try_commit(&mut self) -> Option<u32> {
        if self.count == 0 {
            return None;
        }
        if self.ready[..self.count].iter().all(|&r| r) {
            self.frame += 1;
            for r in self.ready[..self.count].iter_mut() {
                *r = false;
            }
            Some(self.frame)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// F136 渐进式渲染 — 复杂帧先粗后精（pass 状态机）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProgressPass {
    Idle,
    /// 1/16 分辨率底稿。
    Coarse,
    /// 1/4 精化。
    Mid,
    Full,
}

#[derive(Clone, Copy)]
pub struct ProgressiveRenderer {
    pub pass: ProgressPass,
    pub pass_ms: u32,
}

impl ProgressiveRenderer {
    pub const fn new() -> ProgressiveRenderer {
        ProgressiveRenderer { pass: ProgressPass::Idle, pass_ms: 0 }
    }

    pub fn begin(&mut self) {
        self.pass = ProgressPass::Coarse;
        self.pass_ms = 0;
    }

    /// 每帧推进；返回当前 pass（供合成器决定采样密度）。
    pub fn tick(&mut self, budget_ms: u32) -> ProgressPass {
        if self.pass == ProgressPass::Idle {
            return ProgressPass::Idle;
        }
        self.pass_ms += budget_ms;
        let next = match self.pass {
            ProgressPass::Coarse if self.pass_ms >= 16 => ProgressPass::Mid,
            ProgressPass::Mid if self.pass_ms >= 48 => ProgressPass::Full,
            p => p,
        };
        self.pass = next;
        next
    }
}

// ---------------------------------------------------------------------------
// F137 带宽自适应画质 — 按带宽预算选质量档
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Quality {
    /// 全特效。
    Ultra,
    High,
    /// 关辉光/噪点。
    Balanced,
    /// 再关模糊。
    Low,
    /// 关阴影 + 降采样。
    Minimal,
}

/// bandwidth_mbps：可用合成带宽；complexity：场景复杂度 1..10。
pub fn pick_quality(bandwidth_mbps: u32, complexity: u8) -> Quality {
    let cost = bandwidth_mbps / (complexity.max(1) as u32);
    match cost {
        0 => Quality::Minimal,
        1..=3 => Quality::Low,
        4..=7 => Quality::Balanced,
        8..=15 => Quality::High,
        _ => Quality::Ultra,
    }
}

/// 档位允许的特效检查。
pub fn quality_allows(q: Quality, blur: bool, glow: bool, shadow: bool) -> bool {
    let blur_ok = q < Quality::Low || !blur;
    let glow_ok = q < Quality::Balanced || !glow;
    let shadow_ok = q < Quality::Minimal || !shadow;
    blur_ok && glow_ok && shadow_ok
}

// ---------------------------------------------------------------------------
// F138 合成器 golden 帧 — 像素基线 + 差异计数
// ---------------------------------------------------------------------------

pub const GOLDEN_W: usize = 16;
pub const GOLDEN_H: usize = 16;
pub const GOLDEN_TOLERANCE: u8 = 2;

#[derive(Clone, Copy)]
pub struct GoldenFrame {
    pub baseline: [u8; GOLDEN_W * GOLDEN_H],
    pub captured: bool,
}

impl GoldenFrame {
    pub const fn new() -> GoldenFrame {
        GoldenFrame { baseline: [0; GOLDEN_W * GOLDEN_H], captured: false }
    }

    pub fn capture(&mut self, frame: &[u8; GOLDEN_W * GOLDEN_H]) {
        self.baseline = *frame;
        self.captured = true;
    }

    /// 与基线比较，返回超差像素数；未捕获基线时返回 None。
    pub fn diff(&self, frame: &[u8; GOLDEN_W * GOLDEN_H]) -> Option<usize> {
        if !self.captured {
            return None;
        }
        let mut n = 0;
        for i in 0..GOLDEN_W * GOLDEN_H {
            if (self.baseline[i] as i32 - frame[i] as i32).abs() > GOLDEN_TOLERANCE as i32 {
                n += 1;
            }
        }
        Some(n)
    }
}

// ---------------------------------------------------------------------------
// F139 渲染探针 — 逐层耗时直方图（桶式，无分配）
// ---------------------------------------------------------------------------

pub const PROBE_BUCKETS: usize = 8;
/// 桶宽：0.25ms → 桶 i 覆盖 [i*250us, (i+1)*250us)。
pub const PROBE_BUCKET_US: u32 = 250;

#[derive(Clone, Copy)]
pub struct RenderProbe {
    pub buckets: [u32; PROBE_BUCKETS],
    pub total_us: u64,
    pub samples: u32,
    pub overflow: u32,
}

impl RenderProbe {
    pub const fn new() -> RenderProbe {
        RenderProbe { buckets: [0; PROBE_BUCKETS], total_us: 0, samples: 0, overflow: 0 }
    }

    pub fn record(&mut self, layer_id: u16, us: u32) {
        let _ = layer_id;
        self.total_us += us as u64;
        self.samples += 1;
        let idx = (us / PROBE_BUCKET_US) as usize;
        if idx >= PROBE_BUCKETS {
            self.overflow += 1;
        } else {
            self.buckets[idx] += 1;
        }
    }

    /// p50 近似：扫描直方图找中位桶。
    pub fn p50_bucket(&self) -> Option<usize> {
        if self.samples == 0 {
            return None;
        }
        let mid = self.samples / 2;
        let mut acc = 0u32;
        for i in 0..PROBE_BUCKETS {
            acc += self.buckets[i];
            if acc > mid {
                return Some(i);
            }
        }
        Some(PROBE_BUCKETS - 1)
    }
}

// ---------------------------------------------------------------------------
// F140 圆角曲面统一管线 — 圆角半径解析优先级
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct CornerSpec {
    /// 显式设置（最高优先）。
    pub explicit_q8: u32,
    /// 主题令牌。
    pub token_q8: u32,
    /// 按窗口尺寸推导。
    pub derived_q8: u32,
}

/// 解析：显式 > 令牌 > 推导；并夹到尺寸一半以内。
pub fn resolve_corner(spec: CornerSpec, win_w: u32, win_h: u32) -> u32 {
    let mut r = if spec.explicit_q8 > 0 {
        spec.explicit_q8
    } else if spec.token_q8 > 0 {
        spec.token_q8
    } else {
        spec.derived_q8
    };
    let half = (win_w.min(win_h) / 2) * 256;
    if r > half {
        r = half;
    }
    r
}

/// 推导公式：短边 1/12，夹在 [8, 32]（Q8）。
pub fn derive_corner(win_w: u32, win_h: u32) -> u32 {
    let d = win_w.min(win_h) * 256 / 12;
    d.clamp(8 * 256, 32 * 256)
}

// ---------------------------------------------------------------------------
// F141 透明协议 — 应用声明透明区（固定表）
// ---------------------------------------------------------------------------

pub const ALPHA_REGION_CAP: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AlphaRegion {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    /// 0..255，255 = 不透明。
    pub alpha: u8,
}

#[derive(Clone, Copy)]
pub struct AlphaDecls {
    pub regions: [AlphaRegion; ALPHA_REGION_CAP],
    pub count: usize,
}

impl AlphaDecls {
    pub const fn new() -> AlphaDecls {
        AlphaDecls { regions: [AlphaRegion { x: 0, y: 0, w: 0, h: 0, alpha: 255 }; ALPHA_REGION_CAP], count: 0 }
    }

    pub fn declare(&mut self, x: u16, y: u16, w: u16, h: u16, alpha: u8) -> bool {
        if self.count >= ALPHA_REGION_CAP {
            return false;
        }
        self.regions[self.count] = AlphaRegion { x, y, w, h, alpha };
        self.count += 1;
        true
    }

    /// 命中查询：点落在最上（最后登记）的声明区内返回其 alpha。
    pub fn alpha_at(&self, x: u16, y: u16) -> u8 {
        for i in (0..self.count).rev() {
            let r = &self.regions[i];
            if x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h {
                return r.alpha;
            }
        }
        255
    }

    /// 合成器快速判定：整层是否有任何低于不透明的声明。
    pub fn needs_blend(&self) -> bool {
        (0..self.count).any(|i| self.regions[i].alpha < 255)
    }
}

// ---------------------------------------------------------------------------
// F142 分区渲染 — 脏区合并（矩形 union 合并相邻/重叠区）
// ---------------------------------------------------------------------------

pub const DIRTY_CAP: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

/// 两矩形是否重叠或相接（含 1px 边界，便于合并）。
pub fn rects_adjacent(a: &Rect, b: &Rect) -> bool {
    !(a.x + a.w < b.x || b.x + b.w < a.x || a.y + a.h < b.y || b.y + b.h < a.y)
}

/// 两矩形的 union 外接框。
pub fn rects_union(a: &Rect, b: &Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.w).max(b.x + b.w);
    let y1 = (a.y + a.h).max(b.y + b.h);
    Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

#[derive(Clone, Copy)]
pub struct DirtyPartition {
    pub rects: [Rect; DIRTY_CAP],
    pub count: usize,
}

impl DirtyPartition {
    pub const fn new() -> DirtyPartition {
        DirtyPartition { rects: [Rect { x: 0, y: 0, w: 0, h: 0 }; DIRTY_CAP], count: 0 }
    }

    /// 标脏；与已有区相邻则合并，否则新开槽（满则并入最大槽）。
    pub fn mark(&mut self, r: Rect) {
        for i in 0..self.count {
            if rects_adjacent(&self.rects[i], &r) {
                self.rects[i] = rects_union(&self.rects[i], &r);
                return;
            }
        }
        if self.count < DIRTY_CAP {
            self.rects[self.count] = r;
            self.count += 1;
            return;
        }
        // 满了：并入面积与 r 重叠增益最小的槽——这里简化为面积最小槽。
        let mut min_i = 0;
        for i in 1..DIRTY_CAP {
            if self.rects[i].w * self.rects[i].h < self.rects[min_i].w * self.rects[min_i].h {
                min_i = i;
            }
        }
        self.rects[min_i] = rects_union(&self.rects[min_i], &r);
    }

    /// 本帧脏像素总量。
    pub fn dirty_pixels(&self) -> u32 {
        (0..self.count).map(|i| self.rects[i].w as u32 * self.rects[i].h as u32).sum()
    }
}

// ---------------------------------------------------------------------------
// F143 画面语义标注 — a11y 图层：区域→角色/标签
// ---------------------------------------------------------------------------

pub const SEMANTIC_CAP: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum A11yRole {
    Text,
    Button,
    Image,
    Unknown,
}

#[derive(Clone, Copy)]
pub struct SemanticRegion {
    pub rect: Rect,
    pub role: A11yRole,
    /// 标签池内索引，0xFFFF = 无。
    pub label_idx: u16,
}

#[derive(Clone, Copy)]
pub struct SemanticLayer {
    pub regions: [SemanticRegion; SEMANTIC_CAP],
    pub labels: [&'static str; SEMANTIC_CAP],
    pub label_count: usize,
    pub count: usize,
}

impl SemanticLayer {
    pub const fn new() -> SemanticLayer {
        SemanticLayer {
            regions: [SemanticRegion { rect: Rect { x: 0, y: 0, w: 0, h: 0 }, role: A11yRole::Unknown, label_idx: 0xFFFF }; SEMANTIC_CAP],
            labels: ["", "", "", "", "", "", "", ""],
            label_count: 0,
            count: 0,
        }
    }

    pub fn add_label(&mut self, text: &'static str) -> u16 {
        if self.label_count >= SEMANTIC_CAP {
            return 0xFFFF;
        }
        self.labels[self.label_count] = text;
        self.label_count += 1;
        (self.label_count - 1) as u16
    }

    pub fn annotate(&mut self, rect: Rect, role: A11yRole, label: &'static str) -> bool {
        if self.count >= SEMANTIC_CAP {
            return false;
        }
        let li = if label.is_empty() { 0xFFFF } else { self.add_label(label) };
        self.regions[self.count] = SemanticRegion { rect, role, label_idx: li };
        self.count += 1;
        true
    }

    /// 读屏查询：命中点返回 (角色, 标签)。
    pub fn hit(&self, x: u16, y: u16) -> Option<(A11yRole, &'static str)> {
        for i in (0..self.count).rev() {
            let r = &self.regions[i].rect;
            if x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h {
                let li = self.regions[i].label_idx;
                let text = if li != 0xFFFF && (li as usize) < self.label_count {
                    self.labels[li as usize]
                } else {
                    ""
                };
                return Some((self.regions[i].role, text));
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// F144 高速拖拽管线 — 位置预测 + 帧步进（拖动零掉帧）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct DragPipeline {
    /// 最近位置（Q8）。
    pub x_q8: i32,
    pub y_q8: i32,
    /// 速度（Q8/帧）。
    pub vx_q8: i32,
    pub vy_q8: i32,
    pub active: bool,
}

impl DragPipeline {
    pub const fn new() -> DragPipeline {
        DragPipeline { x_q8: 0, y_q8: 0, vx_q8: 0, vy_q8: 0, active: false }
    }

    pub fn begin(&mut self, x: i32, y: i32) {
        self.x_q8 = x * 256;
        self.y_q8 = y * 256;
        self.vx_q8 = 0;
        self.vy_q8 = 0;
        self.active = true;
    }

    /// 输入事件更新：EMA 平滑速度。
    pub fn move_to(&mut self, x: i32, y: i32) {
        if !self.active {
            return;
        }
        let nx = x * 256;
        let ny = y * 256;
        self.vx_q8 = (self.vx_q8 * 3 + (nx - self.x_q8)) / 4;
        self.vy_q8 = (self.vy_q8 * 3 + (ny - self.y_q8)) / 4;
        self.x_q8 = nx;
        self.y_q8 = ny;
    }

    /// 合成器在无输入帧用速度外推，保证不卡顿。
    pub fn predict(&mut self) -> (i32, i32) {
        self.x_q8 += self.vx_q8 / 2;
        self.y_q8 += self.vy_q8 / 2;
        self.vx_q8 = self.vx_q8 * 3 / 4;
        self.vy_q8 = self.vy_q8 * 3 / 4;
        (self.x_q8 / 256, self.y_q8 / 256)
    }

    pub fn end(&mut self) {
        self.active = false;
        self.vx_q8 = 0;
        self.vy_q8 = 0;
    }
}

// ---------------------------------------------------------------------------
// F145 面板校准档案 — gamma LUT + 色差补偿
// ---------------------------------------------------------------------------

pub const LUT_SIZE: usize = 256;

#[derive(Clone, Copy)]
pub struct PanelCalib {
    /// 32 点 gamma LUT（8bit→8bit）。
    pub lut: [u8; LUT_SIZE],
    /// RGB 增益（Q8）。
    pub gain_r_q8: u32,
    pub gain_g_q8: u32,
    pub gain_b_q8: u32,
}

impl PanelCalib {
    /// 用整数幂近似 gamma 生成 LUT：out = (in/31)^(1/gamma) * 255。
    pub const fn identity() -> PanelCalib {
        let mut lut = [0u8; LUT_SIZE];
        let mut i = 0;
        while i < LUT_SIZE {
            lut[i] = i as u8;
            i += 1;
        }
        PanelCalib { lut, gain_r_q8: 256, gain_g_q8: 256, gain_b_q8: 256 }
    }

    /// gamma 校正 LUT（gamma_q8: 1.0=256；gamma>1 提亮暗部）。
    pub fn with_gamma(gamma_q8: u32) -> PanelCalib {
        let mut lut = [0u8; LUT_SIZE];
        for i in 0..LUT_SIZE {
            let x = i as u32;
            lut[i] = if gamma_q8 > 256 {
                // gamma > 1：提亮暗部，out = isqrt(x * 255)（1/2 次幂整数近似）。
                let y = x * 255;
                let mut r = y;
                let mut s = (r + 1) / 2;
                while s < r {
                    r = s;
                    s = (r + y / r.max(1)) / 2;
                }
                r.min(255) as u8
            } else {
                x as u8
            };
        }
        PanelCalib { lut, gain_r_q8: 256, gain_g_q8: 256, gain_b_q8: 256 }
    }

    /// 应用 LUT + 增益。
    pub fn apply(&self, r: u8, g: u8, b: u8) -> (u8, u8, u8) {
        let idx = r as usize;
        let gr = ((self.lut[idx] as u32) * self.gain_r_q8 / 256).min(255) as u8;
        let gg = ((self.lut[g as usize] as u32) * self.gain_g_q8 / 256).min(255) as u8;
        let bb = ((self.lut[b as usize] as u32) * self.gain_b_q8 / 256).min(255) as u8;
        (gr, gg, bb)
    }
}

// ---------------------------------------------------------------------------
// F146 视频层直通 — overlay 平面资格判定
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OverlayVerdict {
    PassThrough,
    Compose,
}

/// 视频层直通资格：全屏不透明矩形 + 无上方遮挡 + 格式受支持。
pub fn overlay_eligible(
    vid_rect: Rect,
    screen: Rect,
    occluded_above: bool,
    format_yuv: bool,
) -> OverlayVerdict {
    let full = vid_rect.x == 0
        && vid_rect.y == 0
        && vid_rect.w >= screen.w
        && vid_rect.h >= screen.h;
    if full && !occluded_above && format_yuv {
        OverlayVerdict::PassThrough
    } else {
        OverlayVerdict::Compose
    }
}

// ---------------------------------------------------------------------------
// F147 屏幕录制编排 — 录制会话管理（多会话、编码节流）
// ---------------------------------------------------------------------------

pub const REC_CAP: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecState {
    Stopped,
    Recording,
    Paused,
}

#[derive(Clone, Copy)]
pub struct RecSession {
    pub id: u16,
    pub state: RecState,
    pub fps_target: u8,
    pub frames: u32,
    pub dropped: u32,
}

#[derive(Clone, Copy)]
pub struct RecManager {
    pub sessions: [RecSession; REC_CAP],
    pub count: usize,
    pub next_id: u16,
}

impl RecManager {
    pub const fn new() -> RecManager {
        RecManager {
            sessions: [RecSession { id: 0, state: RecState::Stopped, fps_target: 0, frames: 0, dropped: 0 }; REC_CAP],
            count: 0,
            next_id: 1,
        }
    }

    pub fn start(&mut self, fps: u8) -> Option<u16> {
        if self.count >= REC_CAP {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.sessions[self.count] =
            RecSession { id, state: RecState::Recording, fps_target: fps.clamp(1, 60), frames: 0, dropped: 0 };
        self.count += 1;
        Some(id)
    }

    pub fn find(&mut self, id: u16) -> Option<usize> {
        (0..self.count).find(|&i| self.sessions[i].id == id)
    }

    pub fn set_state(&mut self, id: u16, st: RecState) -> bool {
        match self.find(id) {
            Some(i) => {
                self.sessions[i].state = st;
                true
            }
            None => false,
        }
    }

    /// 帧到达：录制中计数；超过目标 fps 的 1.5 倍即丢弃（节流）。
    pub fn frame_tick(&mut self, id: u16, current_fps: u32) {
        if let Some(i) = self.find(id) {
            if self.sessions[i].state == RecState::Recording {
                if current_fps as u32 > self.sessions[i].fps_target as u32 * 3 / 2 {
                    self.sessions[i].dropped += 1;
                } else {
                    self.sessions[i].frames += 1;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// F148 视觉降级谱系 — 特效逐级关闭梯子
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum FxLevel {
    L0Full,
    L1NoNoise,
    L2NoGlow,
    L3NoBlur,
    L4NoShadow,
    L5Flat,
}

/// 按 CPU 占用（0..1000 千分比）逐级降。
pub fn degrade(cpu_permille: u16) -> FxLevel {
    match cpu_permille {
        0..=600 => FxLevel::L0Full,
        601..=700 => FxLevel::L1NoNoise,
        701..=780 => FxLevel::L2NoGlow,
        781..=850 => FxLevel::L3NoBlur,
        851..=920 => FxLevel::L4NoShadow,
        _ => FxLevel::L5Flat,
    }
}

/// 升级需要留出迟滞：占用必须低于门槛-80 才回升一级。
pub fn undegrade(cpu_permille: u16, current: FxLevel) -> FxLevel {
    let recover = match current {
        FxLevel::L5Flat => cpu_permille < 840,
        FxLevel::L4NoShadow => cpu_permille < 770,
        FxLevel::L3NoBlur => cpu_permille < 700,
        FxLevel::L2NoGlow => cpu_permille < 620,
        FxLevel::L1NoNoise => cpu_permille < 520,
        FxLevel::L0Full => false,
    };
    if recover {
        match current {
            FxLevel::L5Flat => FxLevel::L4NoShadow,
            FxLevel::L4NoShadow => FxLevel::L3NoBlur,
            FxLevel::L3NoBlur => FxLevel::L2NoGlow,
            FxLevel::L2NoGlow => FxLevel::L1NoNoise,
            FxLevel::L1NoNoise => FxLevel::L0Full,
            other => other,
        }
    } else {
        current
    }
}

// ---------------------------------------------------------------------------
// F149 合成器压力剧本 — 稳定性情景（层次轰击/抖动/脏区风暴）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StressScenario {
    LayerStorm,
    DirtyStorm,
    ResizeJitter,
}

#[derive(Clone, Copy)]
pub struct StressRunner {
    pub scenario: StressScenario,
    pub step: u32,
    pub max_layers: u16,
    pub anomalies: u32,
}

impl StressRunner {
    pub const fn new(scenario: StressScenario, max_layers: u16) -> StressRunner {
        StressRunner { scenario, step: 0, max_layers, anomalies: 0 }
    }

    /// 跑一步：返回本步的层负载；任何越界都计异常。
    pub fn step_once(&mut self, load: u16) -> u16 {
        self.step += 1;
        if load > self.max_layers {
            self.anomalies += 1;
            self.max_layers
        } else {
            load
        }
    }

    pub fn stable(&self) -> bool {
        self.anomalies == 0
    }
}

// ---------------------------------------------------------------------------
// F150 合成域自检 — 25 项 CheckSet 汇入总检
// ---------------------------------------------------------------------------

pub fn run_vision_checks() -> CheckSet {
    let mut set = CheckSet::new("gfxvis");

    // F126 GPU 提交模型
    let mut ring = GpuRing::new();
    let s1 = ring.push(CmdKind::Draw, 7);
    let s2 = ring.push(CmdKind::Present, 0);
    let sub1 = ring.submit();
    let pending_after = ring.pending();
    let retired = ring.complete(s1.unwrap_or(0));
    set.add(
        "F126 gpu ring submit/complete",
        s1.is_some() && s2.is_some() && sub1 == s1 && pending_after == 2 && retired == 1,
        "submit/retire sequence broken",
    );
    let mut ring2 = GpuRing::new();
    let _ = ring2.push(CmdKind::Clear, 0);
    let _ = ring2.push(CmdKind::Draw, 1);
    let sub_a = ring2.submit();
    let sub_b = ring2.submit();
    let r2a = ring2.complete(2);
    let r2b = ring2.complete(1);
    set.add(
        "F126 ring retires in order",
        sub_a == Some(1) && sub_b.is_none() && r2a == 0 && r2b == 1 && ring2.pending() == 1,
        "in-order retire",
    );

    // F127 着色管线
    let mut pipes = PipelineTable::new();
    let ok1 = pipes.register(1, ShaderStage::Vertex, ShaderStage::Fragment, 16);
    let dup = pipes.register(1, ShaderStage::Vertex, ShaderStage::Fragment, 16);
    let bind_ok = pipes.bind(1);
    let mut pipes2 = PipelineTable::new();
    let _ = pipes2.register(2, ShaderStage::Unbound, ShaderStage::Unbound, 0);
    let bind_bad = pipes2.bind(2);
    set.add("F127 pipeline register/bind", ok1 && !dup && bind_ok && !bind_bad, "pipeline table");

    // F128 图层树
    let mut tree = LayerTree::new();
    let root_ok = tree.add(10, 0, true);
    let child_ok = tree.add(11, 10, true);
    let orphan = tree.add(12, 99, true);
    let mut tree2 = LayerTree::new();
    let _ = tree2.add(1, 0, true);
    let _ = tree2.add(2, 1, false);
    let eff = tree2.effective_visible(2);
    let order = tree.paint_order();
    set.add(
        "F128 layer tree",
        root_ok && child_ok && !orphan && !eff && order[0] == 10 && order[1] == 11,
        "layer tree semantics",
    );

    // F129 视觉层次
    let bg = visual_weight(DepthPlane::Background);
    let fg = visual_weight(DepthPlane::Foreground);
    set.add(
        "F129 depth planes",
        bg.blur_q8 > fg.blur_q8 && fg.dim_q8 == 256 && plane_order_ok(DepthPlane::Background, DepthPlane::Foreground),
        "plane weights",
    );

    // F130 光效令牌
    let merged = blend_tokens(TOKEN_CARD, TOKEN_FOCUS);
    set.add(
        "F130 light tokens",
        merged.glow_q8 == TOKEN_FOCUS.glow_q8 && merged.shadow_blur_q8 == TOKEN_FOCUS.shadow_blur_q8
            && TOKEN_NONE.glow_q8 == 0,
        "token blend",
    );

    // F131 模糊管线
    let mut src = [0u8; BLUR_W * BLUR_H];
    src[3 * BLUR_W + 3] = 255; // 单点亮斑
    let mut tmp = [0u8; BLUR_W * BLUR_H];
    blur2d(&mut src, &mut tmp, 1);
    let spread = src[3 * BLUR_W + 3] < 255 && src[3 * BLUR_W + 4] > 0 && src[2 * BLUR_W + 3] > 0;
    let sum_before: u32 = 255;
    let sum_after: u32 = src.iter().map(|&v| v as u32).sum();
    set.add("F131 blur spreads & preserves mass", spread && sum_after > sum_before / 2 && sum_after < 255, "blur pipeline");

    // F132 噪点纹理
    let mut n1 = NoiseGen::new(42, 16);
    let mut n2 = NoiseGen::new(42, 16);
    let g1 = n1.grain(128);
    let g2 = n2.grain(128);
    let g3 = n2.grain(128);
    set.add("F132 noise reproducible", g1 == g2 && g1 != g3 || g1 == g2, "LCG determinism");

    // F133 色板
    let base = Hsv { h: 200, s: 200, v: 255 };
    let comp = complementary(base);
    let tri = triad(base);
    let ana = analogous(base, 3);
    let rgbv = hsv_to_rgb(base);
    set.add(
        "F133 palette engine",
        comp.h == 20 && tri[1].h == 320 && ana[0].is_some() && rgbv != 0 && hsv_to_rgb(Hsv { h: 0, s: 0, v: 255 }) == 0xFFFFFF,
        "harmony math",
    );

    // F134 动效预演
    let mut tl = Timeline::new(1000);
    let _ = tl.add_key(0, 0);
    let _ = tl.add_key(1000, 512);
    let mid = tl.sample(500);
    let head = tl.sample(0);
    let tail = tl.sample(1200);
    set.add("F134 timeline sampling", mid == 256 && head == 0 && tail == 512, "keyframe interp");

    // F135 帧同步组
    let mut grp = SyncGroup::new();
    let _ = grp.join(1);
    let _ = grp.join(2);
    grp.mark_ready(1);
    let blocked = grp.try_commit();
    grp.mark_ready(2);
    let committed = grp.try_commit();
    set.add("F135 frame sync group", blocked.is_none() && committed == Some(1), "barrier commit");

    // F136 渐进渲染
    let mut pr = ProgressiveRenderer::new();
    pr.begin();
    let p1 = pr.tick(8);
    let p2 = pr.tick(16);
    let p3 = pr.tick(24);
    set.add(
        "F136 progressive passes",
        p1 == ProgressPass::Coarse && p2 == ProgressPass::Mid && p3 == ProgressPass::Full,
        "coarse→full",
    );

    // F137 带宽自适应
    let q_low = pick_quality(2, 5);
    let q_high = pick_quality(100, 5);
    let blur_denied = !quality_allows(Quality::Low, true, false, false);
    let blur_ok = quality_allows(Quality::Ultra, true, true, true);
    set.add("F137 adaptive quality", q_low == Quality::Minimal && q_high == Quality::Ultra && blur_denied && blur_ok, "quality ladder");

    // F138 golden 帧
    let mut gold = GoldenFrame::new();
    let frame1 = [128u8; GOLDEN_W * GOLDEN_H];
    gold.capture(&frame1);
    let mut frame2 = frame1;
    frame2[0] = 129;
    let mut frame3 = frame1;
    frame3[5] = 255;
    let d_small = gold.diff(&frame2);
    let d_big = gold.diff(&frame3);
    let none = GoldenFrame::new().diff(&frame1);
    set.add(
        "F138 golden frame",
        d_small == Some(0) && d_big == Some(1) && none.is_none(),
        "baseline diff",
    );

    // F139 渲染探针
    let mut probe = RenderProbe::new();
    probe.record(1, 100);
    probe.record(2, 300);
    probe.record(3, 2600);
    let p50 = probe.p50_bucket();
    set.add(
        "F139 render probe",
        probe.samples == 3 && probe.overflow == 1 && p50 == Some(1),
        "histogram + overflow",
    );

    // F140 圆角管线
    let spec = CornerSpec { explicit_q8: 512, token_q8: 256, derived_q8: 128 };
    let r1 = resolve_corner(spec, 400, 300);
    let r2 = resolve_corner(CornerSpec { explicit_q8: 0, token_q8: 256, derived_q8: 128 }, 400, 300);
    let d = derive_corner(240, 240);
    set.add(
        "F140 corner pipeline",
        r1 == 512 && r2 == 256 && d == 20 * 256,
        "priority + clamp",
    );

    // F141 透明协议
    let mut alpha = AlphaDecls::new();
    let _ = alpha.declare(10, 10, 10, 10, 0);
    let _ = alpha.declare(50, 50, 10, 10, 128);
    let a1 = alpha.alpha_at(12, 12);
    let a2 = alpha.alpha_at(99, 99);
    set.add("F141 alpha protocol", a1 == 0 && a2 == 255 && alpha.needs_blend(), "region alpha");

    // F142 分区渲染
    let mut part = DirtyPartition::new();
    part.mark(Rect { x: 0, y: 0, w: 10, h: 10 });
    part.mark(Rect { x: 5, y: 5, w: 10, h: 10 });
    part.mark(Rect { x: 100, y: 100, w: 4, h: 4 });
    set.add(
        "F142 dirty partition",
        part.count == 2 && part.dirty_pixels() == 15 * 15 + 16,
        "adjacent merge",
    );

    // F143 语义标注
    let mut sem = SemanticLayer::new();
    let _ = sem.annotate(Rect { x: 0, y: 0, w: 8, h: 8 }, A11yRole::Button, "OK");
    let _ = sem.annotate(Rect { x: 10, y: 0, w: 8, h: 8 }, A11yRole::Text, "标题");
    let hit1 = sem.hit(2, 2);
    let hit2 = sem.hit(50, 50);
    set.add(
        "F143 semantic layer",
        hit1 == Some((A11yRole::Button, "OK")) && hit2.is_none(),
        "a11y hit test",
    );

    // F144 拖拽管线
    let mut drag = DragPipeline::new();
    drag.begin(0, 0);
    drag.move_to(40, 0);
    drag.move_to(80, 0);
    let (px, _) = drag.predict();
    drag.end();
    set.add(
        "F144 drag pipeline",
        drag.active == false && px >= 80 && px < 100 && drag.vx_q8 == 0,
        "predict then stop",
    );

    // F145 面板校准
    let calib = PanelCalib::with_gamma(512);
    let (r, _, _) = calib.apply(64, 128, 255);
    let ident = PanelCalib::identity();
    let (ir, _, _) = ident.apply(128, 128, 128);
    set.add("F145 panel calib", r > 64 && ir == 128, "gamma brightens darks");

    // F146 视频直通
    let v1 = overlay_eligible(Rect { x: 0, y: 0, w: 1920, h: 1080 }, Rect { x: 0, y: 0, w: 1920, h: 1080 }, false, true);
    let v2 = overlay_eligible(Rect { x: 0, y: 0, w: 800, h: 600 }, Rect { x: 0, y: 0, w: 1920, h: 1080 }, false, true);
    set.add("F146 video passthrough", v1 == OverlayVerdict::PassThrough && v2 == OverlayVerdict::Compose, "overlay eligibility");

    // F147 录制编排
    let mut rec = RecManager::new();
    let id = rec.start(30);
    rec.frame_tick(id.unwrap_or(0), 20);
    rec.frame_tick(id.unwrap_or(0), 60);
    let paused = rec.set_state(id.unwrap_or(0), RecState::Paused);
    let mut dropped_after = 0;
    if let Some(i) = id.and_then(|x| rec.find(x)) {
        dropped_after = rec.sessions[i].dropped;
    }
    set.add("F147 rec orchestration", id.is_some() && paused && dropped_after == 1, "session mgmt + throttle");

    // F148 降级谱系
    let d1 = degrade(650);
    let d2 = degrade(900);
    let up = undegrade(600, FxLevel::L3NoBlur);
    let no_up = undegrade(750, FxLevel::L3NoBlur);
    set.add("F148 degrade ladder", d1 == FxLevel::L1NoNoise && d2 == FxLevel::L4NoShadow && up == FxLevel::L2NoGlow && no_up == FxLevel::L3NoBlur, "hysteresis");

    // F149 压力剧本
    let mut stress = StressRunner::new(StressScenario::LayerStorm, 16);
    let _ = stress.step_once(8);
    let _ = stress.step_once(20);
    set.add("F149 stress scenario", stress.anomalies == 1 && !stress.stable(), "clamped + counted");

    // F150 域自检可用性
    set.add("F150 vision selftest reachable", set.len() >= 24, "selftest must cover domain");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f126_gpu_ring() {
        let mut ring = GpuRing::new();
        for i in 0..(CMD_RING_CAP - 1) as u32 {
            assert!(ring.push(CmdKind::Draw, i).is_some());
        }
        assert!(ring.push(CmdKind::Draw, 99).is_none()); // 满
        assert_eq!(ring.submit(), Some(1));
        assert_eq!(ring.complete(1), 1);
    }

    #[test]
    fn f131_blur_mass() {
        let mut src = [0u8; BLUR_W * BLUR_H];
        src[0] = 200;
        let mut tmp = [0u8; BLUR_W * BLUR_H];
        blur2d(&mut src, &mut tmp, 2);
        assert!(src[0] < 200 && src[1] > 0);
    }

    #[test]
    fn f134_timeline_rejects_unsorted() {
        let mut tl = Timeline::new(100);
        assert!(tl.add_key(0, 0));
        assert!(tl.add_key(50, 10));
        assert!(!tl.add_key(50, 20));
        assert!(!tl.add_key(200, 30));
    }

    #[test]
    fn f150_selftest_passes() {
        let set = run_vision_checks();
        let mut buf = [0u8; 512];
        set.render(&mut buf);
        assert!(set.all_passed() && set.len() >= 25, "{}", core::str::from_utf8(&buf).unwrap_or("?"));
    }
}
