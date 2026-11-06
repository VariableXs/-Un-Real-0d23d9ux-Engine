//! UNX-F48845 MT-slot 多点触控解析与 5 触点并发 · 0x70580
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   F48845 ① 纯功能代码 ≥380 行；
//!          ② UNX-F48845-J1 5 触点并发 tracking_id ×100 组全程不混淆；
//!   F48846 ① 纯功能代码 ≥340 行；
//!          ② UNX-F48846-J1 生命周期 ×10^2 悬空句柄全拒；
//!   F48847 ① 纯功能代码 ≥300 行；
//!          ② UNX-F48847-J1 全轴 ×10 设备量纲表一致。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：事件码白名单 → 槽位界内 → tracking 状态 → 轴值量程 →
//!           长短轴次序 → 并发上限 → 样本完备 → 释放语义。
//!   次序不可倒：先查 tracking 再验槽位，会让越界槽把 tracking 表污染；
//!   先做量纲归一后验量程，溢出值会被静默截断成合法值；并发上限必须
//!   放在样本完备之后，否则「空槽被当作第 6 触点」也会触发误拒。
//! 关键差异：unx48843 只认「设备是 A 还是 B」和槽位簿记；本模块消费
//!   它的协议判定与 slot 表语义，落脚点是「一帧原始 AXIS 事件 → 触点
//!   样本」。轨迹窗口归 unx48848，手势归 B10；本模块绝不缓存历史，
//!   也绝不向调用方返回未量纲化的原始轴值。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70580，VX6 批 B03 第三槽）。
pub const SEG_BASE: u32 = 0x70580;

/// 解析期槽位上限（与 unx48843 的 10 槽语义对齐）。
pub const MAX_SLOT: usize = 10;
/// 并发触点上上限（判据：5 触点并发）。
pub const MAX_TOUCH: usize = 5;
/// 量纲满刻度（压力/长短轴统一归一到 0..=1000）。
pub const SCALE_FULL: u16 = 1000;
/// 原始轴值硬上限（量化前先拒，防 u32 乘法溢出）。
pub const RAW_MAX: u32 = 0xFFFF;
/// tracking 提升哨兵（Linux 语义：ABS_MT_TRACKING_ID = -1）。
pub const TRACK_LIFT: i32 = -1;
/// 生命周期判据轮数。
pub const LIFE_ROUNDS: u32 = 100;
/// 并发判据组数。
pub const TOUCH_GROUPS: u32 = 100;

// ── 诊断码闭集（+0x01..+0x0D） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TgCode {
    /// 未知事件码：不属于 MT 轴码闭集
    EvBad,
    /// 同一帧内同一轴重复出现
    EvDup,
    /// 槽位越界
    SlotBad,
    /// tracking 缺失或非法
    TrackBad,
    /// tracking 重复：同一 id 被两个槽同时占用
    TrackDup,
    /// 悬空句柄：生命周期已释放或代际过期
    TrackDangling,
    /// 轴值越过设备量程
    AxisBad,
    /// 长短轴次序颠倒（major < minor）
    AxisOrder,
    /// 接触点缺失（无 X 或 Y 却未抬起）
    PosBad,
    /// 并发超过 5 触点
    ConcurrencyBad,
    /// 帧为空
    FrameEmpty,
    /// 复算不一致
    ReplayBad,
    /// 段位越界
    SegmentBad,
}

impl TgCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            TgCode::EvBad => "未知事件码：不属于 ABS_MT 七轴闭集",
            TgCode::EvDup => "同帧同轴重复：一帧内同一码出现两次",
            TgCode::SlotBad => "槽位越界：只允许 0..=9",
            TgCode::TrackBad => "tracking 缺失或非法：活触点必须有 0..=0xFFFE 的 id",
            TgCode::TrackDup => "tracking 重复：同一 id 被两个槽同时占用",
            TgCode::TrackDangling => "悬空句柄：生命周期已释放或代际过期",
            TgCode::AxisBad => "轴值越界：原始值超过设备量程",
            TgCode::AxisOrder => "长短轴次序颠倒：major < minor",
            TgCode::PosBad => "接触点缺失：无 X/Y 却未声明抬起",
            TgCode::ConcurrencyBad => "并发超限：同时活触点只允许 5 个",
            TgCode::FrameEmpty => "空帧：没有任何 MT 轴事件",
            TgCode::ReplayBad => "复算不一致：两次解析结果不同",
            TgCode::SegmentBad => "段位越界：诊断码逃出 0x70580 窗口",
        }
    }

    pub const ALL: [TgCode; 13] = [
        TgCode::EvBad,
        TgCode::EvDup,
        TgCode::SlotBad,
        TgCode::TrackBad,
        TgCode::TrackDup,
        TgCode::TrackDangling,
        TgCode::AxisBad,
        TgCode::AxisOrder,
        TgCode::PosBad,
        TgCode::ConcurrencyBad,
        TgCode::FrameEmpty,
        TgCode::ReplayBad,
        TgCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TgCode::ALL.len() {
        if TgCode::ALL[i].val() == code {
            return TgCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x70580 段闭集内"
}

// ── 轴码闭集 ──────────────────────────────────────────────────────
pub const ABS_MT_SLOT: u16 = 0x2F;
pub const ABS_MT_TOUCH_MAJOR: u16 = 0x30;
pub const ABS_MT_TOUCH_MINOR: u16 = 0x31;
pub const ABS_MT_POSITION_X: u16 = 0x35;
pub const ABS_MT_POSITION_Y: u16 = 0x36;
pub const ABS_MT_TRACKING_ID: u16 = 0x39;
pub const ABS_MT_PRESSURE: u16 = 0x3A;

/// 七轴闭集判定。
pub fn is_mt_code(code: u16) -> bool {
    code == ABS_MT_SLOT
        || code == ABS_MT_TOUCH_MAJOR
        || code == ABS_MT_TOUCH_MINOR
        || code == ABS_MT_POSITION_X
        || code == ABS_MT_POSITION_Y
        || code == ABS_MT_TRACKING_ID
        || code == ABS_MT_PRESSURE
}

/// 原始事件（与事件层 input_event 的 code/value 对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MtEvent {
    pub code: u16,
    pub value: i32,
}

impl MtEvent {
    pub fn new(code: u16, value: i32) -> MtEvent {
        MtEvent { code, value }
    }

    pub fn text(&self) -> String {
        format!("abs 0x{:02X}={}", self.code, self.value)
    }
}

// ── 量纲表（10 台设备） ───────────────────────────────────────────
/// 单台设备的量纲：X/Y 像素量程 + 三枚语义轴原始满量程。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AxisSpec {
    pub x_max: u32,
    pub y_max: u32,
    pub pressure_max: u32,
    pub major_max: u32,
    pub minor_max: u32,
}

impl AxisSpec {
    pub fn new(x_max: u32, y_max: u32, pressure_max: u32, major_max: u32, minor_max: u32) -> AxisSpec {
        AxisSpec { x_max, y_max, pressure_max, major_max, minor_max }
    }

    /// 量纲表自洽：五维非零且不超过 RAW_MAX。
    pub fn valid(&self) -> bool {
        self.x_max > 0
            && self.y_max > 0
            && self.pressure_max > 0
            && self.major_max > 0
            && self.minor_max > 0
            && self.x_max <= RAW_MAX
            && self.y_max <= RAW_MAX
            && self.pressure_max <= RAW_MAX
            && self.major_max <= RAW_MAX
            && self.minor_max <= RAW_MAX
    }

    pub fn text(&self) -> String {
        format!(
            "x{} y{} p{} M{} m{}",
            self.x_max, self.y_max, self.pressure_max, self.major_max, self.minor_max
        )
    }
}

/// 十设备量纲表（与 unx48843 的十设备语料一一对应）。
pub const AXIS_SPECS: [AxisSpec; 10] = [
    AxisSpec { x_max: 4096, y_max: 4096, pressure_max: 255, major_max: 255, minor_max: 255 },
    AxisSpec { x_max: 4096, y_max: 4096, pressure_max: 511, major_max: 511, minor_max: 511 },
    AxisSpec { x_max: 2048, y_max: 1536, pressure_max: 255, major_max: 255, minor_max: 255 },
    AxisSpec { x_max: 2048, y_max: 1536, pressure_max: 1023, major_max: 1023, minor_max: 1023 },
    AxisSpec { x_max: 1024, y_max: 768, pressure_max: 255, major_max: 255, minor_max: 255 },
    AxisSpec { x_max: 8192, y_max: 8192, pressure_max: 4095, major_max: 4095, minor_max: 4095 },
    AxisSpec { x_max: 8192, y_max: 8192, pressure_max: 2047, major_max: 2047, minor_max: 2047 },
    AxisSpec { x_max: 6144, y_max: 4096, pressure_max: 1023, major_max: 1023, minor_max: 1023 },
    AxisSpec { x_max: 6144, y_max: 4096, pressure_max: 511, major_max: 511, minor_max: 511 },
    AxisSpec { x_max: 1280, y_max: 720, pressure_max: 255, major_max: 255, minor_max: 255 },
];

/// 量纲归一：0 → 0，满量程 → 1000，其余四舍五入。
///
/// 先验量程再乘，乘法上限 65535 × 1000 < 2^32，无溢出。
pub fn quantize(value: u32, max: u32) -> Result<u16, u32> {
    if max == 0 || max > RAW_MAX || value > max {
        return Err(TgCode::AxisBad.val());
    }
    let scaled = value.saturating_mul(SCALE_FULL as u32);
    let rounded = scaled.saturating_add(max / 2);
    let out = rounded / max;
    if out > SCALE_FULL as u32 {
        return Err(TgCode::AxisBad.val());
    }
    Ok(out as u16)
}

/// 量纲表审计：十行全自洽，且端点/中点量化可复算。
pub fn audit_axis_table() -> Result<usize, u32> {
    let mut i = 0usize;
    while i < AXIS_SPECS.len() {
        let s = match AXIS_SPECS.get(i) {
            Some(v) => *v,
            None => return Err(TgCode::SegmentBad.val()),
        };
        if !s.valid() {
            return Err(TgCode::AxisBad.val());
        }
        if quantize(s.pressure_max, s.pressure_max) != Ok(SCALE_FULL) {
            return Err(TgCode::AxisBad.val());
        }
        if quantize(0, s.pressure_max) != Ok(0) {
            return Err(TgCode::AxisBad.val());
        }
        let mid = quantize(s.major_max / 2, s.major_max)?;
        if mid < 498 || mid > 501 {
            return Err(TgCode::AxisBad.val());
        }
        i += 1;
    }
    Ok(AXIS_SPECS.len())
}

// ── tracking 生命周期（分配/存续/释放三态 + 代际） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrackState {
    /// 已分配未上报坐标
    Allocated,
    /// 存续：至少一帧坐标已上报
    Live,
    /// 已释放
    Released,
}

impl TrackState {
    pub fn text(self) -> &'static str {
        match self {
            TrackState::Allocated => "allocated",
            TrackState::Live => "live",
            TrackState::Released => "released",
        }
    }
}

/// 生命周期句柄：id + 代际。释放后同 id 可复用，但代际必变。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TrackHandle {
    pub id: u16,
    pub gen: u32,
}

impl TrackHandle {
    pub fn new(id: u16, gen: u32) -> TrackHandle {
        TrackHandle { id, gen }
    }

    pub fn text(&self) -> String {
        format!("track#{}:{}", self.id, self.gen)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TrackEntry {
    pub id: u16,
    pub state: TrackState,
    pub gen: u32,
}

pub struct TrackBook {
    entries: Vec<TrackEntry>,
    allocs: u32,
    engages: u32,
    releases: u32,
    dangling: u32,
    dup: u32,
}

impl TrackBook {
    pub fn new() -> TrackBook {
        TrackBook {
            entries: Vec::new(),
            allocs: 0,
            engages: 0,
            releases: 0,
            dangling: 0,
            dup: 0,
        }
    }

    pub fn allocs(&self) -> u32 {
        self.allocs
    }

    pub fn engages(&self) -> u32 {
        self.engages
    }

    pub fn releases(&self) -> u32 {
        self.releases
    }

    pub fn dangling(&self) -> u32 {
        self.dangling
    }

    pub fn dup(&self) -> u32 {
        self.dup
    }

    pub fn live_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.state != TrackState::Released {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    fn find(&self, id: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 分配：同一 id 仍有未释放条目 → TrackDup；已释放条目 → 代际 +1 复用。
    pub fn allocate(&mut self, id: u16) -> Result<TrackHandle, u32> {
        if id == u16::MAX {
            return Err(TgCode::TrackBad.val());
        }
        if let Some(idx) = self.find(id) {
            let cur = match self.entries.get(idx) {
                Some(e) => *e,
                None => return Err(TgCode::SegmentBad.val()),
            };
            if cur.state != TrackState::Released {
                self.dup = self.dup.saturating_add(1);
                return Err(TgCode::TrackDup.val());
            }
            let next_gen = match cur.gen.checked_add(1) {
                Some(v) => v,
                None => return Err(TgCode::TrackBad.val()),
            };
            if let Some(slot) = self.entries.get_mut(idx) {
                slot.state = TrackState::Allocated;
                slot.gen = next_gen;
            }
            self.allocs = self.allocs.saturating_add(1);
            return Ok(TrackHandle::new(id, next_gen));
        }
        self.entries.push(TrackEntry { id, state: TrackState::Allocated, gen: 1 });
        self.allocs = self.allocs.saturating_add(1);
        Ok(TrackHandle::new(id, 1))
    }

    /// 存续：Allocated → Live。已 Live 重复上报不算失败（幂等）。
    pub fn engage(&mut self, h: TrackHandle) -> Result<(), u32> {
        let idx = match self.find(h.id) {
            Some(i) => i,
            None => {
                self.dangling = self.dangling.saturating_add(1);
                return Err(TgCode::TrackDangling.val());
            }
        };
        let cur = match self.entries.get(idx) {
            Some(e) => *e,
            None => return Err(TgCode::SegmentBad.val()),
        };
        if cur.gen != h.gen || cur.state == TrackState::Released {
            self.dangling = self.dangling.saturating_add(1);
            return Err(TgCode::TrackDangling.val());
        }
        if let Some(e) = self.entries.get_mut(idx) {
            e.state = TrackState::Live;
        }
        self.engages = self.engages.saturating_add(1);
        Ok(())
    }

    /// 释放：Allocated/Live → Released。已释放或代际不符 → TrackDangling。
    pub fn release(&mut self, h: TrackHandle) -> Result<(), u32> {
        let idx = match self.find(h.id) {
            Some(i) => i,
            None => {
                self.dangling = self.dangling.saturating_add(1);
                return Err(TgCode::TrackDangling.val());
            }
        };
        let cur = match self.entries.get(idx) {
            Some(e) => *e,
            None => return Err(TgCode::SegmentBad.val()),
        };
        if cur.gen != h.gen || cur.state == TrackState::Released {
            self.dangling = self.dangling.saturating_add(1);
            return Err(TgCode::TrackDangling.val());
        }
        if let Some(e) = self.entries.get_mut(idx) {
            e.state = TrackState::Released;
        }
        self.releases = self.releases.saturating_add(1);
        Ok(())
    }

    pub fn state_of(&self, id: u16) -> Result<TrackState, u32> {
        match self.find(id) {
            Some(i) => match self.entries.get(i) {
                Some(e) => Ok(e.state),
                None => Err(TgCode::SegmentBad.val()),
            },
            None => Err(TgCode::TrackBad.val()),
        }
    }

    /// 对账：分配数 = 释放数 + 未释放条目数。
    pub fn audit(&self) -> Result<(), u32> {
        let live = self.live_count() as u32;
        if self.allocs != self.releases.saturating_add(live) {
            return Err(TgCode::TrackDangling.val());
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = fold_word(h, e.id as u32);
                h = fold_word(h, e.gen);
                h = fold_byte(
                    h,
                    match e.state {
                        TrackState::Allocated => 1,
                        TrackState::Live => 2,
                        TrackState::Released => 3,
                    },
                );
            }
            i += 1;
        }
        h = fold_word(h, self.allocs);
        h = fold_word(h, self.engages);
        h = fold_word(h, self.releases);
        fold_word(h, self.dangling)
    }
}

// ── FNV-1a（本域基础） ────────────────────────────────────────────
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_word(h: u64, w: u32) -> u64 {
    let mut x = h;
    x = fold_byte(x, (w & 0xFF) as u8);
    x = fold_byte(x, ((w >> 8) & 0xFF) as u8);
    x = fold_byte(x, ((w >> 16) & 0xFF) as u8);
    fold_byte(x, ((w >> 24) & 0xFF) as u8)
}

pub fn fold_str(h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut x = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            x = fold_byte(x, *b);
        }
        i += 1;
    }
    x
}

// ── 单槽累加器 ────────────────────────────────────────────────────
#[derive(Clone, Copy)]
struct Acc {
    seen: bool,
    lifted: bool,
    tracking: Option<u16>,
    x: Option<u32>,
    y: Option<u32>,
    pressure: Option<u32>,
    major: Option<u32>,
    minor: Option<u32>,
}

impl Acc {
    fn new() -> Acc {
        Acc {
            seen: false,
            lifted: false,
            tracking: None,
            x: None,
            y: None,
            pressure: None,
            major: None,
            minor: None,
        }
    }
}

fn put_u32(slot: &mut Option<u32>, v: u32) -> Result<(), u32> {
    if slot.is_some() {
        return Err(TgCode::EvDup.val());
    }
    *slot = Some(v);
    Ok(())
}

// ── 触点样本 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TouchSample {
    pub slot: u8,
    pub tracking: u16,
    /// 抬起帧：坐标字段保留最后已知值（未上报则为 0）。
    pub lifted: bool,
    pub x_norm: u16,
    pub y_norm: u16,
    pub pressure: u16,
    pub major: u16,
    pub minor: u16,
}

impl TouchSample {
    pub fn text(&self) -> String {
        format!(
            "slot{} track{} {} norm=({}, {}) p={} M={} m={}",
            self.slot,
            self.tracking,
            if self.lifted { "LIFT" } else { "MOVE" },
            self.x_norm,
            self.y_norm,
            self.pressure,
            self.major,
            self.minor
        )
    }
}

fn value_to_raw(v: i32) -> Result<u32, u32> {
    if v < 0 {
        return Err(TgCode::AxisBad.val());
    }
    Ok(v as u32)
}

fn check_range(raw: u32, max: u32) -> Result<u32, u32> {
    if raw > max {
        return Err(TgCode::AxisBad.val());
    }
    Ok(raw)
}

// ── 帧解析（协议 B / 协议 A 双路） ────────────────────────────────
/// 协议 B：ABS_MT_SLOT 选槽；协议 ADisc 由调用方显式声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameProto {
    A,
    B,
}

/// 解析一帧。返回的样本顺序按槽位升序，释放帧也在其中。
///
/// 判据要点：同一帧里 tracking 号不得跨槽重复；活触点数不得超过 5；
/// 任何未抬起的触点都必须同时有 X 与 Y。
pub fn parse_frame(
    proto: FrameProto,
    spec: &AxisSpec,
    events: &[MtEvent],
) -> Result<Vec<TouchSample>, u32> {
    if !spec.valid() {
        return Err(TgCode::AxisBad.val());
    }
    if events.is_empty() {
        return Err(TgCode::FrameEmpty.val());
    }
    let mut accs = [Acc::new(); MAX_SLOT];
    let mut cur = 0usize;
    let mut any = false;
    let mut i = 0usize;
    while i < events.len() {
        let ev = match events.get(i) {
            Some(v) => *v,
            None => return Err(TgCode::SegmentBad.val()),
        };
        if !is_mt_code(ev.code) {
            return Err(TgCode::EvBad.val());
        }
        if proto == FrameProto::A && ev.code == ABS_MT_SLOT {
            // 协议 A 不允许携带 SLOT：出现即是跨协议串路。
            return Err(TgCode::EvBad.val());
        }
        if ev.code == ABS_MT_SLOT {
            let raw = value_to_raw(ev.value)?;
            if raw >= MAX_SLOT as u32 {
                return Err(TgCode::SlotBad.val());
            }
            cur = raw as usize;
            i += 1;
            continue;
        }
        let acc = match accs.get_mut(cur) {
            Some(a) => a,
            None => return Err(TgCode::SlotBad.val()),
        };
        acc.seen = true;
        any = true;
        match ev.code {
            ABS_MT_TRACKING_ID => {
                if ev.value == TRACK_LIFT {
                    acc.lifted = true;
                } else {
                    let raw = value_to_raw(ev.value)?;
                    if raw > 0xFFFE {
                        return Err(TgCode::TrackBad.val());
                    }
                    if acc.tracking.is_some() {
                        return Err(TgCode::EvDup.val());
                    }
                    acc.tracking = Some(raw as u16);
                }
            }
            ABS_MT_POSITION_X => {
                let raw = value_to_raw(ev.value)?;
                let ok = check_range(raw, spec.x_max)?;
                put_u32(&mut acc.x, ok)?;
            }
            ABS_MT_POSITION_Y => {
                let raw = value_to_raw(ev.value)?;
                let ok = check_range(raw, spec.y_max)?;
                put_u32(&mut acc.y, ok)?;
            }
            ABS_MT_PRESSURE => {
                let raw = value_to_raw(ev.value)?;
                let ok = check_range(raw, spec.pressure_max)?;
                put_u32(&mut acc.pressure, ok)?;
            }
            ABS_MT_TOUCH_MAJOR => {
                let raw = value_to_raw(ev.value)?;
                let ok = check_range(raw, spec.major_max)?;
                put_u32(&mut acc.major, ok)?;
            }
            ABS_MT_TOUCH_MINOR => {
                let raw = value_to_raw(ev.value)?;
                let ok = check_range(raw, spec.minor_max)?;
                put_u32(&mut acc.minor, ok)?;
            }
            _ => return Err(TgCode::EvBad.val()),
        }
        i += 1;
    }
    if !any {
        return Err(TgCode::FrameEmpty.val());
    }

    let mut out: Vec<TouchSample> = Vec::new();
    let mut live = 0usize;
    let mut idx = 0usize;
    while idx < MAX_SLOT {
        let acc = match accs.get(idx) {
            Some(a) => *a,
            None => return Err(TgCode::SegmentBad.val()),
        };
        if acc.seen {
            let tracking = match acc.tracking {
                Some(t) => t,
                None => {
                    if !acc.lifted {
                        return Err(TgCode::TrackBad.val());
                    }
                    // 释放帧只带槽位与 LIFT：tracking 已不可考，用 0 占位。
                    // 该样本 lifted=true，不参与活体计数与跨槽去重。
                    0
                }
            };
            let major_raw = acc.major.unwrap_or(0);
            let minor_raw = acc.minor.unwrap_or(0);
            if major_raw < minor_raw {
                return Err(TgCode::AxisOrder.val());
            }
            if !acc.lifted {
                if acc.x.is_none() || acc.y.is_none() {
                    return Err(TgCode::PosBad.val());
                }
                live += 1;
                if live > MAX_TOUCH {
                    return Err(TgCode::ConcurrencyBad.val());
                }
            }
            let x_norm = match acc.x {
                Some(v) => quantize(v, spec.x_max)?,
                None => 0,
            };
            let y_norm = match acc.y {
                Some(v) => quantize(v, spec.y_max)?,
                None => 0,
            };
            let pressure = match acc.pressure {
                Some(v) => quantize(v, spec.pressure_max)?,
                None => 0,
            };
            let major = match acc.major {
                Some(v) => quantize(v, spec.major_max)?,
                None => 0,
            };
            let minor = match acc.minor {
                Some(v) => quantize(v, spec.minor_max)?,
                None => 0,
            };
            out.push(TouchSample {
                slot: idx as u8,
                tracking,
                lifted: acc.lifted,
                x_norm,
                y_norm,
                pressure,
                major,
                minor,
            });
        }
        idx += 1;
    }
    // 跨槽 tracking 去重（O(n^2)，n ≤ 10）。
    let mut a = 0usize;
    while a < out.len() {
        let mut b = a + 1;
        while b < out.len() {
            let ta = out.get(a).map(|s| s.tracking);
            let tb = out.get(b).map(|s| s.tracking);
            if ta.is_some() && ta == tb {
                let lifted = out.get(a).map(|s| s.lifted).unwrap_or(false)
                    || out.get(b).map(|s| s.lifted).unwrap_or(false);
                if !lifted {
                    return Err(TgCode::TrackDup.val());
                }
            }
            b += 1;
        }
        a += 1;
    }
    Ok(out)
}

// ── 五触点并发语料 ────────────────────────────────────────────────
/// 一组五触点帧：五个槽位、五个不同 tracking，坐标互不相同。
pub fn group_frame(group: u32) -> Vec<MtEvent> {
    let base = (group.wrapping_mul(17) % 200) as i32;
    let mut ev: Vec<MtEvent> = Vec::new();
    let mut slot = 0usize;
    while slot < MAX_TOUCH {
        ev.push(MtEvent::new(ABS_MT_SLOT, slot as i32));
        ev.push(MtEvent::new(ABS_MT_TRACKING_ID, 100 + slot as i32));
        ev.push(MtEvent::new(ABS_MT_POSITION_X, base + 10 * slot as i32));
        ev.push(MtEvent::new(ABS_MT_POSITION_Y, base + 20 * slot as i32));
        ev.push(MtEvent::new(ABS_MT_PRESSURE, 100 + slot as i32));
        ev.push(MtEvent::new(ABS_MT_TOUCH_MAJOR, 40 + slot as i32));
        ev.push(MtEvent::new(ABS_MT_TOUCH_MINOR, 20 + slot as i32));
        slot += 1;
    }
    ev
}

/// 并发判据：100 组五触点帧，逐组校验槽位与 tracking 不串。
pub fn golden_concurrency(groups: u32) -> Result<(u32, u32), u32> {
    let spec = AxisSpec::new(4096, 4096, 255, 255, 255);
    if !spec.valid() {
        return Err(TgCode::AxisBad.val());
    }
    let mut samples = 0u32;
    let mut g = 0u32;
    while g < groups {
        let frame = group_frame(g);
        let parsed = parse_frame(FrameProto::B, &spec, &frame)?;
        if parsed.len() != MAX_TOUCH {
            return Err(TgCode::ConcurrencyBad.val());
        }
        let mut i = 0usize;
        while i < parsed.len() {
            let s = match parsed.get(i) {
                Some(v) => *v,
                None => return Err(TgCode::SegmentBad.val()),
            };
            if s.slot as usize != i {
                return Err(TgCode::ConcurrencyBad.val());
            }
            if s.tracking != 100 + i as u16 {
                return Err(TgCode::TrackDup.val());
            }
            if s.lifted {
                return Err(TgCode::PosBad.val());
            }
            samples += 1;
            i += 1;
        }
        g += 1;
    }
    Ok((groups, samples))
}

/// 生命周期判据：100 轮「分配→存续→释放→悬空拒」。
pub fn golden_lifecycle(rounds: u32) -> Result<u32, u32> {
    let mut book = TrackBook::new();
    let mut rejects = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let id = (r % 1000) as u16;
        let h = book.allocate(id)?;
        book.engage(h)?;
        book.release(h)?;
        match book.engage(h) {
            Err(e) => {
                if e != TgCode::TrackDangling.val() {
                    return Err(e);
                }
                rejects += 1;
            }
            Ok(_) => return Err(TgCode::TrackDangling.val()),
        }
        match book.release(h) {
            Err(e) => {
                if e != TgCode::TrackDangling.val() {
                    return Err(e);
                }
                rejects += 1;
            }
            Ok(_) => return Err(TgCode::TrackDangling.val()),
        }
        r += 1;
    }
    book.audit()?;
    Ok(rejects)
}

/// 全链：并发 → 生命周期 → 量纲 → 双跑。
pub fn golden_run() -> Result<(u32, u32, usize, u64), u32> {
    let (groups, samples) = golden_concurrency(TOUCH_GROUPS)?;
    let rejects = golden_lifecycle(LIFE_ROUNDS)?;
    let axes = audit_axis_table()?;
    if groups != TOUCH_GROUPS || samples != TOUCH_GROUPS.saturating_mul(MAX_TOUCH as u32) {
        return Err(TgCode::ConcurrencyBad.val());
    }
    if rejects != LIFE_ROUNDS.saturating_mul(2) {
        return Err(TgCode::TrackDangling.val());
    }
    let mut h = FNV_OFF;
    h = fold_word(h, groups);
    h = fold_word(h, samples);
    h = fold_word(h, rejects);
    h = fold_word(h, axes as u32);
    Ok((groups, rejects, axes, h))
}

// ── LCG ───────────────────────────────────────────────────────────
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入账（四路：越界槽/悬空句柄/轴溢出/长短轴倒挂） ─────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let spec = AxisSpec::new(4096, 4096, 255, 255, 255);
    let mut r = 0u32;
    while r < rounds {
        rng.next();
        // 路 1：槽位 10 越界。
        let bad_slot = parse_frame(
            FrameProto::B,
            &spec,
            &[
                MtEvent::new(ABS_MT_SLOT, 10),
                MtEvent::new(ABS_MT_TRACKING_ID, 1),
                MtEvent::new(ABS_MT_POSITION_X, 1),
                MtEvent::new(ABS_MT_POSITION_Y, 1),
            ],
        ) == Err(TgCode::SlotBad.val());
        led.record(0, bad_slot);
        // 路 2：释放后再存续。
        let mut book = TrackBook::new();
        let dangling = match book.allocate(5) {
            Ok(h) => match book.release(h) {
                Ok(()) => book.engage(h) == Err(TgCode::TrackDangling.val()),
                Err(_) => false,
            },
            Err(_) => false,
        };
        led.record(1, dangling);
        // 路 3：压力轴越界。
        let bad_axis = parse_frame(
            FrameProto::B,
            &spec,
            &[
                MtEvent::new(ABS_MT_SLOT, 0),
                MtEvent::new(ABS_MT_TRACKING_ID, 1),
                MtEvent::new(ABS_MT_POSITION_X, 10),
                MtEvent::new(ABS_MT_POSITION_Y, 10),
                MtEvent::new(ABS_MT_PRESSURE, 4096),
            ],
        ) == Err(TgCode::AxisBad.val());
        led.record(2, bad_axis);
        // 路 4：major < minor。
        let bad_order = parse_frame(
            FrameProto::B,
            &spec,
            &[
                MtEvent::new(ABS_MT_SLOT, 0),
                MtEvent::new(ABS_MT_TRACKING_ID, 1),
                MtEvent::new(ABS_MT_POSITION_X, 10),
                MtEvent::new(ABS_MT_POSITION_Y, 10),
                MtEvent::new(ABS_MT_TOUCH_MAJOR, 5),
                MtEvent::new(ABS_MT_TOUCH_MINOR, 9),
            ],
        ) == Err(TgCode::AxisOrder.val());
        led.record(3, bad_order);
        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let (groups, rejects, axes, _) = golden_run()?;
    if groups == TOUCH_GROUPS && rejects == LIFE_ROUNDS * 2 && axes == 10 {
        Ok(())
    } else {
        Err(TgCode::ReplayBad.val())
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    let spec = AxisSpec::new(4096, 4096, 255, 255, 255);
    if parse_frame(FrameProto::B, &spec, &[]) != Err(TgCode::FrameEmpty.val()) {
        return Err(TgCode::FrameEmpty.val());
    }
    // 协议 A 携带 SLOT 必须拒。
    if parse_frame(
        FrameProto::A,
        &spec,
        &[MtEvent::new(ABS_MT_SLOT, 0), MtEvent::new(ABS_MT_POSITION_X, 1)],
    ) != Err(TgCode::EvBad.val())
    {
        return Err(TgCode::EvBad.val());
    }
    // 同帧同轴重复必须拒。
    if parse_frame(
        FrameProto::B,
        &spec,
        &[
            MtEvent::new(ABS_MT_SLOT, 0),
            MtEvent::new(ABS_MT_TRACKING_ID, 1),
            MtEvent::new(ABS_MT_POSITION_X, 1),
            MtEvent::new(ABS_MT_POSITION_X, 2),
            MtEvent::new(ABS_MT_POSITION_Y, 1),
        ],
    ) != Err(TgCode::EvDup.val())
    {
        return Err(TgCode::EvDup.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    if MAX_TOUCH == 5
        && MAX_SLOT == 10
        && SCALE_FULL == 1000
        && RAW_MAX == 0xFFFF
        && TRACK_LIFT == -1
        && AXIS_SPECS.len() == 10
    {
        Ok(())
    } else {
        Err(TgCode::SegmentBad.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TgCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < TgCode::ALL.len() {
        let v = TgCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x20 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let conc = golden_concurrency(TOUCH_GROUPS);
    v.push((
        "F48845-J1-五触点百组不混淆",
        match &conc {
            Ok((g, s)) => *g == 100 && *s == 500,
            Err(_) => false,
        },
        match &conc {
            Ok((g, s)) => format!("groups={} samples={}", g, s),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let life = golden_lifecycle(LIFE_ROUNDS);
    v.push((
        "F48846-J1-百轮悬空全拒",
        match &life {
            Ok(n) => *n == 200,
            Err(_) => false,
        },
        match &life {
            Ok(n) => format!("dangling_rejects={}", n),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let axes = audit_axis_table();
    v.push((
        "F48847-J1-十设备量纲表一致",
        axes.ok() == Some(10),
        format!("rows={:?}", axes.ok()),
    ));
    let spec = AxisSpec::new(4096, 4096, 255, 255, 255);
    let lifted = parse_frame(
        FrameProto::B,
        &spec,
        &[
            MtEvent::new(ABS_MT_SLOT, 2),
            MtEvent::new(ABS_MT_TRACKING_ID, TRACK_LIFT),
        ],
    );
    v.push((
        "F48845-J1-抬起帧无坐标可解",
        match &lifted {
            Ok(v) => v.len() == 1 && v.iter().all(|s| s.lifted),
            Err(_) => false,
        },
        match &lifted {
            Ok(v) => format!("lift_samples={}", v.len()),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let dup = parse_frame(
        FrameProto::B,
        &spec,
        &[
            MtEvent::new(ABS_MT_SLOT, 0),
            MtEvent::new(ABS_MT_TRACKING_ID, 7),
            MtEvent::new(ABS_MT_POSITION_X, 1),
            MtEvent::new(ABS_MT_POSITION_Y, 1),
            MtEvent::new(ABS_MT_SLOT, 1),
            MtEvent::new(ABS_MT_TRACKING_ID, 7),
            MtEvent::new(ABS_MT_POSITION_X, 2),
            MtEvent::new(ABS_MT_POSITION_Y, 2),
        ],
    );
    v.push((
        "F48845-J1-跨槽同 tracking 拒",
        dup == Err(TgCode::TrackDup.val()),
        format!("dup={:?}", dup.err()),
    ));
    let six = group_frame(0);
    let mut ev6 = six.clone();
    ev6.push(MtEvent::new(ABS_MT_SLOT, 5));
    ev6.push(MtEvent::new(ABS_MT_TRACKING_ID, 105));
    ev6.push(MtEvent::new(ABS_MT_POSITION_X, 300));
    ev6.push(MtEvent::new(ABS_MT_POSITION_Y, 300));
    v.push((
        "F48845-J1-第六触点并发拒",
        parse_frame(FrameProto::B, &spec, &ev6) == Err(TgCode::ConcurrencyBad.val()),
        String::from("5+1 → ConcurrencyBad"),
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48846-J1-注入四路10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48847-J1-探针三序与双跑",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok() && repro_check().is_ok(),
        format!(
            "a={} b={} c={} repro={}",
            probe_seq_a().is_ok(),
            probe_seq_b().is_ok(),
            probe_seq_c().is_ok(),
            repro_check().is_ok()
        ),
    ));
    v.push((
        "F48847-J1-段位守卫",
        segment_guard(),
        String::from("0x70580 窗口 13 码"),
    ));
    v
}

pub fn run_unx48845_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48845 MT-slot 多点触控解析与 5 触点并发");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> AxisSpec {
        AxisSpec::new(4096, 4096, 255, 255, 255)
    }

    #[test]
    fn quantize_endpoints() {
        assert_eq!(quantize(0, 4096), Ok(0));
        assert_eq!(quantize(4096, 4096), Ok(1000));
        assert_eq!(quantize(2048, 4096), Ok(500));
    }

    #[test]
    fn quantize_rejects_overflow() {
        assert_eq!(quantize(4097, 4096), Err(TgCode::AxisBad.val()));
        assert_eq!(quantize(1, 0), Err(TgCode::AxisBad.val()));
    }

    #[test]
    fn axis_table_has_ten_valid_rows() {
        assert_eq!(AXIS_SPECS.len(), 10);
        assert_eq!(audit_axis_table(), Ok(10));
    }

    #[test]
    fn five_touches_parse_in_order() {
        let r = parse_frame(FrameProto::B, &spec(), &group_frame(7));
        assert!(r.is_ok());
        if let Ok(samples) = r {
            assert_eq!(samples.len(), 5);
            assert_eq!(samples.first().map(|s| s.tracking), Some(100));
            assert_eq!(samples.last().map(|s| s.tracking), Some(104));
        }
    }

    #[test]
    fn five_touch_groups_do_not_mix() {
        let r = golden_concurrency(100);
        assert_eq!(r.ok(), Some((100, 500)));
    }

    #[test]
    fn six_touches_is_concurrency_bad() {
        let mut ev = group_frame(0);
        ev.push(MtEvent::new(ABS_MT_SLOT, 5));
        ev.push(MtEvent::new(ABS_MT_TRACKING_ID, 105));
        ev.push(MtEvent::new(ABS_MT_POSITION_X, 100));
        ev.push(MtEvent::new(ABS_MT_POSITION_Y, 100));
        assert_eq!(
            parse_frame(FrameProto::B, &spec(), &ev).err(),
            Some(TgCode::ConcurrencyBad.val())
        );
    }

    #[test]
    fn duplicate_tracking_across_slots_is_rejected() {
        let ev = [
            MtEvent::new(ABS_MT_SLOT, 0),
            MtEvent::new(ABS_MT_TRACKING_ID, 9),
            MtEvent::new(ABS_MT_POSITION_X, 10),
            MtEvent::new(ABS_MT_POSITION_Y, 10),
            MtEvent::new(ABS_MT_SLOT, 1),
            MtEvent::new(ABS_MT_TRACKING_ID, 9),
            MtEvent::new(ABS_MT_POSITION_X, 20),
            MtEvent::new(ABS_MT_POSITION_Y, 20),
        ];
        assert_eq!(
            parse_frame(FrameProto::B, &spec(), &ev).err(),
            Some(TgCode::TrackDup.val())
        );
    }

    #[test]
    fn duplicate_axis_is_rejected() {
        let ev = [
            MtEvent::new(ABS_MT_SLOT, 0),
            MtEvent::new(ABS_MT_TRACKING_ID, 1),
            MtEvent::new(ABS_MT_POSITION_X, 1),
            MtEvent::new(ABS_MT_POSITION_X, 2),
            MtEvent::new(ABS_MT_POSITION_Y, 1),
        ];
        assert_eq!(
            parse_frame(FrameProto::B, &spec(), &ev).err(),
            Some(TgCode::EvDup.val())
        );
    }

    #[test]
    fn missing_position_is_rejected() {
        let ev = [
            MtEvent::new(ABS_MT_SLOT, 0),
            MtEvent::new(ABS_MT_TRACKING_ID, 1),
            MtEvent::new(ABS_MT_POSITION_X, 1),
        ];
        assert_eq!(
            parse_frame(FrameProto::B, &spec(), &ev).err(),
            Some(TgCode::PosBad.val())
        );
    }

    #[test]
    fn lift_without_id_is_accepted() {
        let ev = [MtEvent::new(ABS_MT_SLOT, 0), MtEvent::new(ABS_MT_TRACKING_ID, -1)];
        match parse_frame(FrameProto::B, &spec(), &ev) {
            Ok(v) => {
                assert_eq!(v.len(), 1);
                match v.get(0) {
                    Some(s) => {
                        assert!(s.lifted);
                        assert_eq!(s.tracking, 0);
                    }
                    None => assert!(false),
                }
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn lift_with_id_is_accepted() {
        let ev = [
            MtEvent::new(ABS_MT_SLOT, 3),
            MtEvent::new(ABS_MT_TRACKING_ID, 42),
            MtEvent::new(ABS_MT_TRACKING_ID, -1),
        ];
        let r = parse_frame(FrameProto::B, &spec(), &ev);
        assert!(r.is_ok());
        if let Ok(v) = r {
            assert_eq!(v.len(), 1);
            assert!(v.get(0).map(|s| s.lifted).unwrap_or(false));
        }
    }

    #[test]
    fn protocol_a_rejects_slot_code() {
        let ev = [MtEvent::new(ABS_MT_SLOT, 0), MtEvent::new(ABS_MT_POSITION_X, 1)];
        assert_eq!(
            parse_frame(FrameProto::A, &spec(), &ev).err(),
            Some(TgCode::EvBad.val())
        );
    }

    #[test]
    fn protocol_a_single_sample() {
        let ev = [
            MtEvent::new(ABS_MT_TRACKING_ID, 11),
            MtEvent::new(ABS_MT_POSITION_X, 2048),
            MtEvent::new(ABS_MT_POSITION_Y, 1024),
            MtEvent::new(ABS_MT_PRESSURE, 255),
        ];
        let r = parse_frame(FrameProto::A, &spec(), &ev);
        assert!(r.is_ok());
        if let Ok(v) = r {
            assert_eq!(v.len(), 1);
            assert_eq!(v.first().map(|s| s.x_norm), Some(500));
        }
    }

    #[test]
    fn unknown_code_is_rejected() {
        let ev = [MtEvent::new(0x99, 1)];
        assert_eq!(
            parse_frame(FrameProto::B, &spec(), &ev).err(),
            Some(TgCode::EvBad.val())
        );
    }

    #[test]
    fn lifecycle_rejects_dangling() {
        let mut b = TrackBook::new();
        let h = b.allocate(3);
        assert!(h.is_ok());
        if let Ok(hh) = h {
            assert!(b.engage(hh).is_ok());
            assert!(b.release(hh).is_ok());
            assert_eq!(b.engage(hh).err(), Some(TgCode::TrackDangling.val()));
            assert_eq!(b.release(hh).err(), Some(TgCode::TrackDangling.val()));
        }
    }

    #[test]
    fn duplicate_live_tracking_is_rejected() {
        let mut b = TrackBook::new();
        assert!(b.allocate(8).is_ok());
        assert_eq!(b.allocate(8).err(), Some(TgCode::TrackDup.val()));
    }

    #[test]
    fn released_tracking_reuses_with_new_generation() {
        let mut b = TrackBook::new();
        let h = b.allocate(8);
        assert!(h.is_ok());
        if let Ok(hh) = h {
            assert!(b.release(hh).is_ok());
            let h2 = b.allocate(8);
            assert!(h2.is_ok());
            if let Ok(hh2) = h2 {
                assert_eq!(hh2.gen, hh.gen + 1);
                assert_eq!(b.engage(hh).err(), Some(TgCode::TrackDangling.val()));
                assert!(b.engage(hh2).is_ok());
            }
        }
    }

    #[test]
    fn hundred_lifecycles_audit_clean() {
        assert_eq!(golden_lifecycle(100), Ok(200));
    }

    #[test]
    fn free_book_is_balanced() {
        let b = TrackBook::new();
        assert!(b.audit().is_ok());
        assert_eq!(b.allocs(), 0);
        assert_eq!(b.live_count(), 0);
    }

    #[test]
    fn golden_run_is_deterministic() {
        assert_eq!(golden_run(), golden_run());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn all_codes_inside_segment() {
        assert!(segment_guard());
        assert_eq!(TgCode::ALL.len(), 13);
        assert_eq!(TgCode::EvBad.val(), SEG_BASE + 1);
        assert_eq!(TgCode::SegmentBad.val(), SEG_BASE + 13);
    }
}
