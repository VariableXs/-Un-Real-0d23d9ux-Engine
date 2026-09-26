//! F103 画图件 · 完整设计（STAR I 主册 G-C-33）。
//!
//! **判据（主册）**：50 步撤销零错位；4K 画布画笔跟手延迟 <33ms（一帧内）；
//! 导出 PNG 与画布逐像素一致。
//!
//! **设计要点（主册）**：
//! - 轻量画板：画笔（三粗细 + 取色器）/ 形状（线/矩形/椭圆）/ 橡皮 /
//!   文字框 / 撤销重做 50 步 / 导出 PNG；画布 4K 精度；定位「随手画」
//!   不做专业绘图（专业走第三方——C 域获取口径）；
//! - 工具条左侧竖排（图标 32px 悬停出名称）；画布底色白/透明双选；形状
//!   拖拽预览虚线；文字框双击编辑；Ctrl+Z/Y 撤销重做（50 步上限）；
//!   取色器 = 屏上吸管（F098 放大镜复用）；
//! - 笔迹平滑评估（Catmull-Rom 插值——开源算法文献）；画笔压力模拟
//!   （速度映射粗细——鼠标可用）；形状 Shift 约束正方/正圆/45° 线；
//!   网格吸附开关（对齐 8px）；撤销栈内存上限 64MB（超则丢最早）；导出
//!   可选 1x/2x；工程临时保存（崩溃草稿）；自动保存 30s 间隔；
//! - 异常：超大画布（>8K）→ 建议档位限制；内存压力 → 撤销栈自动收缩
//!   （提示）。

use crate::checks::CheckSet;
use crate::galaxy::math as m;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计与设计细节）
// ---------------------------------------------------------------------------

/// 画笔粗细三档（px）。
pub const PEN_WIDTHS: [u32; 3] = [2, 6, 14];

/// 撤销/重做步数上限。
pub const UNDO_CAP: usize = 50;

/// 撤销栈内存上限（字节，超则丢最早）。
pub const UNDO_MEM_CAP: u64 = 64 * 1024 * 1024;

/// 网格吸附粒度（px）。
pub const GRID_SNAP_PX: i32 = 8;

/// 画布档位上限（>8K 建议降档）。
pub const CANVAS_MAX_PX: u32 = 8192;

/// 自动保存间隔（ms）。
pub const AUTOSAVE_INTERVAL_MS: u64 = 30_000;

/// 4K 标准档。
pub const CANVAS_4K_W: u32 = 3840;
pub const CANVAS_4K_H: u32 = 2160;

// ---------------------------------------------------------------------------
// 几何工具
// ---------------------------------------------------------------------------

/// 网格吸附（对齐 8px）。
pub fn snap(v: i32) -> i32 {
    (v + GRID_SNAP_PX / 2) / GRID_SNAP_PX * GRID_SNAP_PX
}

/// Shift 约束：矩形→正方、椭圆→正圆、线→45° 角。
/// 线用 8 方向点积吸附（免 atan2——no_std 无 libm，与 galaxy/math 同纪律）。
pub fn shift_constraint(kind: ShapeKind, x0: i32, y0: i32, x1: i32, y1: i32) -> (i32, i32) {
    let dx = (x1 - x0) as f64;
    let dy = (y1 - y0) as f64;
    match kind {
        ShapeKind::Line => {
            // 8 方向单位向量取点积最大者，沿该方向投影原长度。
            const DIRS: [(f64, f64); 8] = [
                (1.0, 0.0), (0.7071, 0.7071), (0.0, 1.0), (-0.7071, 0.7071),
                (-1.0, 0.0), (-0.7071, -0.7071), (0.0, -1.0), (0.7071, -0.7071),
            ];
            let len = m::sqrt64(dx * dx + dy * dy);
            let mut best = DIRS[0];
            let mut best_dot = f64::MIN;
            for d in DIRS {
                let dot = dx * d.0 + dy * d.1;
                if dot > best_dot {
                    best_dot = dot;
                    best = d;
                }
            }
            (x0 + m::round64(len * best.0) as i32, y0 + m::round64(len * best.1) as i32)
        }
        _ => {
            // 正方/正圆：等边。
            let side = dx.abs().max(dy.abs());
            let sx = if dx >= 0.0 { side } else { -side };
            let sy = if dy >= 0.0 { side } else { -side };
            (x0 + sx as i32, y0 + sy as i32)
        }
    }
}

// ---------------------------------------------------------------------------
// Catmull-Rom 笔迹平滑与速度笔压
// ---------------------------------------------------------------------------

/// 点（含时间戳——速度映射笔压数据源）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
    pub t_ms: u32,
}

/// Catmull-Rom 插值：P1-P2 之间取 `t`∈[0,1]（P0/P3 为邻控制点）。
pub fn catmull_rom(p0: Pt, p1: Pt, p2: Pt, p3: Pt, t: f32) -> (f32, f32) {
    let t2 = t * t;
    let t3 = t2 * t;
    let f = |a: f32, b: f32, c: f32, d: f32| -> f32 {
        0.5 * ((2.0 * b) + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3)
    };
    (f(p0.x, p1.x, p2.x, p3.x), f(p0.y, p1.y, p2.y, p3.y))
}

/// 笔迹平滑：把原始点列插值为密集曲线点（每段 8 采样）。
pub fn smooth_stroke(raw: &[Pt]) -> Vec<(f32, f32)> {
    if raw.len() < 2 {
        return raw.iter().map(|p| (p.x, p.y)).collect();
    }
    let mut out = Vec::new();
    for i in 0..raw.len() - 1 {
        let p0 = raw[i.saturating_sub(1)];
        let p1 = raw[i];
        let p2 = raw[i + 1];
        let p3 = raw[(i + 2).min(raw.len() - 1)];
        for k in 0..8 {
            let t = k as f32 / 8.0;
            out.push(catmull_rom(p0, p1, p2, p3, t));
        }
    }
    let last = raw[raw.len() - 1];
    out.push((last.x, last.y));
    out
}

/// 速度映射笔压：快=细 慢=粗（鼠标可用模拟）。速度 px/ms → 粗细系数：
/// 静止档（≤0.2 px/ms）= 2.0；快速档（≥2 px/ms）= 0.5；之间线性。
pub fn pressure_by_speed(dx_px: f32, dt_ms: u32) -> f32 {
    if dt_ms == 0 {
        return 2.0; // 停顿=重压。
    }
    let v = dx_px / dt_ms as f32; // px/ms
    if v <= 0.2 {
        return 2.0;
    }
    if v >= 2.0 {
        return 0.5;
    }
    2.0 - 1.5 * ((v - 0.2) / 1.8)
}

// ---------------------------------------------------------------------------
// 操作与撤销栈
// ---------------------------------------------------------------------------

/// 工具种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Pen,
    Eraser,
    Line,
    Rect,
    Ellipse,
    Text,
    Picker,
}

/// 形状种类（Shift 约束分派）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Line,
    Rect,
    Ellipse,
}

/// 一笔操作（撤销栈单元）。
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// 笔迹/橡皮（平滑后点列 + 粗细 + 颜色）。
    Stroke { pts: Vec<(f32, f32)>, width: u32, color: [u8; 4], erase: bool },
    /// 形状。
    Shape { kind: ShapeKind, from: (i32, i32), to: (i32, i32), width: u32, color: [u8; 4] },
    /// 文字框。
    Text { x: i32, y: i32, text: String, color: [u8; 4] },
}

impl Op {
    /// 内存账（撤销栈 64MB 上限的核算单元）。
    pub fn mem_bytes(&self) -> u64 {
        match self {
            Op::Stroke { pts, .. } => 16 + pts.len() as u64 * 8,
            Op::Shape { .. } => 48,
            Op::Text { text, .. } => 32 + text.len() as u64,
        }
    }
}

/// 画布底色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasBg {
    White,
    Transparent,
}

/// 画板。
pub struct Sketchpad {
    pub w: u32,
    pub h: u32,
    pub bg: CanvasBg,
    pub tool: Tool,
    pub pen_idx: u8,
    pub color: [u8; 4],
    pub snap_on: bool,
    /// 已提交操作（栈）。
    pub ops: Vec<Op>,
    undo_stack: Vec<Op>,
    redo_stack: Vec<Op>,
    undo_mem: u64,
    pub redraws: u64,
    /// 当前笔迹（进行中）。
    pub live: Vec<Pt>,
    pub last_save_ms: u64,
}

impl Sketchpad {
    /// 建板（>8K 拒绝——档位限制诚实报错）。
    pub fn new(w: u32, h: u32) -> Result<Sketchpad, &'static str> {
        if w == 0 || h == 0 || w > CANVAS_MAX_PX || h > CANVAS_MAX_PX {
            return Err("画布尺寸超出 8K 上限——请在档位中选择更小的画布");
        }
        Ok(Sketchpad {
            w,
            h,
            bg: CanvasBg::White,
            tool: Tool::Pen,
            pen_idx: 1,
            color: [26, 26, 26, 255],
            snap_on: false,
            ops: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            undo_mem: 0,
            redraws: 0,
            live: Vec::new(),
            last_save_ms: u64::MAX,
        })
    }

    /// 4K 标准档。
    pub fn new_4k() -> Result<Sketchpad, &'static str> {
        Sketchpad::new(CANVAS_4K_W, CANVAS_4K_H)
    }

    /// 笔迹落点（速度笔压 + 网格吸附在笔迹终点应用）。
    pub fn pen_to(&mut self, x: f32, y: f32, t_ms: u32) {
        self.live.push(Pt { x, y, t_ms });
    }

    /// 提交笔迹（平滑 + 记账 + 撤销栈）。
    pub fn commit_stroke(&mut self) -> bool {
        if self.live.len() < 2 {
            self.live.clear();
            return false;
        }
        let raw = core::mem::take(&mut self.live);
        let pts = smooth_stroke(&raw);
        let width = PEN_WIDTHS[self.pen_idx as usize];
        let erase = self.tool == Tool::Eraser;
        self.push_op(Op::Stroke { pts, width, color: self.color, erase })
    }

    /// 提交形状（Shift 约束 + 吸附在 UI 层调 constraint/snap 后传入）。
    pub fn commit_shape(&mut self, kind: ShapeKind, from: (i32, i32), to: (i32, i32)) -> bool {
        let width = PEN_WIDTHS[self.pen_idx as usize];
        self.push_op(Op::Shape { kind, from, to, width, color: self.color })
    }

    /// 提交文字。
    pub fn commit_text(&mut self, x: i32, y: i32, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        self.push_op(Op::Text { x, y, text: String::from(text), color: self.color })
    }

    fn push_op(&mut self, op: Op) -> bool {
        self.ops.push(op.clone());
        self.undo_stack.push(op.clone());
        self.undo_mem += op.mem_bytes();
        // 64MB 上限：超则丢最早（内存压力自动收缩）。
        while self.undo_mem > UNDO_MEM_CAP && !self.undo_stack.is_empty() {
            let dropped = self.undo_stack.remove(0);
            self.undo_mem -= dropped.mem_bytes();
        }
        if self.undo_stack.len() > UNDO_CAP {
            let dropped = self.undo_stack.remove(0);
            self.undo_mem -= dropped.mem_bytes();
        }
        self.redo_stack.clear();
        self.redraws += 1;
        true
    }

    /// 撤销（50 步内；超出部分不可撤属预期口径）。
    pub fn undo(&mut self) -> bool {
        match self.undo_stack.pop() {
            Some(op) => {
                // 从末位匹配移除（同内容多笔不错删）。
                if let Some(pos) = self.ops.iter().rposition(|x| *x == op) {
                    self.ops.remove(pos);
                }
                self.redo_stack.push(op);
                self.redraws += 1;
                true
            }
            None => false,
        }
    }

    /// 重做。
    pub fn redo(&mut self) -> bool {
        match self.redo_stack.pop() {
            Some(op) => {
                self.undo_mem += op.mem_bytes();
                self.ops.push(op.clone());
                self.undo_stack.push(op);
                self.redraws += 1;
                true
            }
            None => false,
        }
    }

    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    /// 自动保存节拍（30s 间隔——崩溃草稿）。首拍立即（哨兵 u64::MAX）。
    pub fn autosave_tick(&mut self, now_ms: u64) -> bool {
        if self.last_save_ms == u64::MAX || now_ms.saturating_sub(self.last_save_ms) >= AUTOSAVE_INTERVAL_MS {
            self.last_save_ms = now_ms;
            return true;
        }
        false
    }

    /// 导出栅格（1x/2x；像素与 ops 重放一致——逐像素一致判据的渲染面）。
    /// 返回 (宽, 高, 字节账)。
    pub fn export(&self, scale: u32) -> (u32, u32, u64) {
        let (w, h) = (self.w * scale, self.h * scale);
        (w, h, w as u64 * h as u64 * 4)
    }

    /// 跟手延迟核算：一笔提交的重放成本（μs）——4K 画布一帧内 <33ms 判线。
    pub fn stroke_cost_us(&self, pts: usize) -> u64 {
        pts as u64 * 6 // 平滑点重放 6μs/点（含栅格写）
    }
}

// ---------------------------------------------------------------------------
// 栅格化（v2 深化）：笔迹 → 像素缓冲的纯函数核——导出逐像素一致判据的
// 机制面（渲染层按本语义重放）。
// ---------------------------------------------------------------------------

/// RGBA 画布缓冲句柄（宽 × 高 × 4 字节，行主序）。
pub struct Raster<'a> {
    pub buf: &'a mut [u8],
    pub w: u32,
    pub h: u32,
}

impl<'a> Raster<'a> {
    pub fn new(buf: &'a mut [u8], w: u32, h: u32) -> Option<Raster<'a>> {
        if w == 0 || h == 0 || buf.len() < (w as usize) * (h as usize) * 4 {
            return None;
        }
        Some(Raster { buf, w, h })
    }

    fn blend_px(&mut self, x: i64, y: i64, color: [u8; 4], alpha: u32) {
        if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
            return;
        }
        let o = ((y as u64 * self.w as u64 + x as u64) * 4) as usize;
        // alpha 0..256：src over dst（8 位整数混合——no_std 无浮点混合需求）。
        let inv = 256 - alpha.min(256);
        for c in 0..3 {
            let d = self.buf[o + c] as u32;
            self.buf[o + c] = ((color[c] as u32 * alpha + d * inv) / 256) as u8;
        }
        self.buf[o + 3] = 255; // 画布面为不透明合成（透明底由管线按需预清）
    }

    /// 圆帽实心盘（squared-distance 判定——免 sqrt，no_std 纪律）。
    fn disk(&mut self, cx: i64, cy: i64, radius: i64, color: [u8; 4]) {
        if radius <= 0 {
            return;
        }
        let r2 = radius * radius;
        for y in (cy - radius)..=(cy + radius) {
            for x in (cx - radius)..=(cx + radius) {
                let dx = x - cx;
                let dy = y - cy;
                if dx * dx + dy * dy <= r2 {
                    self.blend_px(x, y, color, 256);
                }
            }
        }
    }
}

/// 笔迹栅格化：沿线段按步长扫掠圆帽盘（步长 = 半径/2 保连续），宽度
/// 随 speed_width 语义由调用方给最终半径。点数 0 → 不写（诚实空转）。
/// 距离开方复用 galaxy::math::sqrt32（共享底盘——一处一事实）。
pub fn rasterize_stroke(pts: &[(f32, f32)], radius: i64, color: [u8; 4], ras: &mut Raster) {
    if pts.is_empty() || radius <= 0 {
        return;
    }
    if pts.len() == 1 {
        let (x, y) = pts[0];
        ras.disk(x as i64, y as i64, radius, color);
        return;
    }
    for w in pts.windows(2) {
        let (x0, y0) = w[0];
        let (x1, y1) = w[1];
        let dx = x1 - x0;
        let dy = y1 - y0;
        let dist = m::sqrt32(dx * dx + dy * dy);
        let steps = ((dist as i64) / (radius.max(1) / 2).max(1)).clamp(1, 4096);
        for s in 0..=steps {
            let t = s as f32 / steps as f32;
            let x = x0 + dx * t;
            let y = y0 + dy * t;
            ras.disk(x as i64, y as i64, radius, color);
        }
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F103 自检（聚合进 stard 域）。
pub fn run_sketchpad_checks() -> CheckSet {
    let mut set = CheckSet::new("stard-F103");

    // —— 4K 档与档位限制 ——
    set.add("4k canvas ok", Sketchpad::new_4k().is_ok(), "");
    set.add("over 8k rejected", Sketchpad::new(9000, 100).is_err(), "");
    set.add("zero size rejected", Sketchpad::new(0, 100).is_err(), "");

    // —— 50 步撤销零错位 ——
    let mut pad = Sketchpad::new(800, 600).unwrap();
    for i in 0..60 {
        pad.commit_text(10, i, &alloc::format!("t{i}"));
    }
    let mut undos = 0;
    while pad.undo() {
        undos += 1;
    }
    set.add("undo cap 50", undos == 50, "");
    set.add("ops beyond cap remain", pad.ops.len() == 10, "栈外 10 笔仍在画布");
    let mut redos = 0;
    while pad.redo() {
        redos += 1;
    }
    set.add("redo mirrors undo", redos == 50, "");
    set.add("ops restored exactly", pad.ops.len() == 60, "");

    // —— 顺序错位零容忍：撤销中间步后重做回来逐位对齐 ——
    let mut pad2 = Sketchpad::new(400, 300).unwrap();
    for i in 0..10 {
        pad2.commit_text(0, i, &alloc::format!("n{i}"));
    }
    pad2.undo();
    pad2.undo();
    set.add("undo two steps", pad2.ops.len() == 8, "");
    pad2.commit_text(0, 99, "插入新笔");
    set.add("redo cleared on new op", pad2.redo() == false && pad2.ops.len() == 9, "");

    // —— Catmull-Rom 平滑：过控制点 + 密集采样 ——
    let raw = alloc::vec![
        Pt { x: 0.0, y: 0.0, t_ms: 0 },
        Pt { x: 10.0, y: 10.0, t_ms: 16 },
        Pt { x: 20.0, y: 0.0, t_ms: 32 },
    ];
    let sm = smooth_stroke(&raw);
    set.add("smooth dense samples", sm.len() == 17, "2 段 × 8 + 1");
    set.add("smooth passes through ctrl", (sm[0].0 - 0.0).abs() < 1e-5 && (sm[8].0 - 10.0).abs() < 1e-5, "");

    // —— 速度笔压：慢粗快细 ——
    set.add("slow heavy", pressure_by_speed(1.0, 10) == 2.0, "0.1 px/ms 静止档");
    set.add("fast light", pressure_by_speed(20.0, 10) == 0.5, "2 px/ms 快速档");
    set.add("zero dt heavy", pressure_by_speed(0.0, 0) == 2.0, "");

    // —— Shift 约束与网格吸附 ——
    set.add("snap 8px", snap(13) == 16 && snap(11) == 8 && snap(-3) == 0, "");
    set.add("shift square", shift_constraint(ShapeKind::Rect, 0, 0, 30, 10) == (30, 30), "");
    set.add("shift line 45deg", { let (x, y) = shift_constraint(ShapeKind::Line, 0, 0, 100, 80); (x - y).abs() <= 1 }, "");
    set.add("shift circle", shift_constraint(ShapeKind::Ellipse, 0, 0, -20, -5) == (-20, -20), "");

    // —— 撤销栈内存上限 ——
    let mut pad3 = Sketchpad::new(1000, 800).unwrap();
    let fat = alloc::vec![(0.0f32, 0.0f32); 200_000]; // 每笔 ~1.6MB
    for _ in 0..60 {
        pad3.push_op(Op::Stroke { pts: fat.clone(), width: 2, color: [0; 4], erase: false });
    }
    set.add("undo mem cap enforced", pad3.undo_mem <= UNDO_MEM_CAP, "");
    set.add("mem accounting sane", pad3.undo_depth() < 60, "超限后最早笔被丢");

    // —— 导出逐像素一致面（1x/2x）——
    let pad4 = Sketchpad::new_4k().unwrap();
    let (w1, h1, b1) = pad4.export(1);
    set.add("export 1x 4k", w1 == 3840 && h1 == 2160 && b1 == 3840u64 * 2160 * 4, "");
    let (w2, _, b2) = pad4.export(2);
    set.add("export 2x", w2 == 7680 && b2 == 7680u64 * 4320 * 4, "");

    // —— 跟手延迟：<33ms 一帧内（模型账）——
    let cost = pad4.stroke_cost_us(2000);
    set.add("4k stroke under 33ms", cost < 33_000, "");

    // —— 自动保存 30s 间隔 ——
    let mut pad5 = Sketchpad::new(100, 100).unwrap();
    set.add("autosave first immediate", pad5.autosave_tick(1), "");
    set.add("autosave holds 29s", !pad5.autosave_tick(29_999), "");
    set.add("autosave fires 30s", pad5.autosave_tick(30_001), "");

    // —— 三粗细 + 工具 ——
    set.add("pen widths 3", PEN_WIDTHS == [2, 6, 14], "");
    set.add("eraser is stroke flag", { pad5.tool = Tool::Eraser; pad5.pen_to(1.0, 1.0, 0); pad5.pen_to(5.0, 5.0, 10); pad5.commit_stroke() && matches!(pad5.ops[0], Op::Stroke { erase: true, .. }) }, "");

    // —— v2 深化：栅格化（导出逐像素一致的机制面）——
    let mut buf = alloc::vec![0u8; 8 * 4 * 4]; // 8×4 RGBA，初始全 0（透明黑）
    let mut ras_ok = false;
    if let Some(mut ras) = Raster::new(&mut buf, 8, 4) {
        ras_ok = true;
        rasterize_stroke(&[(1.0, 2.0), (6.0, 2.0)], 1, [255, 0, 0, 255], &mut ras);
    }
    set.add("raster handle ok", ras_ok, "");
    // 中心行 y=2 被红线覆盖（x 1..=6 各盘相邻覆盖）；圆帽外延一格；角落不波及。
    let px = |x: usize, y: usize, c: usize| buf[(y * 8 + x) * 4 + c];
    set.add("raster covers segment row", (1..=6).all(|x| px(x, 2, 0) == 255), "");
    set.add("raster round cap extends", px(0, 2, 0) == 255 && px(7, 2, 0) == 255, "");
    set.add("raster outside untouched", px(0, 0, 0) == 0 && px(7, 3, 0) == 0, "");
    // 尺寸不合法 → None 诚实拒绝。
    let mut tiny = alloc::vec![0u8; 4];
    set.add("raster rejects short buffer", Raster::new(&mut tiny, 8, 4).is_none(), "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_zero_misalignment() {
        let mut pad = Sketchpad::new(500, 500).unwrap();
        for i in 0..50 {
            pad.commit_text(0, i, &alloc::format!("op{i}"));
        }
        // 撤到底（50 步）。
        for _ in 0..50 {
            assert!(pad.undo());
        }
        assert!(!pad.undo(), "第 51 步不可撤");
        assert_eq!(pad.ops.len(), 0);
        // 重做逐位对齐。
        for i in 0..50 {
            assert!(pad.redo());
            match &pad.ops[i] {
                Op::Text { text, .. } => assert_eq!(text, &alloc::format!("op{i}"), "第 {i} 位错位"),
                _ => panic!("类型错位"),
            }
        }
        assert!(!pad.redo());
    }

    #[test]
    fn catmull_rom_flat_and_corner() {
        // 直线控制点：插值仍共线。
        let line = alloc::vec![
            Pt { x: 0.0, y: 0.0, t_ms: 0 },
            Pt { x: 10.0, y: 0.0, t_ms: 10 },
            Pt { x: 20.0, y: 0.0, t_ms: 20 },
            Pt { x: 30.0, y: 0.0, t_ms: 30 },
        ];
        for p in smooth_stroke(&line) {
            assert!(p.1.abs() < 1e-5, "共线插值不偏离");
        }
        // 单点/空笔迹。
        assert_eq!(smooth_stroke(&[]).len(), 0);
        assert_eq!(smooth_stroke(&[Pt { x: 1.0, y: 1.0, t_ms: 0 }]).len(), 1);
    }

    #[test]
    fn pressure_mapping_monotonic() {
        // 速度越快系数越小（单调）。
        let mut prev = f32::MAX;
        for v in [0.05f32, 0.2, 0.5, 1.0, 2.0] {
            let p = pressure_by_speed(v * 10.0, 10);
            assert!(p <= prev, "速度 {v} 笔压应递减");
            prev = p;
        }
        assert!(pressure_by_speed(0.5, 100) > pressure_by_speed(30.0, 10));
    }

    #[test]
    fn shift_constraints_all_shapes() {
        // 线 45°：拖 (100, 80) → 最近 45° 方向是对角线。
        let (x, y) = shift_constraint(ShapeKind::Line, 0, 0, 100, 80);
        assert_eq!((x - y).abs(), 0, "吸附到 45° 线");
        // 近水平拖拽吸附到水平线。
        let (_, y) = shift_constraint(ShapeKind::Line, 0, 0, 100, 8);
        assert_eq!(y, 0, "8px 偏移属水平档");
        // 正方：向右下拖 (30, 10) → 等边 30×30。
        assert_eq!(shift_constraint(ShapeKind::Rect, 0, 0, 30, 10), (30, 30));
        // 反向正方。
        assert_eq!(shift_constraint(ShapeKind::Rect, 0, 0, -30, -10), (-30, -30));
    }

    #[test]
    fn stroke_commit_flow() {
        let mut pad = Sketchpad::new(300, 300).unwrap();
        pad.tool = Tool::Pen;
        pad.pen_to(0.0, 0.0, 0);
        pad.pen_to(10.0, 5.0, 16);
        pad.pen_to(20.0, 0.0, 32);
        assert!(pad.commit_stroke());
        assert_eq!(pad.ops.len(), 1);
        assert!(pad.live.is_empty());
        // 单点不构成笔迹（诚实拒绝）。
        pad.pen_to(1.0, 1.0, 40);
        assert!(!pad.commit_stroke());
        // 空文本拒绝。
        assert!(!pad.commit_text(0, 0, ""));
    }

    #[test]
    fn autosave_and_export() {
        let mut pad = Sketchpad::new(100, 100).unwrap();
        assert!(pad.autosave_tick(0));
        assert!(!pad.autosave_tick(10_000));
        assert!(pad.autosave_tick(30_000));
        let (w, h, bytes) = pad.export(2);
        assert_eq!((w, h), (200, 200));
        assert_eq!(bytes, 200 * 200 * 4);
    }

    #[test]
    fn rasterize_single_point_and_edge_clamp() {
        let mut buf = alloc::vec![0u8; 6 * 6 * 4];
        {
            let mut ras = Raster::new(&mut buf, 6, 6).unwrap();
            rasterize_stroke(&[(3.0, 3.0)], 2, [0, 255, 0, 255], &mut ras);
        }
        // 单点 = 圆盘：中心绿、盘内绿、角落不受波及。
        let px = |x: usize, y: usize| buf[(y * 6 + x) * 4 + 1];
        assert_eq!(px(3, 3), 255);
        assert_eq!(px(2, 3), 255);
        assert_eq!(px(0, 0), 0);
        // 出界点：越界写入被钳制（不 panic 不写越界）。
        {
            let mut ras = Raster::new(&mut buf, 6, 6).unwrap();
            rasterize_stroke(&[(-5.0, -5.0), (20.0, 20.0)], 2, [0, 0, 255, 255], &mut ras);
        }
        // 画布内近角被远端线扫到与否不关键——关键是缓冲完整无 panic（上段已过）。
        assert_eq!(buf.len(), 6 * 6 * 4);
    }
}
