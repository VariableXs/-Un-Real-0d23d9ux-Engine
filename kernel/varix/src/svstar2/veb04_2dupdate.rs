//! VE-F0204 · virtio 2D 内容更新（VE-B 域 · GPU 驱动矩阵 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0204`
//!
//! **规格原文**：桌面 UI 的主要内容通道：TRANSFER_TO_HOST_2D（把 guest 渲染
//! 好的位图推到 host 资源）+ RESOURCE_FLUSH（把资源刷到 scanout）。优化三招：
//! 脏区裁剪（只传 dirty rect，r/x/y/w 精确到像素，实测减少传输量 60-90%）、
//! 传输合批（同资源多脏区合并为单命令多 rect 或相邻合并）、流水线（TRANSFER
//! 与 FLUSH 异步流水，靠围栏保证 flush 时传输已完成）。节流：光标移动等高频
//! 小更新与帧级大更新分开限速。与 VE-D 合成引擎对接：合成输出的脏区直接翻译
//! 为本层参数。判据：脏区裁剪正确性（屏幕内容无残影）、合批收益实测、流水线
//! 围栏正确、限速策略、4K 桌面滚动场景带宽实测。
//!
//! **设计要点**：
//! - 裁剪零丢失铁律：脏区越界一律裁到资源边界内而不是整体丢弃——裁剪丢像素
//!   就是残影，"无残影"判据的对偶是"bounds 内的脏像素一个不许少"（覆盖率
//!   采样对拍在 checks 里逐格验证）；
//! - 合批两档：相邻（间距 ≤ MERGE_GAP_PX）且面积代价可容忍才并——瞎并会把
//!   两个小矩形并成一个大矩形反而多传字节；矩形数超批上限时强制并（选面积
//!   增量最小的对，确定性不靠运气）；
//! - 流水线围栏：每笔 TRANSFER 登记围栏，FLUSH 只在该资源全部在途传输完成
//!   后发射；未完成时 flush 挂起（deferred），最后一笔传输完成即释放——
//!   "flush 时传输已完成"由账面保证，不靠时序运气；
//! - 限速分档：小更新（≤ SMALL_UPDATE_AREA_PX，光标级）走令牌桶按次数限速，
//!   帧级大更新走每帧字节预算；被限速的脏区**并入下一帧待发集**（合流不清
//!   丢——限速是节奏策略不是丢弃策略，丢弃就是残影）；
//! - 带宽台账：全程记账 full-frame vs 实发字节，4K 滚动场景的 60-90% 节省
//!   是测出来的数字不是口号（checks 内置滚动场景实测）。

use super::veb02_proto::{CtrlCommand, Rect};
use super::veb03_resource::{fmt_bpp, ResourceTable, PAGE_SIZE};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量与配置
// ---------------------------------------------------------------------------

/// 小更新面积上限（64×64）：光标移动、单控件微调量级。
pub const SMALL_UPDATE_AREA_PX: u64 = 64 * 64;
/// 合批判定：两矩形膨胀 gap 像素后相交即视为相邻。
pub const MERGE_GAP_PX: u32 = 16;
/// 合批面积容忍度：并后面积 ≤ 并前面积 × (1 + 容忍度) 才并（百分比）。
pub const MERGE_TOLERANCE_PCT: u64 = 100;
/// 单帧计划矩形上限——超出强制合批（描述符与命令条目都是有限资源）。
pub const MAX_RECTS_PER_BATCH: usize = 8;
/// 待发集上限：超出直接并成包围盒（合并覆盖全部脏像素，零丢失）。
pub const MAX_PENDING_RECTS: usize = 64;
/// 帧间隔（µs）：80fps 对应 12500。
pub const FRAME_INTERVAL_US: u64 = 12_500;
/// 默认帧级字节预算：8 MB/帧（4K 全帧约 31.6 MB，预算内走不完一整帧全量，
/// 全量更新必须分帧——这正是预算存在的意义）。
pub const FRAME_BUDGET_BYTES_DEFAULT: u64 = 8 * 1024 * 1024;
/// 小更新令牌桶：桶容量 8、每帧回填 2（光标高频微动不淹没控制队列）。
pub const SMALL_BUCKET_CAP: u64 = 8;
pub const SMALL_REFILL_PER_FRAME: u64 = 2;

/// stride 计算：行字节数向上对齐到页（与 veb03 导出句柄的 stride 纪律一致）。
pub fn page_aligned_stride(width: u32, bpp: u32) -> u64 {
    let row = width as u64 * bpp as u64;
    (row + PAGE_SIZE as u64 - 1) / PAGE_SIZE as u64 * PAGE_SIZE as u64
}

/// TRANSFER_TO_HOST_2D 的 offset 语义：rect 首像素在 backing 中的字节偏移。
pub fn offset_of(r: &Rect, stride: u64, bpp: u32) -> u64 {
    r.y as u64 * stride + r.x as u64 * bpp as u64
}

// ---------------------------------------------------------------------------
// 二、矩形代数（裁剪/合并/覆盖的纯函数层）
// ---------------------------------------------------------------------------

/// 矩形面积（u64，防 4K 以上尺寸乘法溢出）。
pub fn rect_area(r: &Rect) -> u64 {
    r.width as u64 * r.height as u64
}

/// 裁剪：r 与 bounds 相交部分。完全在外或空矩形 → None。
///
/// 铁律：只裁不丢——bounds 内的部分必须保留，整体丢弃才会残影。
pub fn clip_rect(r: &Rect, bounds: &Rect) -> Option<Rect> {
    let x1 = r.x.max(bounds.x);
    let y1 = r.y.max(bounds.y);
    let x2 = r.x.saturating_add(r.width).min(bounds.x.saturating_add(bounds.width));
    let y2 = r.y.saturating_add(r.height).min(bounds.y.saturating_add(bounds.height));
    if x2 <= x1 || y2 <= y1 {
        return None;
    }
    Some(Rect {
        x: x1,
        y: y1,
        width: x2 - x1,
        height: y2 - y1,
    })
}

/// 包围盒并集（两个非空矩形）。
pub fn union_rect(a: &Rect, b: &Rect) -> Rect {
    let x1 = a.x.min(b.x);
    let y1 = a.y.min(b.y);
    let x2 = (a.x + a.width).max(b.x + b.width);
    let y2 = (a.y + a.height).max(b.y + b.height);
    Rect {
        x: x1,
        y: y1,
        width: x2 - x1,
        height: y2 - y1,
    }
}

/// 膨胀 gap 后相交（相邻判定：视觉上连成一片的脏区）。
pub fn rects_adjacent(a: &Rect, b: &Rect, gap: u32) -> bool {
    let ax2 = a.x + a.width + gap;
    let ay2 = a.y + a.height + gap;
    let bx2 = b.x + b.width + gap;
    let by2 = b.y + b.height + gap;
    a.x < bx2 && b.x < ax2 && a.y < by2 && b.y < ay2
}

/// 覆盖率采样对拍：bounds 内的采样点上，raw 覆盖 ⟺ clipped 覆盖。
///
/// 这是"无残影"的机器可验形式：裁剪不许丢掉 bounds 内任何脏像素。
/// step 为采样步长（像素）；0 步长按 1 处理。
pub fn coverage_preserved(raw: &[Rect], clipped: &[Rect], bounds: &Rect, step: u32) -> bool {
    let step = step.max(1) as u64;
    let w = bounds.width as u64;
    let h = bounds.height as u64;
    if w == 0 || h == 0 {
        return true;
    }
    // 采样点 = 每行起点、step 中点、行尾前一点（边界点最易漏）。
    let mut xs: Vec<u64> = Vec::new();
    let mut i = 0;
    while i < w {
        xs.push(i);
        if i + step / 2 < w {
            xs.push(i + step / 2);
        }
        i += step;
    }
    xs.push(w.saturating_sub(1));
    let mut ys: Vec<u64> = Vec::new();
    let mut j = 0;
    while j < h {
        ys.push(j);
        if j + step / 2 < h {
            ys.push(j + step / 2);
        }
        j += step;
    }
    ys.push(h.saturating_sub(1));

    let hit = |rects: &[Rect], px: u64, py: u64| {
        rects.iter().any(|r| {
            (px as u32) >= r.x
                && (px as u32) < r.x + r.width
                && (py as u32) >= r.y
                && (py as u32) < r.y + r.height
        })
    };
    for py in ys.iter() {
        for px in xs.iter() {
            if hit(raw, *px, *py) != hit(clipped, *px, *py) {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 三、合批规划（确定性贪心 + 强制并）
// ---------------------------------------------------------------------------

/// 合批：相邻且面积代价可容忍则并；超批上限强制并（增量最小对优先）。
///
/// 输入先按 (y, x) 排序——同输入同输出，确定性纪律。
pub fn merge_all(rects: &[Rect], gap: u32, tolerance_pct: u64) -> Vec<Rect> {
    let mut items: Vec<Rect> = rects
        .iter()
        .filter(|r| r.width > 0 && r.height > 0)
        .copied()
        .collect();
    items.sort_by_key(|r| (r.y, r.x));

    let mut out: Vec<Rect> = Vec::new();
    for r in items {
        let mut merged = false;
        for o in out.iter_mut() {
            if rects_adjacent(o, &r, gap) {
                let u = union_rect(o, &r);
                let sum = rect_area(o) + rect_area(&r);
                if rect_area(&u) * 100 <= sum * (100 + tolerance_pct) {
                    *o = u;
                    merged = true;
                    break;
                }
            }
        }
        if !merged {
            out.push(r);
        }
    }

    // 强制合批：矩形数超上限，反复并"面积增量最小"的一对（确定性扫描序）。
    while out.len() > MAX_RECTS_PER_BATCH {
        let mut best: Option<(usize, usize, u64)> = None; // (i, j, 增量)
        for i in 0..out.len() {
            for j in (i + 1)..out.len() {
                let growth =
                    rect_area(&union_rect(&out[i], &out[j])) - rect_area(&out[i]) - rect_area(&out[j]);
                match best {
                    Some((_, _, g)) if g <= growth => {}
                    _ => best = Some((i, j, growth)),
                }
            }
        }
        let (i, j, _) = best.expect("长度超上限时必存在一对");
        let u = union_rect(&out[i], &out[j]);
        out[i] = u;
        out.swap_remove(j);
    }
    out
}

// ---------------------------------------------------------------------------
// 四、限速（小更新令牌桶 + 帧级字节预算，逻辑时钟）
// ---------------------------------------------------------------------------

/// 更新档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateClass {
    /// 光标级小更新：按次数限速。
    CursorSmall,
    /// 帧级大更新：按字节预算限速。
    FrameMajor,
}

impl UpdateClass {
    pub fn label(self) -> &'static str {
        match self {
            UpdateClass::CursorSmall => "光标级小更新",
            UpdateClass::FrameMajor => "帧级大更新",
        }
    }
}

pub fn classify(area_px: u64) -> UpdateClass {
    if area_px <= SMALL_UPDATE_AREA_PX {
        UpdateClass::CursorSmall
    } else {
        UpdateClass::FrameMajor
    }
}

/// 限速决策。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateVerdict {
    Allowed,
    Deferred,
}

/// 限速器：逻辑时钟（µs）驱动，零墙钟、可复现。
pub struct UpdateThrottle {
    small_tokens: u64,
    small_last_refill_frame: u64,
    frame_window_start: u64,
    frame_bytes_used: u64,
    pub small_allowed: u64,
    pub small_deferred: u64,
    pub frame_allowed: u64,
    pub frame_deferred: u64,
}

impl UpdateThrottle {
    pub fn new() -> UpdateThrottle {
        UpdateThrottle {
            small_tokens: SMALL_BUCKET_CAP,
            small_last_refill_frame: 0,
            frame_window_start: 0,
            frame_bytes_used: 0,
            small_allowed: 0,
            small_deferred: 0,
            frame_allowed: 0,
            frame_deferred: 0,
        }
    }

    /// 门禁：class 档位、cost（小更新=1 次，帧级=字节数）、frame_budget 帧预算。
    pub fn gate(&mut self, class: UpdateClass, cost: u64, frame_budget: u64, now: u64) -> GateVerdict {
        match class {
            UpdateClass::CursorSmall => {
                // 按帧回填：距上次回填过了几帧就补几个令牌（不回填溢出桶容）。
                let frame_no = now / FRAME_INTERVAL_US;
                if frame_no > self.small_last_refill_frame {
                    let elapsed = (frame_no - self.small_last_refill_frame).min(1 << 30);
                    self.small_tokens = (self.small_tokens + elapsed * SMALL_REFILL_PER_FRAME)
                        .min(SMALL_BUCKET_CAP);
                    self.small_last_refill_frame = frame_no;
                }
                if self.small_tokens >= 1 {
                    self.small_tokens -= 1;
                    self.small_allowed += 1;
                    GateVerdict::Allowed
                } else {
                    self.small_deferred += 1;
                    GateVerdict::Deferred
                }
            }
            UpdateClass::FrameMajor => {
                // 字节窗口：窗口滚动重置用量。
                if now >= self.frame_window_start + FRAME_INTERVAL_US {
                    self.frame_window_start = now - (now % FRAME_INTERVAL_US);
                    self.frame_bytes_used = 0;
                }
                // 防饿死条款：窗口首笔不受预算上限钳制——超预算的全帧更新
                // 若被永久推迟就是活锁（残影永不消失）；预算管的是"同窗口
                // 内还有多少额度给后续笔"，不是"单笔够不够格"。
                if self.frame_bytes_used == 0
                    || self.frame_bytes_used + cost <= frame_budget
                {
                    self.frame_bytes_used += cost;
                    self.frame_allowed += 1;
                    GateVerdict::Allowed
                } else {
                    self.frame_deferred += 1;
                    GateVerdict::Deferred
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 五、流水线围栏账本（flush 时传输已完成——账面保证）
// ---------------------------------------------------------------------------

/// flush 请求裁决。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlushDecision {
    /// 资源无在途传输，flush 立即发射。
    Emit,
    /// 有在途传输，flush 挂起，等待最后一笔完成时释放。
    Deferred { waiting_on: usize },
}

/// 流水线围栏账本。
pub struct FenceLedger {
    /// (资源, 围栏) 在途传输表。
    outstanding: Vec<(u32, u64)>,
    /// 挂起的 flush：资源与 flush 包围盒。
    deferred_flushes: Vec<(u32, Rect)>,
    next_fence: u64,
    pub transfers_registered: u64,
    pub transfers_completed: u64,
    pub flushes_emitted_direct: u64,
    pub flushes_deferred: u64,
    pub flushes_released: u64,
}

impl FenceLedger {
    pub fn new() -> FenceLedger {
        FenceLedger {
            outstanding: Vec::new(),
            deferred_flushes: Vec::new(),
            next_fence: 1,
            transfers_registered: 0,
            transfers_completed: 0,
            flushes_emitted_direct: 0,
            flushes_deferred: 0,
            flushes_released: 0,
        }
    }

    /// 登记一笔在途传输，返回围栏号（单调分配）。
    pub fn register_transfer(&mut self, resource_id: u32) -> u64 {
        let f = self.next_fence;
        self.next_fence += 1;
        self.outstanding.push((resource_id, f));
        self.transfers_registered += 1;
        f
    }

    /// 该资源的在途传输数。
    pub fn outstanding_of(&self, resource_id: u32) -> usize {
        self.outstanding.iter().filter(|(r, _)| *r == resource_id).count()
    }

    /// flush 请求：无在途 → Emit；有在途 → 挂起并返回等待数。
    pub fn request_flush(&mut self, resource_id: u32, rect: Rect) -> FlushDecision {
        let n = self.outstanding_of(resource_id);
        if n == 0 {
            self.flushes_emitted_direct += 1;
            FlushDecision::Emit
        } else {
            self.deferred_flushes.push((resource_id, rect));
            self.flushes_deferred += 1;
            FlushDecision::Deferred { waiting_on: n }
        }
    }

    /// 传输完成：按围栏销账；随后释放该资源已无在途的挂起 flush。
    ///
    /// 返回 (资源是否找到, 释放的 flush 命令体)。未知围栏返回 None——
    /// 串扰显性化，不静默吞。
    pub fn complete_transfer(&mut self, fence: u64) -> Option<(u32, Vec<(u32, Rect)>)> {
        let pos = self.outstanding.iter().position(|(_, f)| *f == fence)?;
        let (res, _) = self.outstanding.remove(pos);
        self.transfers_completed += 1;
        if self.outstanding_of(res) == 0 {
            let released: Vec<(u32, Rect)> = self
                .deferred_flushes
                .iter()
                .copied()
                .filter(|(r, _)| *r == res)
                .collect();
            self.deferred_flushes.retain(|(r, _)| *r != res);
            self.flushes_released += released.len() as u64;
            Some((res, released))
        } else {
            Some((res, Vec::new()))
        }
    }

    /// 不变量：资源无在途传输时不得有挂起 flush（flush 等传输，账面必须闭合）。
    pub fn invariant_closed(&self) -> bool {
        self.deferred_flushes
            .iter()
            .all(|(r, _)| self.outstanding_of(*r) > 0)
    }
}

// ---------------------------------------------------------------------------
// 六、带宽台账（合批/裁剪收益与 4K 场景的实测数字）
// ---------------------------------------------------------------------------

/// 带宽台账：全部按实发记账，收益是算出来的不是报出来的。
pub struct BandwidthLedger {
    pub frames: u64,
    pub full_frame_bytes_would: u64,
    pub raw_bytes_before_merge: u64,
    pub bytes_sent: u64,
    pub commands_sent: u64,
    pub raw_commands_would: u64,
    pub deferred_rects_total: u64,
}

impl BandwidthLedger {
    pub fn new() -> BandwidthLedger {
        BandwidthLedger {
            frames: 0,
            full_frame_bytes_would: 0,
            raw_bytes_before_merge: 0,
            bytes_sent: 0,
            commands_sent: 0,
            raw_commands_would: 0,
            deferred_rects_total: 0,
        }
    }

    /// 裁剪节省（全帧字节 vs 裁剪后字节）百分比（整数运算）。
    pub fn clip_savings_pct(&self) -> u64 {
        if self.full_frame_bytes_would == 0 {
            return 0;
        }
        let planned = self.bytes_sent.max(1);
        (self.full_frame_bytes_would.saturating_sub(planned)) * 100
            / self.full_frame_bytes_would
    }

    /// 合批节省（命令条数减少）百分比。
    pub fn merge_savings_pct(&self) -> u64 {
        if self.raw_commands_would == 0 {
            return 0;
        }
        (self.raw_commands_would.saturating_sub(self.commands_sent)) * 100
            / self.raw_commands_would
    }

    /// 均摊每帧字节（4K 滚动场景的实测口径）。
    pub fn avg_bytes_per_frame(&self) -> u64 {
        if self.frames == 0 {
            return 0;
        }
        self.bytes_sent / self.frames
    }
}

// ---------------------------------------------------------------------------
// 七、帧计划与更新引擎（VE-D 脏区 → 命令的唯一入口）
// ---------------------------------------------------------------------------

/// 更新错误（三要素，异常零静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateError {
    pub code: &'static str,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl UpdateError {
    fn new(code: &'static str, what: String, why: String, next: String) -> UpdateError {
        UpdateError { code, what, why, next }
    }
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

/// 一帧的提交产物。
#[derive(Clone, Debug)]
pub struct FrameSubmission {
    pub resource_id: u32,
    /// (围栏, 命令)——传输命令已按序登记围栏。
    pub transfers: Vec<(u64, CtrlCommand)>,
    /// 立即可发射的 flush（无在途传输时）；挂起时为 None 且 flush_pending=true。
    pub flush: Option<CtrlCommand>,
    pub flush_pending: bool,
    /// 本帧实发字节。
    pub emitted_bytes: u64,
    /// 被限速推迟的矩形数（已并入待发集，下帧合流发出）。
    pub deferred_rects: usize,
    pub full_frame_bytes: u64,
}

/// 命令日志条目（流水线不变量的证据链）。
#[derive(Clone, Debug)]
pub struct LogEntry {
    pub cmd: CtrlCommand,
    /// flush 发射时刻该资源的在途传输数（判据：必须为 0）。
    pub outstanding_at_emit: usize,
}

/// VE-F0204 更新引擎：合成器脏区 → 裁剪 → 合批 → 限速 → 传输+flush 流水线。
pub struct TwoDUpdateEngine {
    pub throttle: UpdateThrottle,
    pub ledger: FenceLedger,
    pub bandwidth: BandwidthLedger,
    /// 待发集（限速推迟的脏区按资源合流，下一帧优先发出——不清丢）。
    pending: Vec<(u32, Vec<Rect>)>,
    /// 命令日志：发射序证据链（flush 必须在传输全部完成后）。
    pub log: Vec<LogEntry>,
}

impl TwoDUpdateEngine {
    pub fn new() -> TwoDUpdateEngine {
        TwoDUpdateEngine {
            throttle: UpdateThrottle::new(),
            ledger: FenceLedger::new(),
            bandwidth: BandwidthLedger::new(),
            pending: Vec::new(),
            log: Vec::new(),
        }
    }

    /// 待发集里某资源的脏区（取出并清空）。
    fn take_pending(&mut self, resource_id: u32) -> Vec<Rect> {
        if let Some(pos) = self.pending.iter().position(|(r, _)| *r == resource_id) {
            let (_, rects) = self.pending.swap_remove(pos);
            rects
        } else {
            Vec::new()
        }
    }

    fn push_pending(&mut self, resource_id: u32, rects: Vec<Rect>) {
        if rects.is_empty() {
            return;
        }
        if let Some(e) = self.pending.iter_mut().find(|(r, _)| *r == resource_id) {
            e.1.extend(rects);
            // 待发集封顶：并成包围盒（覆盖全部脏像素，零丢失）。
            if e.1.len() > MAX_PENDING_RECTS {
                let mut u = e.1[0];
                for r in e.1[1..].iter() {
                    u = union_rect(&u, r);
                }
                e.1 = vec![u];
            }
        } else {
            self.pending.push((resource_id, rects));
        }
    }

    /// 提交一帧：合成器脏区（VE-D 输出）→ 本层参数 → 命令计划。
    ///
    /// 资源必须在 veb03 资源表内且为 2D 资源；stride 按页对齐规则推导。
    /// 被限速的矩形并入待发集（下帧合流），不是丢弃。
    pub fn submit_frame(
        &mut self,
        table: &ResourceTable,
        resource_id: u32,
        dirty: &[Rect],
        now: u64,
    ) -> Result<FrameSubmission, UpdateError> {
        let res = table.find(resource_id).ok_or_else(|| {
            UpdateError::new(
                "E_NO_RESOURCE",
                format!("资源 {} 不在 veb03 资源表内", resource_id),
                "内容更新只能落在已创建的资源上——句柄表是唯一事实源".to_string(),
                "先经 VE-F0203 创建资源并挂 backing，再提交内容更新".to_string(),
            )
        })?;
        if res.is_3d {
            return Err(UpdateError::new(
                "E_NOT_2D_PATH",
                format!("资源 {} 是 3D 资源，不能走 2D 内容通道", resource_id),
                "3D 资源的内容更新走 TRANSFER_TO_HOST_3D 通路，语义与 2D 不同".to_string(),
                "3D 通路实现（VE-B 域 3D 批次）就位前，3D 资源更新显性拒绝".to_string(),
            )
            );
        }
        let bpp = fmt_bpp(res.format).ok_or_else(|| {
            UpdateError::new(
                "E_FORMAT_UNKNOWN",
                format!("资源 {} 格式 {} 未登记 bpp", resource_id, res.format),
                "无 bpp 就无法算 stride 与 offset，硬算就是瞎编".to_string(),
                "在 fmt_bpp 登记格式（带依据）或改用已登记格式".to_string(),
            )
        })?;
        let stride = page_aligned_stride(res.width, bpp);
        let bounds = Rect {
            x: 0,
            y: 0,
            width: res.width,
            height: res.height,
        };
        let full_frame_bytes = rect_area(&bounds) * bpp as u64;

        // 1) 待发集合流 + 本帧脏区，全部裁剪到资源边界内（只裁不丢）。
        let carried = self.take_pending(resource_id);
        let mut all: Vec<Rect> = Vec::with_capacity(carried.len() + dirty.len());
        all.extend(carried);
        all.extend(dirty.iter().copied());
        let clipped: Vec<Rect> = all.iter().filter_map(|r| clip_rect(r, &bounds)).collect();
        let raw_count = clipped.len();

        // 2) 合批（确定性贪心）。
        let merged = merge_all(&clipped, MERGE_GAP_PX, MERGE_TOLERANCE_PCT);

        // 3) 限速分档门禁。
        let mut emitted: Vec<Rect> = Vec::new();
        let mut deferred: Vec<Rect> = Vec::new();
        for r in merged.iter() {
            let area = rect_area(r);
            let cost = if classify(area) == UpdateClass::CursorSmall {
                1
            } else {
                area * bpp as u64
            };
            match self.throttle.gate(classify(area), cost, FRAME_BUDGET_BYTES_DEFAULT, now) {
                GateVerdict::Allowed => emitted.push(*r),
                GateVerdict::Deferred => deferred.push(*r),
            }
        }
        let deferred_count = deferred.len();
        self.push_pending(resource_id, deferred);

        // 4) 传输命令 + 围栏登记。
        let mut transfers: Vec<(u64, CtrlCommand)> = Vec::new();
        let mut emitted_bytes: u64 = 0;
        for r in emitted.iter() {
            let cmd = CtrlCommand::TransferToHost2d {
                resource_id,
                rect: *r,
                offset: offset_of(r, stride, bpp),
            };
            let fence = self.ledger.register_transfer(resource_id);
            emitted_bytes += rect_area(r) * bpp as u64;
            self.log.push(LogEntry {
                cmd: cmd.clone(),
                outstanding_at_emit: usize::MAX, // 传输命令无此语义
            });
            transfers.push((fence, cmd));
        }

        // 5) flush：无在途 → 立即发射；有在途 → 挂起等最后一笔完成释放。
        let mut flush = None;
        let mut flush_pending = false;
        if !emitted.is_empty() {
            let mut bbox = emitted[0];
            for r in emitted[1..].iter() {
                bbox = union_rect(&bbox, r);
            }
            let flush_cmd = CtrlCommand::ResourceFlush {
                resource_id,
                rect: bbox,
            };
            match self.ledger.request_flush(resource_id, bbox) {
                FlushDecision::Emit => {
                    self.log.push(LogEntry {
                        cmd: flush_cmd.clone(),
                        outstanding_at_emit: 0,
                    });
                    flush = Some(flush_cmd);
                }
                FlushDecision::Deferred { .. } => {
                    flush_pending = true;
                }
            }
        }

        // 6) 台账记账。
        self.bandwidth.frames += 1;
        self.bandwidth.full_frame_bytes_would += full_frame_bytes;
        self.bandwidth.raw_bytes_before_merge += clipped.iter().map(rect_area).sum::<u64>() * bpp as u64;
        self.bandwidth.bytes_sent += emitted_bytes;
        self.bandwidth.commands_sent += transfers.len() as u64 + if flush.is_some() { 1 } else { 0 };
        self.bandwidth.raw_commands_would += raw_count as u64 + 1;
        self.bandwidth.deferred_rects_total += deferred_count as u64;

        Ok(FrameSubmission {
            resource_id,
            transfers,
            flush,
            flush_pending,
            emitted_bytes,
            deferred_rects: deferred_count,
            full_frame_bytes,
        })
    }

    /// 传输完成回调：销围栏账，释放已就绪的挂起 flush。
    ///
    /// 返回本笔完成释放出来的 flush 命令（可能为空）。未知围栏显性报错。
    pub fn on_transfer_complete(
        &mut self,
        fence: u64,
    ) -> Result<Vec<CtrlCommand>, UpdateError> {
        match self.ledger.complete_transfer(fence) {
            None => Err(UpdateError::new(
                "E_UNKNOWN_FENCE",
                format!("围栏 {} 不在在途账上", fence),
                "完成回调对不上登记——串扰或重复完成，账面闭合被破坏".to_string(),
                "按设备可疑处置并联动 F0009；检查是否重复回调".to_string(),
            )),
            Some((_, released)) => {
                let mut out = Vec::new();
                for (res, rect) in released {
                    let cmd = CtrlCommand::ResourceFlush {
                        resource_id: res,
                        rect,
                    };
                    self.log.push(LogEntry {
                        cmd: cmd.clone(),
                        outstanding_at_emit: self.ledger.outstanding_of(res),
                    });
                    out.push(cmd);
                }
                Ok(out)
            }
        }
    }

    /// 待发集里某资源的脏区条数（读屏与自检用）。
    pub fn pending_rects_of(&self, resource_id: u32) -> usize {
        self.pending
            .iter()
            .find(|(r, _)| *r == resource_id)
            .map(|(_, v)| v.len())
            .unwrap_or(0)
    }

    /// 待发集里有脏区的资源数。
    pub fn pending_resource_count(&self) -> usize {
        self.pending.len()
    }

    /// 流水线不变量：日志里每条 flush 发射时刻，其资源在途传输数为 0。
    pub fn pipeline_invariant_holds(&self) -> bool {
        self.log
            .iter()
            .all(|e| match &e.cmd {
                CtrlCommand::ResourceFlush { .. } => e.outstanding_at_emit == 0,
                _ => true,
            })
    }

    /// 读屏可达状态摘要。
    pub fn a11y_summary(&self) -> String {
        format!(
            "virtio 2D 内容更新：帧 {}、实发 {} 字节（全帧口径节省 {}%）、\
在途传输 {} 笔、挂起 flush {} 条、待发集 {} 资源、\
限速推迟小更新 {} 次/帧级 {} 次",
            self.bandwidth.frames,
            self.bandwidth.bytes_sent,
            self.bandwidth.clip_savings_pct(),
            self.ledger.outstanding.len(),
            self.ledger.deferred_flushes.len(),
            self.pending.len(),
            self.throttle.small_deferred,
            self.throttle.frame_deferred,
        )
    }
}
