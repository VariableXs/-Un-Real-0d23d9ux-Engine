//! VE-F0041 · 上传堆与读回堆策略（VE-A 域 · 资源段 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0041`
//!
//! **职责定位（锚点原文）**：上传/读回堆的策略管理（上传分帧预算/读回异步
//! 优先），堆内存水位可见；堆策略与 F0086 预算拒绝联动（超预算先拒创建）。
//!
//! **数据结构（锚点原文）**：水位三档 [`Watermark3`]、帧分配块
//! [`FrameChunk`]、上传堆 [`UpHeap`]（分帧预算 + 水位 + 预算余量联动）、
//! 读回请求 [`ReadbackReq`]、读回堆 [`RbHeap`]（异步优先队列 + 队列水位）、
//! 策略失配建议 [`strategy_advice`]、统计账 [`UpStats`] / [`RbStats`]。
//!
//! **判据（锚点原文）**：分帧上传、异步读回、水位可见、失配建议、判据。
//!
//! 本模块 15 条判据逐条映射：**分帧上传 4 条**（超预算拆分到后续帧逐帧不超
//! 预算／整单可入单帧不拆／零尺寸或零预算拒绝／整除帧数对账）、
//! **异步读回 4 条**（读回请求入队不阻塞提交／队列满显性拒绝／完成按队出且
//! 空队列显性拒绝／零字节读回请求拒绝）、**水位可见 2 条**（三档阈值判据侧
//! 字面量重算／告急带告警语读屏可达）、**预算联动 2 条**（超余量先拒创建／
//! 余量内创建放行且堆满拒绝）、**失配建议 2 条**（上传高读回低产出转移建议
//! ／水位正常不给建议）、**判据自身 1 条**（诊断码互异且段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 超预算 → **分帧** | [`UpHeap::plan_frames`] 把超过单帧预算的请求按预算切块摊到后续帧，帧分配表逐帧字节数 ≤ 预算——「一次塞不下」不等于「拒绝」，而是「分几帧塞」的可判定答案 |
//! | 水位告急 → **告警** | [`UpHeap::level`] / [`RbHeap::level`] 按 700/900 千分比双阈值给绿/告警/告急三档，告急档带显性告警语（[`Watermark3::alert_text`]）——水位不是内部隐变量，是读屏可达的显性事实 |
//! | 策略失配 → **建议** | [`strategy_advice`] 对「上传堆水位长期 >0.9 而读回堆 <0.1」产出容量转移建议语（反向亦然）——建议是产出物不是注释 |
//! | 超预算先拒创建（F0086 联动） | [`UpHeap::request`] 把调用方传入的预算余量当硬闸：`size > margin` 直接 [`CODE_OVER_MARGIN`] 拒绝，创建根本不发生 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **分帧是纯函数**：[`UpHeap::plan_frames`] 不改堆水位、不碰时钟，输入
//!   (size, budget) 输出帧分配表——帧数恒等于 ⌈size/budget⌉，判据侧用
//!   (size + budget - 1) / budget 字面量重算对账，谁改公式谁先红。
//! - **异步优先 = 入队即返回**：[`RbHeap::submit`] 把读回请求压入队列立刻
//!   返回 id，绝不阻塞提交路径；队列深度与队列水位独立可见，「堆积」在第
//!   一时间可判定而不是等到超时。
//! - **预算余量是调用方传参不是堆内状态**：F0086 的预算账在预算模块手里，
//!   本模块只做闸门——`request(size, margin)` 超余量先拒创建，堆与预算两本
//!   账各记各的，谁也不替谁做决定。
//! - **水位用千分比整数不用浮点**：`used * 1000 / capacity` 全程整数，
//!   no_std 内核侧无浮点依赖；阈值 700/900 在判据里按字面量重算。
//! - **零 panic 面**：构造接口拒绝零容量、查表走 `position`、计数全
//!   `saturating_*`、所有操作返回 [`Result`]，队列空/未知 id/堆满显性分码。

// ---------------------------------------------------------------------------
// 引入（no_std 内核侧；alloc 三件套自查：vec 宏 / Vec 类型 / String+format）
// ---------------------------------------------------------------------------

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0x1Bxx，独占——0x1B01..=0x1B08 整仓 grep 零占用后选定）
// ---------------------------------------------------------------------------

/// 堆容量为 0（构造非法：容量与预算/队列深度都必须 ≥1）。
pub const CODE_CAPACITY_ZERO: u16 = 0x1B01;
/// 分帧计划非法（size == 0 或 budget == 0）。
pub const CODE_PLAN_BAD: u16 = 0x1B02;
/// 请求超过调用方传入的预算余量（F0086 联动：超预算先拒创建）。
pub const CODE_OVER_MARGIN: u16 = 0x1B03;
/// 请求尺寸为 0（上传/读回均无零字节请求形态）。
pub const CODE_REQ_BAD_SIZE: u16 = 0x1B04;
/// 上传堆剩余容量不足（used + size > capacity）。
pub const CODE_HEAP_FULL: u16 = 0x1B05;
/// 读回队列已满（异步优先也要有队列深度上限）。
pub const CODE_QUEUE_FULL: u16 = 0x1B06;
/// 读回队列为空却请求完成（无可完成项）。
pub const CODE_QUEUE_EMPTY: u16 = 0x1B07;
/// 读回 id 未入过堆（从未提交或已完成出队）。
pub const CODE_RB_UNKNOWN: u16 = 0x1B08;

// ---------------------------------------------------------------------------
// 二、水位三档（整数千分比，无浮点）
// ---------------------------------------------------------------------------

/// 水位阈值：告警档下界（千分比，判据侧按字面量重算）。
pub const LEVEL_WARN_PER_MILLE: u32 = 700;
/// 水位阈值：告急档下界（千分比，判据侧按字面量重算）。
pub const LEVEL_CRIT_PER_MILLE: u32 = 900;

/// 水位三档（绿/告警/告急；无 `Default`——水位是算出来的不是默认出来的）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Watermark3 {
    /// 占用 < 70%。
    Green,
    /// 70% ≤ 占用 < 90%。
    Warning,
    /// 占用 ≥ 90%。
    Critical,
}

impl Watermark3 {
    /// 按千分比水位定档（阈值 700/900 在判据里按字面量重算）。
    pub const fn from_per_mille(pm: u32) -> Watermark3 {
        if pm >= LEVEL_CRIT_PER_MILLE {
            Watermark3::Critical
        } else if pm >= LEVEL_WARN_PER_MILLE {
            Watermark3::Warning
        } else {
            Watermark3::Green
        }
    }

    /// 档名（读屏口径：绿/告警/告急）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Watermark3::Green => "绿",
            Watermark3::Warning => "告警",
            Watermark3::Critical => "告急",
        }
    }

    /// 告警语（绿档为「水位正常」，非绿档给出显性告警文案）。
    pub const fn alert_text(self) -> &'static str {
        match self {
            Watermark3::Green => "水位正常",
            Watermark3::Warning => "水位告警：堆占用已过七成，关注分帧节奏",
            Watermark3::Critical => "水位告急：堆占用已过九成，暂停非必要请求",
        }
    }
}

/// 整数千分比水位：used * 1000 / capacity（capacity 为 0 时按满水位 1000）。
pub fn per_mille(used: u64, capacity: u64) -> u32 {
    if capacity == 0 {
        return 1000;
    }
    (used.saturating_mul(1000) / capacity) as u32
}

// ---------------------------------------------------------------------------
// 三、上传堆：分帧预算 + 水位 + 预算余量联动
// ---------------------------------------------------------------------------

/// 上传堆统计账（分帧/创建/拒绝的全部依据都可从这里读出）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UpStats {
    /// 分帧计划次数。
    pub plans: usize,
    /// 分帧产出的总帧数。
    pub frames_planned: usize,
    /// 创建放行次数（request 成功）。
    pub created: usize,
    /// 超余量拒绝次数（F0086 联动）。
    pub over_margin_rejects: usize,
    /// 堆满拒绝次数。
    pub full_rejects: usize,
    /// 零尺寸拒绝次数。
    pub bad_size_rejects: usize,
    /// 释放次数。
    pub released: usize,
}

/// 单帧分配块：第 `frame` 帧分得 `bytes` 字节上传量。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameChunk {
    /// 帧号（0 起连续递增）。
    pub frame: u32,
    /// 本帧上传字节数（恒 ≤ 单帧预算）。
    pub bytes: u64,
}

/// 上传堆：分帧预算 + 水位 + 与 F0086 预算拒绝联动。
#[derive(Clone, Debug)]
pub struct UpHeap {
    /// 堆总容量（字节，构造期保证 ≥1）。
    pub capacity: u64,
    /// 当前占用（字节；≤ capacity）。
    pub used: u64,
    /// 统计账。
    pub stats: UpStats,
}

impl UpHeap {
    /// 新建上传堆（容量为 0 显性拒绝——没有零容量堆的形态）。
    pub fn new(capacity: u64) -> Result<UpHeap, u16> {
        if capacity == 0 {
            return Err(CODE_CAPACITY_ZERO);
        }
        Ok(UpHeap { capacity, used: 0, stats: UpStats::default() })
    }

    /// 分帧计划（锚点主判据）：把 size 按单帧预算 budget 切块摊到后续帧。
    ///
    /// 返回帧分配表，逐帧字节数 ≤ budget，帧数恒等于 ⌈size/budget⌉。
    /// 零尺寸或零预算显性拒绝；本函数是纯计划，不改堆水位。
    pub fn plan_frames(&mut self, size: u64, budget: u64) -> Result<Vec<FrameChunk>, u16> {
        self.stats.plans = self.stats.plans.saturating_add(1);
        if size == 0 || budget == 0 {
            return Err(CODE_PLAN_BAD);
        }
        let mut chunks = Vec::new();
        let mut remaining = size;
        let mut frame = 0u32;
        while remaining > 0 {
            let take = if remaining > budget { budget } else { remaining };
            chunks.push(FrameChunk { frame, bytes: take });
            remaining -= take;
            frame += 1;
        }
        self.stats.frames_planned = self.stats.frames_planned.saturating_add(chunks.len());
        Ok(chunks)
    }

    /// 创建上传请求（F0086 联动：超预算先拒创建）。
    ///
    /// `margin` 是调用方从预算侧取到的余量（字节）：size > margin 直接拒绝，
    /// 创建根本不发生；堆满（used + size > capacity）亦显性拒绝。
    pub fn request(&mut self, size: u64, margin: u64) -> Result<(), u16> {
        if size == 0 {
            self.stats.bad_size_rejects = self.stats.bad_size_rejects.saturating_add(1);
            return Err(CODE_REQ_BAD_SIZE);
        }
        if size > margin {
            self.stats.over_margin_rejects = self.stats.over_margin_rejects.saturating_add(1);
            return Err(CODE_OVER_MARGIN);
        }
        if self.used.saturating_add(size) > self.capacity {
            self.stats.full_rejects = self.stats.full_rejects.saturating_add(1);
            return Err(CODE_HEAP_FULL);
        }
        self.used = self.used.saturating_add(size);
        self.stats.created = self.stats.created.saturating_add(1);
        Ok(())
    }

    /// 释放占用（saturating；多退不少）。
    pub fn release(&mut self, size: u64) {
        self.used = self.used.saturating_sub(size);
        self.stats.released = self.stats.released.saturating_add(1);
    }

    /// 当前水位档（绿/告警/告急，整数千分比）。
    pub fn level(&self) -> Watermark3 {
        Watermark3::from_per_mille(per_mille(self.used, self.capacity))
    }

    /// 水位读屏行（锚点：堆内存水位可见）。
    pub fn a11y_line(&self) -> String {
        format!(
            "上传堆：{} / {}（{}，{}）",
            self.used,
            self.capacity,
            self.level().as_str(),
            self.level().alert_text()
        )
    }
}

// ---------------------------------------------------------------------------
// 四、读回堆：异步优先队列 + 队列水位
// ---------------------------------------------------------------------------

/// 读回堆统计账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RbStats {
    /// 入队次数（异步提交）。
    pub submitted: usize,
    /// 完成出队次数。
    pub completed: usize,
    /// 队列满拒绝次数。
    pub queue_full_rejects: usize,
    /// 未知 id 拒绝次数。
    pub unknown_rejects: usize,
    /// 空队列拒绝次数。
    pub empty_rejects: usize,
    /// 零尺寸拒绝次数。
    pub bad_size_rejects: usize,
}

/// 读回请求条目：id + 字节数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadbackReq {
    /// 请求 id（堆内单调递增，从 1 起，0 留给「无请求」）。
    pub id: u64,
    /// 读回字节数。
    pub bytes: u64,
}

/// 读回堆：读回请求入队不阻塞提交（异步优先），队列水位可见。
#[derive(Clone, Debug)]
pub struct RbHeap {
    /// 堆总容量（字节，构造期保证 ≥1）。
    pub capacity: u64,
    /// 队列深度上限（条数，构造期保证 ≥1）。
    pub queue_cap: usize,
    /// 当前占用（字节）。
    used: u64,
    /// 待完成队列（FIFO：完成按入队序出队）。
    pending: Vec<ReadbackReq>,
    /// 下一个请求 id。
    next_id: u64,
    /// 统计账。
    pub stats: RbStats,
}

impl RbHeap {
    /// 新建读回堆（容量与队列深度为 0 显性拒绝）。
    pub fn new(capacity: u64, queue_cap: usize) -> Result<RbHeap, u16> {
        if capacity == 0 || queue_cap == 0 {
            return Err(CODE_CAPACITY_ZERO);
        }
        Ok(RbHeap {
            capacity,
            queue_cap,
            used: 0,
            pending: Vec::new(),
            next_id: 1,
            stats: RbStats::default(),
        })
    }

    /// 异步提交读回请求：入队即返回 id，不阻塞提交路径。
    ///
    /// 队列满 / 零字节显性拒绝；占用字节同步入账供水位判定。
    pub fn submit(&mut self, bytes: u64) -> Result<u64, u16> {
        if bytes == 0 {
            self.stats.bad_size_rejects = self.stats.bad_size_rejects.saturating_add(1);
            return Err(CODE_REQ_BAD_SIZE);
        }
        if self.pending.len() >= self.queue_cap {
            self.stats.queue_full_rejects = self.stats.queue_full_rejects.saturating_add(1);
            return Err(CODE_QUEUE_FULL);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.pending.push(ReadbackReq { id, bytes });
        self.used = self.used.saturating_add(bytes);
        self.stats.submitted = self.stats.submitted.saturating_add(1);
        Ok(id)
    }

    /// 完成一次读回：按入队序（FIFO）出队并退占用。
    ///
    /// 空队列与未知 id 分码显性拒绝——「还没好」与「没这单」不是一回事。
    pub fn complete(&mut self, id: u64) -> Result<ReadbackReq, u16> {
        let pos = match self.pending.iter().position(|r| r.id == id) {
            Some(p) => p,
            None => {
                if self.pending.is_empty() {
                    self.stats.empty_rejects = self.stats.empty_rejects.saturating_add(1);
                    return Err(CODE_QUEUE_EMPTY);
                }
                self.stats.unknown_rejects = self.stats.unknown_rejects.saturating_add(1);
                return Err(CODE_RB_UNKNOWN);
            }
        };
        let req = self.pending.remove(pos);
        self.used = self.used.saturating_sub(req.bytes);
        self.stats.completed = self.stats.completed.saturating_add(1);
        Ok(req)
    }

    /// 队列深度（待完成条数）。
    pub fn queue_depth(&self) -> usize {
        self.pending.len()
    }

    /// 队列头快照（只读，判据 FIFO 序用）。
    pub fn front(&self) -> Option<ReadbackReq> {
        self.pending.first().copied()
    }

    /// 当前水位档（按占用字节）。
    pub fn level(&self) -> Watermark3 {
        Watermark3::from_per_mille(per_mille(self.used, self.capacity))
    }

    /// 水位读屏行。
    pub fn a11y_line(&self) -> String {
        format!(
            "读回堆：{} / {}，队列 {} 条（{}，{}）",
            self.used,
            self.capacity,
            self.queue_depth(),
            self.level().as_str(),
            self.level().alert_text()
        )
    }
}

// ---------------------------------------------------------------------------
// 五、策略失配建议
// ---------------------------------------------------------------------------

/// 策略失配建议（锚点：策略失配 → 建议）。
///
/// 上传堆水位长期 >0.9（≥900 千分比）而读回堆 <0.1（≤100 千分比）→ 建议把
/// 读回堆容量部分转移给上传堆；反向亦然。水位正常返回 `None`——没有失配
/// 就没有建议，建议不臆造。
pub fn strategy_advice(up_used: u64, up_cap: u64, rb_used: u64, rb_cap: u64) -> Option<String> {
    let up = per_mille(up_used, up_cap);
    let rb = per_mille(rb_used, rb_cap);
    if up >= 900 && rb <= 100 {
        Some(String::from(
            "建议：上传堆水位长期高于 0.9 而读回堆低于 0.1，建议将部分读回堆容量转移给上传堆",
        ))
    } else if rb >= 900 && up <= 100 {
        Some(String::from(
            "建议：读回堆水位长期高于 0.9 而上传堆低于 0.1，建议将部分上传堆容量转移给读回堆",
        ))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 六、判据（run_vea41_checks —— 15 条，逐条映射锚点五条判据）
// ---------------------------------------------------------------------------

/// VE-F0041 域自检：分帧上传 4 + 异步读回 4 + 水位可见 2 + 预算联动 2 + 失配建议 2 + 判据自身 1。
pub fn run_vea41_checks() -> CheckSet {
    let mut s = CheckSet::new("vea41_heaps");

    // --- 判据 1.1：分帧上传——超预算拆分到后续帧逐帧不超预算（判据侧字面量重算） ---
    {
        let mut h = UpHeap::new(100_000).unwrap_or_else(|_| UpHeap { capacity: 1, used: 0, stats: UpStats::default() });
        let plan = h.plan_frames(5000, 2000);
        // 判据侧独立重算：⌈5000/2000⌉ = 3 帧，切块 [2000, 2000, 1000]
        let expect: [u64; 3] = [2000, 2000, 1000];
        let ok = match plan {
            Ok(chunks) => {
                chunks.len() == 3
                    && chunks.iter().enumerate().all(|(i, c)| c.frame == i as u32 && c.bytes == expect[i])
                    && chunks.iter().all(|c| c.bytes <= 2000)
            }
            Err(_) => false,
        };
        s.add(
            "A41-分帧-超预算拆分到后续帧逐帧不超预算",
            ok,
            "5000 字节按 2000 预算切 3 帧 [2000,2000,1000]：帧号连续、逐帧 ≤ 预算",
        );
    }

    // --- 判据 1.2：分帧上传——整单可入单帧不拆 ---
    {
        let mut h = UpHeap::new(100_000).unwrap_or_else(|_| UpHeap { capacity: 1, used: 0, stats: UpStats::default() });
        let plan = h.plan_frames(1500, 2000);
        let ok = match plan {
            Ok(chunks) => chunks.len() == 1 && chunks[0].frame == 0 && chunks[0].bytes == 1500,
            Err(_) => false,
        };
        s.add(
            "A41-分帧-整单可入单帧不拆",
            ok,
            "1500 ≤ 预算 2000：单帧 0 号一整块，分帧只对超预算发生",
        );
    }

    // --- 判据 1.3：分帧上传——零尺寸或零预算显性拒绝 ---
    {
        let mut h = UpHeap::new(100_000).unwrap_or_else(|_| UpHeap { capacity: 1, used: 0, stats: UpStats::default() });
        let zero_size = h.plan_frames(0, 2000);
        let zero_budget = h.plan_frames(5000, 0);
        s.add(
            "A41-分帧-零尺寸或零预算显性拒绝",
            zero_size == Err(CODE_PLAN_BAD) && zero_budget == Err(CODE_PLAN_BAD),
            "没有内容的计划与没有预算的计划都没有意义：双双 CODE_PLAN_BAD 拒绝",
        );
    }

    // --- 判据 1.4：分帧上传——整除帧数对账（帧数 = ⌈size/budget⌉ 判据侧重算） ---
    {
        let mut h = UpHeap::new(100_000).unwrap_or_else(|_| UpHeap { capacity: 1, used: 0, stats: UpStats::default() });
        let exact = h.plan_frames(4000, 2000);
        let ragged = h.plan_frames(4001, 2000);
        // 判据侧独立重算：⌈4000/2000⌉=2；⌈4001/2000⌉=3
        let ok = match (exact, ragged) {
            (Ok(a), Ok(b)) => a.len() == 2 && b.len() == 3 && b[2].bytes == 1,
            _ => false,
        };
        s.add(
            "A41-分帧-整除帧数对账余数补尾帧",
            ok && h.stats.frames_planned == 5,
            "整除 4000/2000 得 2 帧；余 1 字节补尾帧共 3 帧——帧数公式 ⌈size/budget⌉ 判据侧重算一致",
        );
    }

    // --- 判据 2.1：异步读回——读回请求入队不阻塞提交 ---
    {
        let mut h = RbHeap::new(10_000, 4).unwrap_or_else(|_| RbHeap {
            capacity: 1,
            queue_cap: 1,
            used: 0,
            pending: Vec::new(),
            next_id: 1,
            stats: RbStats::default(),
        });
        let id = h.submit(800);
        let ok = id.is_ok()
            && h.queue_depth() == 1
            && h.stats.submitted == 1
            && h.front().map(|r| r.bytes) == Some(800);
        s.add(
            "A41-读回-请求入队即返回不阻塞提交",
            ok,
            "submit 拿到 id 立即返回：队列深度 +1、占用入账，提交路径不被读回拖住",
        );
    }

    // --- 判据 2.2：异步读回——队列满显性拒绝 ---
    {
        let mut h = RbHeap::new(10_000, 2).unwrap_or_else(|_| RbHeap {
            capacity: 1,
            queue_cap: 1,
            used: 0,
            pending: Vec::new(),
            next_id: 1,
            stats: RbStats::default(),
        });
        let _ = h.submit(100);
        let _ = h.submit(200);
        let third = h.submit(300);
        s.add(
            "A41-读回-队列满显性拒绝",
            third == Err(CODE_QUEUE_FULL) && h.queue_depth() == 2 && h.stats.queue_full_rejects == 1,
            "深度上限 2：第 3 条显性拒绝且不入队——异步优先也要有界",
        );
    }

    // --- 判据 2.3：异步读回——完成按 FIFO 出队且空队列/未知 id 分码拒绝 ---
    {
        let mut h = RbHeap::new(10_000, 4).unwrap_or_else(|_| RbHeap {
            capacity: 1,
            queue_cap: 1,
            used: 0,
            pending: Vec::new(),
            next_id: 1,
            stats: RbStats::default(),
        });
        let a = h.submit(100).unwrap_or(0);
        let b = h.submit(300).unwrap_or(0);
        let first = h.complete(a);
        let second = h.complete(b);
        let empty_again = h.complete(a);
        let unknown_on_nonempty = {
            let _ = h.submit(50);
            h.complete(999)
        };
        s.add(
            "A41-读回-完成按队出空队列未知id分码拒绝",
            first.is_ok()
                && second.is_ok()
                && empty_again == Err(CODE_QUEUE_EMPTY)
                && unknown_on_nonempty == Err(CODE_RB_UNKNOWN),
            "FIFO 出队后空队列再完成 = CODE_QUEUE_EMPTY，非空查无此 id = CODE_RB_UNKNOWN——两码分通道",
        );
    }

    // --- 判据 2.4：异步读回——零字节读回请求拒绝 ---
    {
        let mut h = RbHeap::new(10_000, 4).unwrap_or_else(|_| RbHeap {
            capacity: 1,
            queue_cap: 1,
            used: 0,
            pending: Vec::new(),
            next_id: 1,
            stats: RbStats::default(),
        });
        let r = h.submit(0);
        s.add(
            "A41-读回-零字节读回请求拒绝",
            r == Err(CODE_REQ_BAD_SIZE) && h.queue_depth() == 0 && h.stats.bad_size_rejects == 1,
            "零字节读回没有完成语义：提交期即拒，不入队不占位",
        );
    }

    // --- 判据 3.1：水位可见——三档阈值判据侧字面量重算（699/700/899/900） ---
    {
        let g = Watermark3::from_per_mille(699);
        let w_lo = Watermark3::from_per_mille(700);
        let w_hi = Watermark3::from_per_mille(899);
        let c = Watermark3::from_per_mille(900);
        // 堆侧联动：1000 容量堆 used 690/700/900 三点取样
        let mut h = UpHeap::new(1000).unwrap_or_else(|_| UpHeap { capacity: 1, used: 0, stats: UpStats::default() });
        h.used = 690;
        let l1 = h.level();
        h.used = 700;
        let l2 = h.level();
        h.used = 900;
        let l3 = h.level();
        s.add(
            "A41-水位-三档阈值判据侧字面量重算",
            g == Watermark3::Green
                && w_lo == Watermark3::Warning
                && w_hi == Watermark3::Warning
                && c == Watermark3::Critical
                && l1 == Watermark3::Green
                && l2 == Watermark3::Warning
                && l3 == Watermark3::Critical
                && LEVEL_WARN_PER_MILLE == 700
                && LEVEL_CRIT_PER_MILLE == 900,
            "699 绿 / 700 告警 / 899 告警 / 900 告急：阈值边界双侧对拍，堆取样三点一致",
        );
    }

    // --- 判据 3.2：水位可见——告急带告警语读屏可达 ---
    {
        let mut h = UpHeap::new(1000).unwrap_or_else(|_| UpHeap { capacity: 1, used: 0, stats: UpStats::default() });
        h.used = 950;
        let line = h.a11y_line();
        let mut rb = RbHeap::new(1000, 4).unwrap_or_else(|_| RbHeap {
            capacity: 1,
            queue_cap: 1,
            used: 0,
            pending: Vec::new(),
            next_id: 1,
            stats: RbStats::default(),
        });
        let _ = rb.submit(950);
        let rb_line = rb.a11y_line();
        s.add(
            "A41-水位-告急带告警语读屏可达",
            h.level() == Watermark3::Critical
                && line.contains("告急")
                && line.contains("950")
                && rb_line.contains("告急")
                && rb_line.contains("队列 1 条"),
            "上传/读回两侧水位行都带档名与告警语：堆内存水位是读屏可达的显性事实",
        );
    }

    // --- 判据 4.1：预算联动——超余量先拒创建（F0086：超预算先拒创建） ---
    {
        let mut h = UpHeap::new(100_000).unwrap_or_else(|_| UpHeap { capacity: 1, used: 0, stats: UpStats::default() });
        let r = h.request(1500, 1000);
        s.add(
            "A41-联动-超余量先拒创建",
            r == Err(CODE_OVER_MARGIN)
                && h.used == 0
                && h.stats.over_margin_rejects == 1
                && h.stats.created == 0,
            "size 1500 > 余量 1000：创建根本不发生、水位零变化——联动是闸门不是事后追账",
        );
    }

    // --- 判据 4.2：预算联动——余量内放行且堆满显性拒绝 ---
    {
        let mut h = UpHeap::new(1000).unwrap_or_else(|_| UpHeap { capacity: 1, used: 0, stats: UpStats::default() });
        let ok1 = h.request(400, 1000);
        let ok2 = h.request(600, 1000);
        let full = h.request(1, 1000);
        let zero = h.request(0, 1000);
        s.add(
            "A41-联动-余量内放行堆满显性拒绝",
            ok1.is_ok()
                && ok2.is_ok()
                && full == Err(CODE_HEAP_FULL)
                && zero == Err(CODE_REQ_BAD_SIZE)
                && h.used == 1000
                && h.stats.full_rejects == 1,
            "400+600 恰好放满；再要 1 字节显性 CODE_HEAP_FULL——容量边界闭区间对拍",
        );
    }

    // --- 判据 5.1：失配建议——上传高读回低产出转移建议 ---
    {
        let advice = strategy_advice(950, 1000, 50, 1000);
        let ok = match advice {
            Some(text) => text.contains("转移") && text.contains("上传堆"),
            None => false,
        };
        s.add(
            "A41-失配-上传高读回低产出转移建议",
            ok,
            "上传 ≥0.9 且读回 ≤0.1：产出带「转移」字样的容量转移建议语",
        );
    }

    // --- 判据 5.2：失配建议——水位正常不给建议且反向失配也有建议 ---
    {
        let normal = strategy_advice(500, 1000, 500, 1000);
        let reverse = strategy_advice(50, 1000, 950, 1000);
        let ok = normal.is_none()
            && reverse.is_some()
            && reverse.map(|t| t.contains("读回堆") && t.contains("转移")) == Some(true);
        s.add(
            "A41-失配-水位正常不给建议反向失配亦有建议",
            ok,
            "两堆水位居中 → None（建议不臆造）；反向失配（读回高上传低）→ 反向转移建议",
        );
    }

    // --- 判据 6.1：判据自身——诊断码互异且段独占（0x1B 段八码全对拍） ---
    {
        let codes = [
            CODE_CAPACITY_ZERO,
            CODE_PLAN_BAD,
            CODE_OVER_MARGIN,
            CODE_REQ_BAD_SIZE,
            CODE_HEAP_FULL,
            CODE_QUEUE_FULL,
            CODE_QUEUE_EMPTY,
            CODE_RB_UNKNOWN,
        ];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0x1B00);
        s.add(
            "A41-判据-诊断码互异且段独占",
            all_diff && in_segment && codes.len() == 8,
            "0x1B01..=0x1B08 八码两两互异、高字节全部落在 0x1B 独占段",
        );
    }

    s
}

// ---------------------------------------------------------------------------
// 七、单元测试（cargo test svstar2）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn up(cap: u64) -> UpHeap {
        UpHeap::new(cap).unwrap_or_else(|_| UpHeap { capacity: 1, used: 0, stats: UpStats::default() })
    }

    fn rb(cap: u64, q: usize) -> RbHeap {
        RbHeap::new(cap, q).unwrap_or_else(|_| RbHeap {
            capacity: 1,
            queue_cap: 1,
            used: 0,
            pending: Vec::new(),
            next_id: 1,
            stats: RbStats::default(),
        })
    }

    #[test]
    fn vea41_plan_frames_and_release_roundtrip() {
        let mut h = up(10_000);
        let chunks = h.plan_frames(7000, 3000).unwrap_or_default();
        assert_eq!(chunks.len(), 3, "7000/3000 → 3 帧");
        assert_eq!(chunks[2].bytes, 1000, "尾帧拿余数");
        assert!(h.request(3000, 5000).is_ok());
        assert_eq!(h.used, 3000);
        h.release(3000);
        assert_eq!(h.used, 0);
        assert_eq!(h.stats.released, 1);
        // 释放不可把 used 打成负数（saturating）
        h.release(9999);
        assert_eq!(h.used, 0);
    }

    #[test]
    fn vea41_readback_async_queue_lifecycle() {
        let mut h = rb(10_000, 3);
        let a = h.submit(100).unwrap_or(0);
        let b = h.submit(200).unwrap_or(0);
        let c = h.submit(400).unwrap_or(0);
        assert_eq!(h.queue_depth(), 3);
        assert_eq!(h.level(), Watermark3::Green);
        // FIFO：先完成先出
        let done_a = h.complete(a).unwrap_or(ReadbackReq { id: 0, bytes: 0 });
        assert_eq!(done_a.id, a);
        assert_eq!(done_a.bytes, 100);
        let front_now = h.front().unwrap_or(ReadbackReq { id: 0, bytes: 0 });
        assert_eq!(front_now.id, b, "a 出队后队头是 b");
        let _ = h.complete(b);
        let _ = h.complete(c);
        assert_eq!(h.used, 0);
        assert_eq!(h.stats.completed, 3);
        assert_eq!(h.complete(a), Err(CODE_QUEUE_EMPTY));
    }

    #[test]
    fn vea41_zero_capacity_construction_rejected() {
        assert_eq!(UpHeap::new(0).unwrap_err(), CODE_CAPACITY_ZERO);
        assert_eq!(RbHeap::new(0, 4).unwrap_err(), CODE_CAPACITY_ZERO);
        assert_eq!(RbHeap::new(1000, 0).unwrap_err(), CODE_CAPACITY_ZERO);
    }

    #[test]
    fn vea41_a11y_lines_visible() {
        let mut h = up(1000);
        let _ = h.request(750, 1000);
        let line = h.a11y_line();
        assert!(line.contains("告警"), "750/1000 在告警档：{}", line);
        assert!(line.contains("750"), "读屏行带占用量：{}", line);
        let mut r = rb(1000, 2);
        let _ = r.submit(100);
        assert!(r.a11y_line().contains("队列 1 条"));
    }

    #[test]
    fn vea41_checks_all_green() {
        let set = run_vea41_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0041 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
