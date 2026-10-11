//! VE-F6006 · 物理调试支持（vez18_dbg）
//!
//! 物理调试三件——**物理体线框视图、约束可视化、步进单帧调试**（物理
//! 为什么这样动必须可查——三律可预期的执行层，X03 口径延伸）。调试
//! 三件含**时间旅行**——物理回放支持逐帧步进回看；三件含**性能预算
//! 声明**；预算声明含**超标动作**；动作含**降级预告**——预告是「下一
//! 步会降」不是「这一步已降」，卡顿前就给出预期不是事后补票。
//!
//! 调试三件：
//! 1. **线框视图**——物理体 → 线框段列表（圆 12 段/AABB 4 段闭式，
//!    降采样档位减段不减体——看的体一个不少，看的花纹变简），
//!    O(物理体数)；
//! 2. **约束可视化**——约束对（两体+双锚）→ 可视线段+锚点标记；
//! 3. **步进单帧**——step_once 推一帧记一迹；**时间旅行**——快照环
//!    逐帧回看（回看只读历史不篡改，越界钳制不 panic）。
//!
//! 错误路径与降级矩阵：**调试数据不出机**（导出闸默认关+脱敏白名单
//! ——只出统计不出坐标，vez10/vef62 同款纪律）；**视图卡顿→降采样**
//! （Full→Half→Quarter 三档）；**越权开启→拒绝**（Off 级什么都看不
//! 到、Viewer 只读可看不可步进、Editor 全权——权限递增链）。
//!
//! 性能：视图 O(物理体数)。无障碍：视图读屏可达（`status_spoken`）。
//! 跨批对接点：X03 调试套件。
//!
//! Z 域字符串码五码互异：`E_BD_NOAUTH` / `E_BD_BUDGET` /
//! `E_BD_EMPTY` / `E_BD_LEAK` / `E_BD_NOPHYS`。

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 诊断码与原因文案（五码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 越权开启调试（权限闸拒绝）。
pub const E_BD_NOAUTH: &str = "E_BD_NOAUTH";
/// 视图生成超预算（降级预告记录——不是错误是预告）。
pub const E_BD_BUDGET: &str = "E_BD_BUDGET";
/// 空视图请求（无物理体可画——显性空态不是静默黑屏）。
pub const E_BD_EMPTY: &str = "E_BD_EMPTY";
/// 调试数据出机拦截（导出闸默认关/脱敏后仍含坐标面）。
pub const E_BD_LEAK: &str = "E_BD_LEAK";
/// 查无此物理体（回看/视图实体 ID 未登记）。
pub const E_BD_NOPHYS: &str = "E_BD_NOPHYS";

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_BD_NOAUTH => "越权：当前权限级无权开启该调试面",
        E_BD_BUDGET => "超预算：视图生成耗时超声明预算，已预告降级",
        E_BD_EMPTY => "空视图：无物理体可画，等待场景装载",
        E_BD_LEAK => "出机拦截：调试数据含坐标面，导出闸保持默认关",
        E_BD_NOPHYS => "查无此体：实体未登记物理世界",
        _ => "未知诊断码",
    }
}

// ---------------------------------------------------------------------------
// 物理体与约束（调试的数据面）
// ---------------------------------------------------------------------------

/// 物理体形状（闭集两类——调试视图按形状出线框，形状闭集判据才闭合）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PhysShape {
    /// 圆（cx, cy, r）。
    Circle { cx: i32, cy: i32, r: i32 },
    /// 轴对齐盒（x0, y0, x1, y1）。
    Aabb { x0: i32, y0: i32, x1: i32, y1: i32 },
}

/// 物理体（调试登记最小面——体 ID/形状/速度，位姿由世界步进演进）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysBody {
    pub id: u32,
    pub shape: PhysShape,
    pub vx: i32,
    pub vy: i32,
}

/// 约束对（两体+双锚点局部坐标）——约束可视化的数据源。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysConstraint {
    pub id: u32,
    pub body_a: u32,
    pub body_b: u32,
    /// A 体局部锚点（相对体参考点）。
    pub anchor_a: (i32, i32),
    /// B 体局部锚点。
    pub anchor_b: (i32, i32),
}

/// 线框段（可视化输出原子——起终点）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seg {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

/// 降采样档位（三档闭集——Full 全细节/Half 减半/Quarter 粗框）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    Full,
    Half,
    Quarter,
}

impl Detail {
    /// 圆的线框段数（闭式：Full 12 / Half 6 / Quarter 4）。
    pub fn circle_segs(self) -> usize {
        match self {
            Detail::Full => 12,
            Detail::Half => 6,
            Detail::Quarter => 4,
        }
    }

    /// 盒的线框段数（闭式：Full 4 边 / Half 2 对角线 / Quarter 1 对角线）。
    pub fn aabb_segs(self) -> usize {
        match self {
            Detail::Full => 4,
            Detail::Half => 2,
            Detail::Quarter => 1,
        }
    }

    /// 降一档（Quarter 到底不动——降级有底，不会降没了）。
    pub fn degrade(self) -> Detail {
        match self {
            Detail::Full => Detail::Half,
            Detail::Half => Detail::Quarter,
            Detail::Quarter => Detail::Quarter,
        }
    }
}

/// 圆的线框段闭式构造（n 边形逼近：圆心+半径 → n 段弦）。
pub fn circle_wire(b: &PhysBody, n: usize) -> Vec<Seg> {
    let (cx, cy, r) = match b.shape {
        PhysShape::Circle { cx, cy, r } => (cx, cy, r),
        _ => return Vec::new(),
    };
    let mut segs = Vec::new();
    let mut prev = (cx + r, cy);
    for i in 1..=n {
        // 角度 i*30°（Full 档 12 段，每段 30°）——整数三角：72 变体用
        // 查表式近似（示例域以 (0°,60°,90°...) 常见角闭式覆盖，
        // 通用角用 cos45≈r*7/10 的千分比近似——误差在调试视图可容忍）。
        let deg = (i as i64 * 360 / n.max(1) as i64) as i32;
        let (dx, dy) = approx_dir(deg, r);
        let cur = (cx + dx, cy + dy);
        segs.push(Seg { x0: prev.0, y0: prev.1, x1: cur.0, y1: cur.1 });
        prev = cur;
    }
    segs
}

/// 方向近似（整数域）：0°/90°/180°/270° 精确，45° 用 7/10 千分比近似。
/// 调试视图是肉眼参照不是物理输入——近似可见性优先于浮点依赖（no_std
/// 零 float 纪律下整数近似是唯一通路）。
pub fn approx_dir(deg: i32, r: i32) -> (i32, i32) {
    let d = ((deg % 360) + 360) % 360;
    match d {
        0 => (r, 0),
        90 => (0, r),
        180 => (-r, 0),
        270 => (0, -r),
        30 => (r * 87 / 100, r / 2),
        60 => (r / 2, r * 87 / 100),
        120 => (-r / 2, r * 87 / 100),
        150 => (-r * 87 / 100, r / 2),
        210 => (-r * 87 / 100, -r / 2),
        240 => (-r / 2, -r * 87 / 100),
        300 => (r / 2, -r * 87 / 100),
        330 => (r * 87 / 100, -r / 2),
        45 => (r * 7 / 10, r * 7 / 10),
        135 => (-r * 7 / 10, r * 7 / 10),
        225 => (-r * 7 / 10, -r * 7 / 10),
        315 => (r * 7 / 10, -r * 7 / 10),
        _ => (r, 0), // 兜底：非整 15° 倍数按 0° 处理（调试视图可容忍）
    }
}

/// 盒的线框段构造（按档位出 4 边/2 对角/1 对角）。
pub fn aabb_wire(b: &PhysBody, detail: Detail) -> Vec<Seg> {
    let (x0, y0, x1, y1) = match b.shape {
        PhysShape::Aabb { x0, y0, x1, y1 } => (x0, y0, x1, y1),
        _ => return Vec::new(),
    };
    let mut segs = Vec::new();
    match detail {
        Detail::Full => {
            segs.push(Seg { x0, y0, x1: x1, y1: y0 });
            segs.push(Seg { x0: x1, y0, x1: x1, y1: y1 });
            segs.push(Seg { x0: x1, y0: y1, x1: x0, y1: y1 });
            segs.push(Seg { x0, y0: y1, x1: x0, y1: y0 });
        }
        Detail::Half => {
            segs.push(Seg { x0, y0, x1: x1, y1: y1 });
            segs.push(Seg { x0: x1, y0, x1: x0, y1: y1 });
        }
        Detail::Quarter => {
            segs.push(Seg { x0, y0, x1: x1, y1: y1 });
        }
    }
    segs
}

// ---------------------------------------------------------------------------
// 权限与隐私闸（越权拒绝 + 调试数据不出机）
// ---------------------------------------------------------------------------

/// 调试权限三级（递增链：Off < Viewer < Editor——vek13/CIPC 同款纪律）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DebugAuth {
    /// 无权：调试面全关。
    Off,
    /// 只读：可看视图/约束/回看，不可步进（步进改变世界状态）。
    Viewer,
    /// 全权：可步进可导出申请。
    Editor,
}

/// 导出闸默认关显式开三步状态机（vez10/vef62 同款纪律——Closed→Armed→Open，
/// 跳步即告警锁定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportGate {
    /// 默认：调试数据不出机。
    Closed,
    /// 已武装（Editor 显式申请，尚未放行）。
    Armed,
    /// 放行（仅脱敏统计面可出机）。
    Open,
}

/// 脱敏导出面（只出统计不出坐标——体数/段数/约束数/档位）。
#[derive(Clone, Debug, PartialEq)]
pub struct ExportDigest {
    pub body_count: usize,
    pub seg_count: usize,
    pub constraint_count: usize,
    pub detail: Detail,
}

// ---------------------------------------------------------------------------
// 快照与时间旅行
// ---------------------------------------------------------------------------

/// 一帧物理世界快照（体位姿集合+帧号）。
#[derive(Clone, Debug, PartialEq)]
pub struct WorldSnap {
    pub frame: u64,
    pub bodies: Vec<(u32, i32, i32)>, // (id, x, y)
}

/// 时间旅行快照环（有界 FIFO——容量 8，满载丢最旧；回看只读历史）。
pub struct TravelRing {
    snaps: Vec<WorldSnap>,
    pub dropped: u32,
}

pub const TRAVEL_CAP: usize = 8;

impl TravelRing {
    pub fn new() -> Self {
        TravelRing { snaps: Vec::new(), dropped: 0 }
    }

    /// 压入快照（满载丢最旧，丢账不静默）。
    pub fn push(&mut self, s: WorldSnap) {
        if self.snaps.len() >= TRAVEL_CAP {
            self.snaps.remove(0);
            self.dropped += 1;
        }
        self.snaps.push(s);
    }

    /// 回看 n 帧（0=最新；越界钳制到最旧——不 panic 不循环）。
    pub fn travel_back(&self, steps: usize) -> Option<&WorldSnap> {
        if self.snaps.is_empty() {
            return None;
        }
        let idx = self.snaps.len() - 1 - steps.min(self.snaps.len() - 1);
        self.snaps.get(idx)
    }

    pub fn len(&self) -> usize {
        self.snaps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.snaps.is_empty()
    }
}

impl Default for TravelRing {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 调试门面（权限闸 + 三件编排 + 预算声明 + 降级预告 + 状态可读）
// ---------------------------------------------------------------------------

/// 视图生成预算声明（µs）——超标动作：降级预告。
pub const VIEW_BUDGET_US: u64 = 100;
/// 步进轨迹环容量。
pub const TRACE_CAP: usize = 16;

/// 步进轨迹行（一帧步进记录：帧号+步进耗时+体数）。
#[derive(Clone, Debug, PartialEq)]
pub struct TraceRow {
    pub frame: u64,
    pub cost_us: u64,
    pub body_count: usize,
}

/// 物理调试门面。
pub struct PhysDebugger {
    pub auth: DebugAuth,
    pub export: ExportGate,
    bodies: Vec<PhysBody>,
    constraints: Vec<PhysConstraint>,
    /// 步进轨迹环（时间旅行数据源）。
    pub travel: TravelRing,
    /// 当前档位（降级预告推进）。
    pub detail: Detail,
    /// 超标计数（每次视图生成超预算 +1）。
    pub budget_overruns: u32,
    /// 降级预告记录（预告了但未生效——生效在下次视图生成时档位切换）。
    pub pending_degrade: bool,
    /// 步进轨迹（步进单帧调试的回放面）。
    pub traces: Vec<TraceRow>,
    /// 模拟时钟（step_once 推进）。
    pub now_frame: u64,
}

impl PhysDebugger {
    /// 构造（权限在构造时声明——运行时不可自提权）。
    pub fn new(auth: DebugAuth) -> Self {
        PhysDebugger {
            auth,
            export: ExportGate::Closed,
            bodies: Vec::new(),
            constraints: Vec::new(),
            travel: TravelRing::new(),
            detail: Detail::Full,
            budget_overruns: 0,
            pending_degrade: false,
            traces: Vec::new(),
            now_frame: 0,
        }
    }

    /// 物理体登记（O(登记数) 查重幂等）。
    pub fn register_body(&mut self, b: PhysBody) {
        if !self.bodies.iter().any(|x| x.id == b.id) {
            self.bodies.push(b);
        }
    }

    pub fn register_constraint(&mut self, c: PhysConstraint) -> Result<(), &'static str> {
        // 两端体必须已登记（约束挂空体是调试视图的悬空线——拒绝）。
        if !self.bodies.iter().any(|x| x.id == c.body_a)
            || !self.bodies.iter().any(|x| x.id == c.body_b)
        {
            return Err(E_BD_NOPHYS);
        }
        if !self.constraints.iter().any(|x| x.id == c.id) {
            self.constraints.push(c);
        }
        Ok(())
    }

    fn body_of(&self, id: u32) -> Option<&PhysBody> {
        self.bodies.iter().find(|b| b.id == id)
    }

    /// 步进单帧（Editor 专属——步进改变世界状态；Viewer 拒）。位移 =
    /// 速度×1 帧（示例域单位速度）。步进后压快照环+记轨迹行。
    pub fn step_once(&mut self, cost_us: u64) -> Result<u64, &'static str> {
        if self.auth < DebugAuth::Editor {
            return Err(E_BD_NOAUTH);
        }
        self.now_frame += 1;
        for b in self.bodies.iter_mut() {
            match &mut b.shape {
                PhysShape::Circle { cx, .. } => *cx += b.vx,
                PhysShape::Aabb { x0, x1, .. } => {
                    *x0 += b.vx;
                    *x1 += b.vx;
                }
            }
        }
        let snap = WorldSnap {
            frame: self.now_frame,
            bodies: self
                .bodies
                .iter()
                .map(|b| match b.shape {
                    PhysShape::Circle { cx, cy, .. } => (b.id, cx, cy),
                    PhysShape::Aabb { x0, y0, .. } => (b.id, x0, y0),
                })
                .collect(),
        };
        self.travel.push(snap);
        if self.traces.len() >= TRACE_CAP {
            self.traces.remove(0);
        }
        self.traces.push(TraceRow { frame: self.now_frame, cost_us, body_count: self.bodies.len() });
        Ok(self.now_frame)
    }

    /// 线框视图生成（Viewer+；O(物理体数)）。空世界显性空态
    /// E_BD_EMPTY；预算超标 → budget_overruns+1 + 降级预告置位
    /// （预告是「下次降」，本张视图照出——卡顿前给预期）。
    pub fn wireframe_view(&mut self, cost_us: u64) -> Result<Vec<Seg>, &'static str> {
        if self.auth < DebugAuth::Viewer {
            return Err(E_BD_NOAUTH);
        }
        if self.bodies.is_empty() {
            return Err(E_BD_EMPTY);
        }
        let mut segs = Vec::new();
        for b in self.bodies.iter() {
            match b.shape {
                PhysShape::Circle { .. } => {
                    let extra = circle_wire(b, self.detail.circle_segs());
                    segs.extend_from_slice(&extra);
                }
                PhysShape::Aabb { .. } => {
                    let extra = aabb_wire(b, self.detail);
                    segs.extend_from_slice(&extra);
                }
            }
        }
        if cost_us > VIEW_BUDGET_US {
            self.budget_overruns += 1;
            if !self.pending_degrade && self.detail != Detail::Quarter {
                self.pending_degrade = true;
            }
        }
        Ok(segs)
    }

    /// 降级预告落档（超标动作的第二步——预告后由引擎在下一帧前调
    /// 用；Quarter 到底不再降）。返回是否真的降了档。
    pub fn apply_pending_degrade(&mut self) -> bool {
        if !self.pending_degrade {
            return false;
        }
        self.pending_degrade = false;
        let before = self.detail;
        self.detail = self.detail.degrade();
        before != self.detail
    }

    /// 约束可视化（Viewer+；锚点=体参考点+局部锚）。
    pub fn constraint_view(&self) -> Result<Vec<Seg>, &'static str> {
        if self.auth < DebugAuth::Viewer {
            return Err(E_BD_NOAUTH);
        }
        let mut segs = Vec::new();
        for c in self.constraints.iter() {
            let (ax, ay) = self
                .body_of(c.body_a)
                .map(|b| match b.shape {
                    PhysShape::Circle { cx, cy, .. } => (cx + c.anchor_a.0, cy + c.anchor_a.1),
                    PhysShape::Aabb { x0, y0, .. } => (x0 + c.anchor_a.0, y0 + c.anchor_a.1),
                })
                .ok_or(E_BD_NOPHYS)?;
            let (bx, by) = self
                .body_of(c.body_b)
                .map(|b| match b.shape {
                    PhysShape::Circle { cx, cy, .. } => (cx + c.anchor_b.0, cy + c.anchor_b.1),
                    PhysShape::Aabb { x0, y0, .. } => (x0 + c.anchor_b.0, y0 + c.anchor_b.1),
                })
                .ok_or(E_BD_NOPHYS)?;
            segs.push(Seg { x0: ax, y0: ay, x1: bx, y1: by });
        }
        Ok(segs)
    }

    /// 导出闸推进（Editor 专属；Closed→Armed→Open 跳步拒）。
    pub fn export_advance(&mut self) -> Result<ExportGate, &'static str> {
        if self.auth < DebugAuth::Editor {
            return Err(E_BD_NOAUTH);
        }
        self.export = match self.export {
            ExportGate::Closed => ExportGate::Armed,
            ExportGate::Armed => ExportGate::Open,
            ExportGate::Open => ExportGate::Open,
        };
        Ok(self.export)
    }

    /// 脱敏导出（仅 Open 态；只出统计不出坐标——坐标面留在机内）。
    pub fn export_digest(&self) -> Result<ExportDigest, &'static str> {
        if self.export != ExportGate::Open {
            return Err(E_BD_LEAK);
        }
        let seg_count = self
            .bodies
            .iter()
            .map(|b| match b.shape {
                PhysShape::Circle { .. } => self.detail.circle_segs(),
                PhysShape::Aabb { .. } => self.detail.aabb_segs(),
            })
            .sum();
        Ok(ExportDigest {
            body_count: self.bodies.len(),
            seg_count,
            constraint_count: self.constraints.len(),
            detail: self.detail,
        })
    }

    /// 状态可读名单（无障碍：视图读屏可达）。
    pub fn status_spoken(&self) -> String {
        format!(
            "物理调试：权限 {:?}，体 {} 个，约束 {} 条，档位 {:?}，超标 {} 次，回看 {} 帧",
            self.auth,
            self.bodies.len(),
            self.constraints.len(),
            self.detail,
            self.budget_overruns,
            self.travel.len()
        )
    }
}

// ---------------------------------------------------------------------------
// 判据（锚点逐条：线框视图/约束可视/单帧步进/时间旅行/预算降级/权限/隐私/编排）
// ---------------------------------------------------------------------------

/// F6006 判据集（9 条，全部数值手算复核、零 panic 面）。
pub fn run_vez18_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F6006");

    // --- 线框视图（判据一）---
    // 视-01：段数闭式——圆 Full 12 段（12 角逐段）、AABB Full 4 段；
    // 降采样 Half 圆 6 段盒 2 对角；Quarter 圆 4 段盒 1 对角。段数
    // 随档位单调不减体数无关——看的体一个不少。
    let mut d1 = PhysDebugger::new(DebugAuth::Editor);
    d1.register_body(PhysBody { id: 1, shape: PhysShape::Circle { cx: 0, cy: 0, r: 10 }, vx: 1, vy: 0 });
    d1.register_body(PhysBody { id: 2, shape: PhysShape::Aabb { x0: 0, y0: 0, x1: 8, y1: 4 }, vx: 0, vy: 1 });
    let v1 = d1.wireframe_view(50).expect("probe-1");
    let segs_full = v1.len();
    let _ = d1.apply_pending_degrade(); // 无预告——False
    let wire_ok = segs_full == 16 // 12+4
        && d1.detail.circle_segs() == 12
        && Detail::Half.circle_segs() == 6
        && Detail::Quarter.circle_segs() == 4
        && Detail::Full.aabb_segs() == 4
        && Detail::Half.aabb_segs() == 2
        && Detail::Quarter.aabb_segs() == 1
        && !d1.apply_pending_degrade(); // 无预告时落档返回 False
    if wire_ok {
        s.ok("F6006-视-01-线框段数闭式与档位");
    } else {
        s.fail("F6006-视-01-线框段数闭式与档位", "线框视图失效");
    }

    // --- 约束可视（判据二）---
    // 视-02：约束对→可视线锚点精确——圆(0,0)+锚(2,0)→盒(0,0)参考+锚(4,1)
    // →线 (2,0)-(4,1)；挂空体拒 E_BD_NOPHYS。
    let mut d2 = PhysDebugger::new(DebugAuth::Viewer);
    d2.register_body(PhysBody { id: 1, shape: PhysShape::Circle { cx: 0, cy: 0, r: 5 }, vx: 0, vy: 0 });
    let hang = d2.register_constraint(PhysConstraint { id: 1, body_a: 1, body_b: 9, anchor_a: (2, 0), anchor_b: (4, 1) });
    d2.register_body(PhysBody { id: 2, shape: PhysShape::Aabb { x0: 0, y0: 0, x1: 8, y1: 4 }, vx: 0, vy: 0 });
    let ok_c = d2.register_constraint(PhysConstraint { id: 1, body_a: 1, body_b: 2, anchor_a: (2, 0), anchor_b: (4, 1) });
    let cv = d2.constraint_view().expect("probe-2");
    let cons_ok = matches!(hang, Err(E_BD_NOPHYS))
        && ok_c.is_ok()
        && cv.len() == 1
        && cv[0].x0 == 2
        && cv[0].y0 == 0
        && cv[0].x1 == 4
        && cv[0].y1 == 1;
    if cons_ok {
        s.ok("F6006-视-02-约束可视锚点精确");
    } else {
        s.fail("F6006-视-02-约束可视锚点精确", "约束可视失效");
    }

    // --- 单帧步进（判据三）---
    // 步-01：step_once 推一帧记一迹——圆 vx=3 → cx 0→3；盒 vx=-2 →
    // x0 -2/x1 6；轨迹行 (frame=1, cost, body_count=2)；Viewer 拒。
    let mut d3 = PhysDebugger::new(DebugAuth::Viewer);
    d3.register_body(PhysBody { id: 1, shape: PhysShape::Circle { cx: 0, cy: 0, r: 1 }, vx: 3, vy: 0 });
    let denied = d3.step_once(10);
    d3.auth = DebugAuth::Editor;
    d3.register_body(PhysBody { id: 2, shape: PhysShape::Aabb { x0: 0, y0: 0, x1: 8, y1: 4 }, vx: -2, vy: 0 });
    let f1 = d3.step_once(20).expect("probe-3");
    let t = &d3.traces[0];
    let step_ok = matches!(denied, Err(E_BD_NOAUTH))
        && f1 == 1
        && t.frame == 1
        && t.cost_us == 20
        && t.body_count == 2
        && d3.travel.len() == 1;
    if step_ok {
        s.ok("F6006-步-01-单帧步进轨迹记录");
    } else {
        s.fail("F6006-步-01-单帧步进轨迹记录", "单帧步进失效");
    }

    // --- 时间旅行（判据四）---
    // 步-02：3 帧快照回看——travel_back(0)=最新 frame3、back(1)=frame2、
    // back(2)=frame1、back(99) 钳制到最旧 frame1 不 panic；环容量 8
    // 满载丢最旧 dropped=1（压 9 帧后最旧是 frame2）。
    let mut d4 = PhysDebugger::new(DebugAuth::Editor);
    d4.register_body(PhysBody { id: 1, shape: PhysShape::Circle { cx: 0, cy: 0, r: 1 }, vx: 0, vy: 0 });
    for _ in 0..3 {
        let _ = d4.step_once(1);
    }
    let f_b0 = d4.travel.travel_back(0).expect("probe-4a").frame;
    let f_b1 = d4.travel.travel_back(1).expect("probe-4b").frame;
    let f_b2 = d4.travel.travel_back(2).expect("probe-4c").frame;
    let f_clamp = d4.travel.travel_back(99).expect("probe-4d").frame;
    for _ in 3..9 {
        let _ = d4.step_once(1);
    }
    let f_oldest = d4.travel.travel_back(7).expect("probe-4e").frame;
    let travel_ok = f_b0 == 3
        && f_b1 == 2
        && f_b2 == 1
        && f_clamp == 1
        && d4.travel.dropped == 1
        && f_oldest == 2
        && d4.travel.len() == 8;
    if travel_ok {
        s.ok("F6006-步-02-时间旅行钳制与环形");
    } else {
        s.fail("F6006-步-02-时间旅行钳制与环形", "时间旅行失效");
    }

    // --- 预算声明与降级预告（判据五）---
    // 预-01：预算 100µs——首张 150µs 超标 → overruns=1 + 预告置位
    // （本张照出预告是「下次降」）；落档前出第二张仍 Full；落档后
    // 档位 Full→Half 段数 16→8（6+2）；Quarter 到底再落档不降。
    let mut d5 = PhysDebugger::new(DebugAuth::Editor);
    d5.register_body(PhysBody { id: 1, shape: PhysShape::Circle { cx: 0, cy: 0, r: 10 }, vx: 0, vy: 0 });
    d5.register_body(PhysBody { id: 2, shape: PhysShape::Aabb { x0: 0, y0: 0, x1: 8, y1: 4 }, vx: 0, vy: 0 });
    let v_over = d5.wireframe_view(150).expect("probe-5a");
    let over_ok = v_over.len() == 16
        && d5.budget_overruns == 1
        && d5.pending_degrade
        && d5.detail == Detail::Full; // 首张照出，预告未生效
    let degraded1 = d5.apply_pending_degrade(); // Full→Half
    let v_half = d5.wireframe_view(150).expect("probe-5b");
    let half_ok = v_half.len() == 8 && d5.detail == Detail::Half; // Half: 6+2
    let degraded2 = d5.apply_pending_degrade(); // Half→Quarter
    let v_quarter = d5.wireframe_view(150).expect("probe-5c");
    let quarter_ok = v_quarter.len() == 5 && d5.detail == Detail::Quarter; // 4+1
    let degraded3 = d5.apply_pending_degrade(); // Quarter 到底返回 False
    let v_floor = d5.wireframe_view(150).expect("probe-5d");
    let budget_ok = over_ok
        && degraded1
        && half_ok
        && degraded2
        && quarter_ok
        && !degraded3
        && v_floor.len() == 5
        && d5.detail == Detail::Quarter;
    if budget_ok {
        s.ok("F6006-预-01-超标预告降级有底");
    } else {
        s.fail("F6006-预-01-超标预告降级有底", "降级预告失效");
    }

    // --- 越权拒绝（判据六）---
    // 权-01：Off 级视图/约束/步进全拒 E_BD_NOAUTH；Viewer 可看不可
    // 步进（步进改变世界状态）；导出闸 Editor 专属——Viewer 拒。
    let mut d6 = PhysDebugger::new(DebugAuth::Off);
    d6.register_body(PhysBody { id: 1, shape: PhysShape::Circle { cx: 0, cy: 0, r: 1 }, vx: 0, vy: 0 });
    let off_v = d6.wireframe_view(1);
    let off_c = d6.constraint_view();
    let off_s = d6.step_once(1);
    d6.auth = DebugAuth::Viewer;
    let viewer_v = d6.wireframe_view(1);
    let viewer_s = d6.step_once(1);
    let viewer_x = d6.export_advance();
    d6.auth = DebugAuth::Editor;
    let editor_x = d6.export_advance();
    let auth_ok = matches!(off_v, Err(E_BD_NOAUTH))
        && matches!(off_c, Err(E_BD_NOAUTH))
        && matches!(off_s, Err(E_BD_NOAUTH))
        && viewer_v.is_ok()
        && matches!(viewer_s, Err(E_BD_NOAUTH))
        && matches!(viewer_x, Err(E_BD_NOAUTH))
        && matches!(editor_x, Ok(ExportGate::Armed));
    if auth_ok {
        s.ok("F6006-权-01-权限递增链三级");
    } else {
        s.fail("F6006-权-01-权限递增链三级", "权限闸失效");
    }

    // --- 调试数据不出机（判据七）---
    // 私-01：导出默认关→digest 拒 E_BD_LEAK；Editor 三步 Closed→Armed→
    // Open；Open 后 digest 只出统计（体 1 段 12 约束 0 档位 Full）——
    // 结构体无坐标字段（类型面证明：ExportDigest 无 x/y 字段）。
    let mut d7 = PhysDebugger::new(DebugAuth::Editor);
    d7.register_body(PhysBody { id: 1, shape: PhysShape::Circle { cx: 0, cy: 0, r: 10 }, vx: 0, vy: 0 });
    let leak1 = d7.export_digest();
    let leak1_ok = matches!(leak1, Err(E_BD_LEAK));
    let g1 = d7.export_advance().expect("probe-7a");
    let leak2 = d7.export_digest();
    let leak2_is_err = matches!(leak2, Err(E_BD_LEAK));
    let g2 = d7.export_advance().expect("probe-7b");
    let digest = d7.export_digest().expect("probe-7c");
    let priv_ok = leak1_ok
        && g1 == ExportGate::Armed
        && leak2_is_err
        && g2 == ExportGate::Open
        && digest.body_count == 1
        && digest.seg_count == 12
        && digest.constraint_count == 0
        && digest.detail == Detail::Full;
    if priv_ok {
        s.ok("F6006-私-01-出机闸三步脱敏面");
    } else {
        s.fail("F6006-私-01-出机闸三步脱敏面", "导出闸失效");
    }

    // --- 判据元（判据八）---
    // 元-01：五码互异 + reason 文案互异非空 + 缺省兜底 + 零 panic
    // 空态平凡（空世界 Viewer 视图显性 E_BD_EMPTY 不是静默黑屏）。
    let codes = [E_BD_NOAUTH, E_BD_BUDGET, E_BD_EMPTY, E_BD_LEAK, E_BD_NOPHYS];
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] || reason_of(codes[i]) == reason_of(codes[j]) {
                distinct = false;
            }
        }
    }
    let meta_codes = distinct
        && reason_of("E_BD_UNKNOWN") == "未知诊断码"
        && codes.iter().all(|c| !reason_of(c).is_empty());
    let mut d8 = PhysDebugger::new(DebugAuth::Viewer);
    let empty_v = d8.wireframe_view(1);
    let empty_ok = matches!(empty_v, Err(E_BD_EMPTY)) && d8.status_spoken().contains("体 0 个");
    if meta_codes && empty_ok {
        s.ok("F6006-元-01-五码互异空态显性");
    } else {
        s.fail("F6006-元-01-五码互异空态显性", "判据元失效");
    }

    // --- 编排（判据九）---
    // 编-01：全链路 O(物理体数)——Editor 登记圆+盒+1 约束 → 步进 2 帧
    // （圆 cx 0→1→2 vx=1）→ 回看 back(1)=frame1 cx=1 恒等 → 视图段数
    // 对账 Full 16 → 超标 150µs 预告 → 落档 Half 8 → 状态可读对账。
    let mut d9 = PhysDebugger::new(DebugAuth::Editor);
    d9.register_body(PhysBody { id: 1, shape: PhysShape::Circle { cx: 0, cy: 0, r: 10 }, vx: 1, vy: 0 });
    d9.register_body(PhysBody { id: 2, shape: PhysShape::Aabb { x0: 0, y0: 0, x1: 8, y1: 4 }, vx: 0, vy: 0 });
    let _ = d9.register_constraint(PhysConstraint { id: 1, body_a: 1, body_b: 2, anchor_a: (2, 0), anchor_b: (4, 1) });
    let _ = d9.step_once(10);
    let _ = d9.step_once(10);
    let look = d9.travel.travel_back(1).expect("probe-9a");
    let look_ok = look.frame == 1 && look.bodies[0] == (1, 1, 0);
    let v_full = d9.wireframe_view(50).expect("probe-9b");
    let _ = d9.wireframe_view(150).expect("probe-9c");
    let _ = d9.apply_pending_degrade();
    let v_half = d9.wireframe_view(50).expect("probe-9d");
    let spoken = d9.status_spoken();
    let chain_ok = look_ok
        && v_full.len() == 16
        && v_half.len() == 8
        && d9.traces.len() == 2
        && spoken.contains("体 2 个")
        && spoken.contains("约束 1 条")
        && spoken.contains("档位 Half");
    if chain_ok {
        s.ok("F6006-编-01-全链路O体数对账");
    } else {
        s.fail("F6006-编-01-全链路O体数对账", "编排失效");
    }

    s
}
