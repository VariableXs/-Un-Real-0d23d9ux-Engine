//! UNX-F48848 触点轨迹缓存与丢失恢复对账 · 0x705E0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   F48848 ① 纯功能代码 ≥320 行；
//!          ② UNX-F48848-J1 轨迹 ×100 组采样零丢点；
//!   F48849 ① 纯功能代码 ≥320 行；
//!          ② UNX-F48849-J1 丢点注入 ×10 回收全入账；
//!   F48850 ① 纯功能代码 ≥300 行；
//!          ② UNX-F48850-J1 对账 ×10^3 零混淆。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：时间单调 → 窗口合法 → 淘汰水位 → 轨迹命中 → 静默超时 →
//!           恢复记账 → 帧计数对账。
//!   次序不可倒：先裁窗口后验时间，倒退时间戳会被裁成「窗口内正常点」；
//!   淘汰水位必须先于窗口读取，否则被环形覆盖掉的点会被当成从未存在；
//!   静默超时必须后于轨迹命中，未知 tracking 不该触发回收账。
//! 关键差异：unx48845 把一帧解析成触点样本；本模块把样本按 tracking
//!   攒成时间窗轨迹，并在静默超时后回收。手势状态机（B10）只消费窗口
//!   快照，不直接读环形缓存；本模块不判定手势、不合成指针位移。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x705E0，VX6 批 B03 第四槽）。
pub const SEG_BASE: u32 = 0x705E0;

/// 单条轨迹环形容量（64 点，覆盖 80ms 窗口在 1000Hz 下的 80 点峰值有余）。
pub const TRAJ_CAP: usize = 64;
/// 采样窗口宽度（毫秒，冻结）。
pub const WINDOW_MS: u32 = 80;
/// 静默触点超时（毫秒，冻结 100：与缺 SYN 超时同一口径）。
pub const TIMEOUT_MS: u32 = 100;
/// 并发轨迹上限（5 触点）。
pub const MAX_TRACK: usize = 5;
/// 轨迹判据组数。
pub const GROUPS: u32 = 100;
/// 每组帧数。
pub const GROUP_FRAMES: u32 = 20;
/// 丢点注入台数。
pub const LOSS_INJECT: u32 = 10;
/// 对账帧数（10^3）。
pub const RECON_FRAMES: u32 = 1000;

// ── 诊断码闭集（+0x01..+0x0D） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrCode {
    /// 轨迹容量非法
    CapBad,
    /// 时间戳非单调
    TimeBad,
    /// 采样窗口非法（from > to）
    WindowBad,
    /// 未知 tracking
    TrackBad,
    /// 并发轨迹超限
    TrackOver,
    /// 环形淘汰落入采样窗口：窗口内已丢点
    LossBad,
    /// 静默超时参数非法
    TimeoutBad,
    /// 恢复记账不平
    RecoverBad,
    /// 帧计数对账不平
    ReconcileBad,
    /// 槽位与 tracking 串路
    ConfuseBad,
    /// 双跑不一致
    ReplayBad,
    /// 段位越界
    SegmentBad,
}

impl TrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            TrCode::CapBad => "轨迹容量非法：必须落在 1..=64",
            TrCode::TimeBad => "时间戳非单调：新点早于或等于上一点",
            TrCode::WindowBad => "采样窗口非法：起点晚于终点",
            TrCode::TrackBad => "未知 tracking：轨迹缓存未登记该触点",
            TrCode::TrackOver => "并发轨迹超限：同时只允许 5 条",
            TrCode::LossBad => "窗口内丢点：环形淘汰水位侵入采样窗口",
            TrCode::TimeoutBad => "静默超时参数非法：必须大于 0",
            TrCode::RecoverBad => "恢复记账不平：回收数 ≠ 恢复数 + 未恢复数",
            TrCode::ReconcileBad => "帧计数对账不平：期望 ≠ 接收 + 丢弃",
            TrCode::ConfuseBad => "槽位串路：同一槽位的 tracking 中途改变",
            TrCode::ReplayBad => "双跑不一致：轨迹账两次重放不同",
            TrCode::SegmentBad => "段位越界：诊断码逃出 0x705E0 窗口",
        }
    }

    pub const ALL: [TrCode; 12] = [
        TrCode::CapBad,
        TrCode::TimeBad,
        TrCode::WindowBad,
        TrCode::TrackBad,
        TrCode::TrackOver,
        TrCode::LossBad,
        TrCode::TimeoutBad,
        TrCode::RecoverBad,
        TrCode::ReconcileBad,
        TrCode::ConfuseBad,
        TrCode::ReplayBad,
        TrCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < TrCode::ALL.len() {
        if TrCode::ALL[i].val() == code {
            return TrCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x705E0 段闭集内"
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

// ── 轨迹点与环形轨迹 ──────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Point {
    pub ts: u32,
    pub x: u16,
    pub y: u16,
    pub pressure: u16,
}

impl Point {
    pub fn new(ts: u32, x: u16, y: u16, pressure: u16) -> Point {
        Point { ts, x, y, pressure }
    }

    pub fn zero() -> Point {
        Point { ts: 0, x: 0, y: 0, pressure: 0 }
    }

    pub fn text(&self) -> String {
        format!("t={} ({}, {}) p={}", self.ts, self.x, self.y, self.pressure)
    }
}

/// 单条 tracking 的环形轨迹。
///
/// 淘汰语义：容量满时覆盖最旧点，并记录被覆盖点的最大时间戳
/// `evict_floor`。窗口采样如果落在 `evict_floor` 及更早，必须报
/// LossBad，而不是把「被覆盖」静默当作「本来就没有」。
pub struct Trajectory {
    pub tracking: u16,
    ring: [Point; TRAJ_CAP],
    head: usize,
    len: usize,
    total: u32,
    evicted: u32,
    evict_floor: u32,
    last_ts: u32,
}

impl Trajectory {
    pub fn new(tracking: u16) -> Trajectory {
        Trajectory {
            tracking,
            ring: [Point::zero(); TRAJ_CAP],
            head: 0usize,
            len: 0usize,
            total: 0,
            evicted: 0,
            evict_floor: 0,
            last_ts: 0,
        }
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    pub fn evicted(&self) -> u32 {
        self.evicted
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn last_ts(&self) -> u32 {
        self.last_ts
    }

    pub fn last_point(&self) -> Option<Point> {
        if self.len == 0 {
            return None;
        }
        let idx = (self.head + self.len - 1) % TRAJ_CAP;
        self.ring.get(idx).copied()
    }

    /// 入点：时间严格递增；满环时淘汰最旧点并推进水位。
    pub fn push(&mut self, p: Point) -> Result<(), u32> {
        if self.total > 0 && p.ts <= self.last_ts {
            return Err(TrCode::TimeBad.val());
        }
        if self.len == TRAJ_CAP {
            let old = match self.ring.get(self.head) {
                Some(v) => *v,
                None => return Err(TrCode::SegmentBad.val()),
            };
            if old.ts > self.evict_floor {
                self.evict_floor = old.ts;
            }
            self.head = (self.head + 1) % TRAJ_CAP;
            self.len = self.len.saturating_sub(1);
            self.evicted = self.evicted.saturating_add(1);
        }
        let idx = (self.head + self.len) % TRAJ_CAP;
        match self.ring.get_mut(idx) {
            Some(slot) => *slot = p,
            None => return Err(TrCode::SegmentBad.val()),
        }
        self.len += 1;
        self.total = self.total.saturating_add(1);
        self.last_ts = p.ts;
        Ok(())
    }

    /// 窗口采样：返回 [from, to] 闭区间内全部点（按到达序）。
    ///
    /// 若 from 已落入淘汰水位，返回 LossBad——此时「零丢点」不可承诺。
    pub fn window(&self, from: u32, to: u32) -> Result<Vec<Point>, u32> {
        if from > to {
            return Err(TrCode::WindowBad.val());
        }
        if self.evicted > 0 && from <= self.evict_floor {
            return Err(TrCode::LossBad.val());
        }
        let mut out: Vec<Point> = Vec::new();
        let mut i = 0usize;
        while i < self.len {
            let idx = (self.head + i) % TRAJ_CAP;
            let p = match self.ring.get(idx) {
                Some(v) => *v,
                None => return Err(TrCode::SegmentBad.val()),
            };
            if p.ts >= from && p.ts <= to {
                out.push(p);
            }
            i += 1;
        }
        Ok(out)
    }

    /// 内部恒等式：总数 = 在环数 + 淘汰数。
    pub fn audit(&self) -> Result<(), u32> {
        if self.total != (self.len as u32).saturating_add(self.evicted) {
            return Err(TrCode::LossBad.val());
        }
        Ok(())
    }

    fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.tracking as u32);
        h = fold_word(h, self.total);
        h = fold_word(h, self.evicted);
        h = fold_word(h, self.evict_floor);
        h = fold_word(h, self.last_ts);
        h = fold_word(h, self.len as u32);
        let mut i = 0usize;
        while i < self.len {
            let idx = (self.head + i) % TRAJ_CAP;
            if let Some(p) = self.ring.get(idx) {
                h = fold_word(h, p.ts);
                h = fold_word(h, p.x as u32);
                h = fold_word(h, p.y as u32);
                h = fold_word(h, p.pressure as u32);
            }
            i += 1;
        }
        h
    }
}

// ── 轨迹缓存（多 tracking） ───────────────────────────────────────
pub struct TrajCache {
    tracks: Vec<Trajectory>,
}

impl TrajCache {
    pub fn new() -> TrajCache {
        TrajCache { tracks: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn get(&self, tracking: u16) -> Result<&Trajectory, u32> {
        let mut i = 0usize;
        while i < self.tracks.len() {
            match self.tracks.get(i) {
                Some(t) => {
                    if t.tracking == tracking {
                        return Ok(t);
                    }
                }
                None => return Err(TrCode::SegmentBad.val()),
            }
            i += 1;
        }
        Err(TrCode::TrackBad.val())
    }

    pub fn get_mut(&mut self, tracking: u16) -> Result<&mut Trajectory, u32> {
        let mut i = 0usize;
        while i < self.tracks.len() {
            let hit = match self.tracks.get(i) {
                Some(t) => t.tracking == tracking,
                None => false,
            };
            if hit {
                return match self.tracks.get_mut(i) {
                    Some(t) => Ok(t),
                    None => Err(TrCode::SegmentBad.val()),
                };
            }
            i += 1;
        }
        Err(TrCode::TrackBad.val())
    }

    /// 入点：不存在则新建（并发上限 5）。
    pub fn push(&mut self, tracking: u16, p: Point) -> Result<(), u32> {
        if self.get(tracking).is_err() {
            if self.tracks.len() >= MAX_TRACK {
                return Err(TrCode::TrackOver.val());
            }
            self.tracks.push(Trajectory::new(tracking));
        }
        let t = self.get_mut(tracking)?;
        t.push(p)
    }

    pub fn window(&self, tracking: u16, from: u32, to: u32) -> Result<Vec<Point>, u32> {
        self.get(tracking)?.window(from, to)
    }

    pub fn drop_track(&mut self, tracking: u16) -> Result<(), u32> {
        let mut i = 0usize;
        while i < self.tracks.len() {
            let hit = match self.tracks.get(i) {
                Some(t) => t.tracking == tracking,
                None => false,
            };
            if hit {
                self.tracks.remove(i);
                return Ok(());
            }
            i += 1;
        }
        Err(TrCode::TrackBad.val())
    }

    pub fn audit(&self) -> Result<(), u32> {
        let mut i = 0usize;
        while i < self.tracks.len() {
            match self.tracks.get(i) {
                Some(t) => t.audit()?,
                None => return Err(TrCode::SegmentBad.val()),
            }
            i += 1;
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.tracks.len() {
            if let Some(t) = self.tracks.get(i) {
                h = fold_word(h, (t.fold() & 0xFFFF_FFFF) as u32);
            }
            i += 1;
        }
        fold_word(h, self.tracks.len() as u32)
    }
}

// ── 静默触点丢失/恢复账 ──────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WatchEntry {
    pub tracking: u16,
    pub last_seen: u32,
    pub recycled: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TouchOutcome {
    pub tracking: u16,
    pub recovered: bool,
}

pub struct LossWatch {
    entries: Vec<WatchEntry>,
    timeout_ms: u32,
    recycled: u32,
    recovered: u32,
    anomalies: u32,
    nonmono: u32,
}

impl LossWatch {
    pub fn new(timeout_ms: u32) -> Result<LossWatch, u32> {
        if timeout_ms == 0 {
            return Err(TrCode::TimeoutBad.val());
        }
        Ok(LossWatch {
            entries: Vec::new(),
            timeout_ms,
            recycled: 0,
            recovered: 0,
            anomalies: 0,
            nonmono: 0,
        })
    }

    pub fn timeout_ms(&self) -> u32 {
        self.timeout_ms
    }

    pub fn recycled(&self) -> u32 {
        self.recycled
    }

    pub fn recovered(&self) -> u32 {
        self.recovered
    }

    pub fn anomalies(&self) -> u32 {
        self.anomalies
    }

    pub fn nonmono(&self) -> u32 {
        self.nonmono
    }

    pub fn live(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if !e.recycled {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    fn find(&self, tracking: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.tracking == tracking {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 触点活动：登记/刷新 last_seen。
    ///
    /// 已回收的 tracking 再次出现，视为「恢复」：recovered +1，条目复活，
    /// 但回收账不冲销（回收是既成事实，恢复只记新事件）。
    pub fn touch(&mut self, tracking: u16, now: u32) -> Result<TouchOutcome, u32> {
        match self.find(tracking) {
            Some(idx) => {
                let cur = match self.entries.get(idx) {
                    Some(e) => *e,
                    None => return Err(TrCode::SegmentBad.val()),
                };
                if now < cur.last_seen {
                    self.nonmono = self.nonmono.saturating_add(1);
                    return Err(TrCode::TimeBad.val());
                }
                let recovered = cur.recycled;
                if let Some(e) = self.entries.get_mut(idx) {
                    e.last_seen = now;
                    e.recycled = false;
                }
                if recovered {
                    self.recovered = self.recovered.saturating_add(1);
                }
                Ok(TouchOutcome { tracking, recovered })
            }
            None => {
                self.entries.push(WatchEntry { tracking, last_seen: now, recycled: false });
                Ok(TouchOutcome { tracking, recovered: false })
            }
        }
    }

    /// 时间推进：静默超过 timeout 的触点回收，返回被回收的 tracking 列表。
    pub fn tick(&mut self, now: u32) -> Result<Vec<u16>, u32> {
        let mut out: Vec<u16> = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            let cur = match self.entries.get(i) {
                Some(e) => *e,
                None => return Err(TrCode::SegmentBad.val()),
            };
            if !cur.recycled {
                let silent = now.saturating_sub(cur.last_seen);
                if silent > self.timeout_ms {
                    if let Some(e) = self.entries.get_mut(i) {
                        e.recycled = true;
                    }
                    self.recycled = self.recycled.saturating_add(1);
                    self.anomalies = self.anomalies.saturating_add(1);
                    out.push(cur.tracking);
                }
            }
            i += 1;
        }
        Ok(out)
    }

    /// 对账：回收数 = 恢复数 + 仍处于回收态数。
    pub fn audit(&self) -> Result<(), u32> {
        let mut stale = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.recycled {
                    stale = stale.saturating_add(1);
                }
            }
            i += 1;
        }
        if self.recycled != self.recovered.saturating_add(stale) {
            return Err(TrCode::RecoverBad.val());
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.timeout_ms);
        h = fold_word(h, self.recycled);
        h = fold_word(h, self.recovered);
        h = fold_word(h, self.anomalies);
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = fold_word(h, e.tracking as u32);
                h = fold_word(h, e.last_seen);
                h = fold_byte(h, if e.recycled { 1 } else { 0 });
            }
            i += 1;
        }
        h
    }
}

// ── 帧计数对账（×5 触点 ×10^3 帧） ───────────────────────────────
pub struct Reconcile {
    expected: [u32; MAX_TRACK],
    accepted: [u32; MAX_TRACK],
    lost: [u32; MAX_TRACK],
    assign: [u16; MAX_TRACK],
    assigned: [bool; MAX_TRACK],
    frames: u32,
}

impl Reconcile {
    pub fn new() -> Reconcile {
        Reconcile {
            expected: [0; MAX_TRACK],
            accepted: [0; MAX_TRACK],
            lost: [0; MAX_TRACK],
            assign: [0; MAX_TRACK],
            assigned: [false; MAX_TRACK],
            frames: 0,
        }
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub fn expected_of(&self, slot: usize) -> u32 {
        self.expected.get(slot).copied().unwrap_or(0)
    }

    pub fn accepted_of(&self, slot: usize) -> u32 {
        self.accepted.get(slot).copied().unwrap_or(0)
    }

    pub fn lost_of(&self, slot: usize) -> u32 {
        self.lost.get(slot).copied().unwrap_or(0)
    }

    /// 一帧开始：五个槽位各记一次期望。
    pub fn begin_frame(&mut self) {
        let mut i = 0usize;
        while i < MAX_TRACK {
            if let Some(e) = self.expected.get_mut(i) {
                *e = e.saturating_add(1);
            }
            i += 1;
        }
        self.frames = self.frames.saturating_add(1);
    }

    /// 槽位接收：首次接收锁定 tracking，此后必须一致（防串路）。
    pub fn accept(&mut self, slot: usize, tracking: u16) -> Result<(), u32> {
        if slot >= MAX_TRACK {
            return Err(TrCode::TrackOver.val());
        }
        let locked = self.assigned.get(slot).copied().unwrap_or(false);
        if locked {
            let cur = self.assign.get(slot).copied().unwrap_or(0);
            if cur != tracking {
                return Err(TrCode::ConfuseBad.val());
            }
        } else {
            if let Some(a) = self.assign.get_mut(slot) {
                *a = tracking;
            }
            if let Some(a) = self.assigned.get_mut(slot) {
                *a = true;
            }
        }
        if let Some(a) = self.accepted.get_mut(slot) {
            *a = a.saturating_add(1);
        }
        Ok(())
    }

    /// 槽位丢弃（丢点注入的唯一入账口）。
    pub fn drop_one(&mut self, slot: usize) -> Result<(), u32> {
        if slot >= MAX_TRACK {
            return Err(TrCode::TrackOver.val());
        }
        if let Some(l) = self.lost.get_mut(slot) {
            *l = l.saturating_add(1);
        }
        Ok(())
    }

    /// 对账：每槽期望 = 接收 + 丢弃；总帧数 = 各槽期望（五槽同步）。
    pub fn audit(&self) -> Result<(), u32> {
        let mut i = 0usize;
        while i < MAX_TRACK {
            let e = self.expected.get(i).copied().unwrap_or(0);
            let a = self.accepted.get(i).copied().unwrap_or(0);
            let l = self.lost.get(i).copied().unwrap_or(0);
            if e != a.saturating_add(l) {
                return Err(TrCode::ReconcileBad.val());
            }
            if e != self.frames {
                return Err(TrCode::ReconcileBad.val());
            }
            i += 1;
        }
        Ok(())
    }

    pub fn total_accepted(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MAX_TRACK {
            n = n.saturating_add(self.accepted.get(i).copied().unwrap_or(0));
            i += 1;
        }
        n
    }

    pub fn total_lost(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MAX_TRACK {
            n = n.saturating_add(self.lost.get(i).copied().unwrap_or(0));
            i += 1;
        }
        n
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.frames);
        let mut i = 0usize;
        while i < MAX_TRACK {
            h = fold_word(h, self.expected.get(i).copied().unwrap_or(0));
            h = fold_word(h, self.accepted.get(i).copied().unwrap_or(0));
            h = fold_word(h, self.lost.get(i).copied().unwrap_or(0));
            h = fold_word(h, self.assign.get(i).copied().unwrap_or(0) as u32);
            i += 1;
        }
        h
    }
}

// ── 黄金语料 ──────────────────────────────────────────────────────
/// 一组五轨迹、每组 20 帧的窗口语料：时间戳 0,5,...,95。
pub fn group_points(group: u32) -> Vec<Vec<Point>> {
    let base = (group.wrapping_mul(13) % 40) as u32;
    let mut out: Vec<Vec<Point>> = Vec::new();
    let mut t = 0usize;
    while t < MAX_TRACK {
        let mut pts: Vec<Point> = Vec::new();
        let mut f = 0u32;
        while f < GROUP_FRAMES {
            let ts = f.saturating_mul(5).saturating_add(base % 5);
            let x = ((t as u32).saturating_mul(100).saturating_add(f.saturating_mul(3)) % 4000) as u16;
            let y = ((t as u32).saturating_mul(80).saturating_add(f.saturating_mul(2)) % 4000) as u16;
            let p = (100u32.saturating_add((f % 100) as u32)) as u16;
            pts.push(Point::new(ts, x, y, p));
            f += 1;
        }
        out.push(pts);
        t += 1;
    }
    out
}

/// 轨迹判据：100 组 × 5 轨迹 × 20 点，窗口采样零丢点。
pub fn golden_trajectory(groups: u32) -> Result<(u32, u32), u32> {
    let mut points = 0u32;
    let mut g = 0u32;
    while g < groups {
        let mut cache = TrajCache::new();
        let group = group_points(g);
        let mut t = 0usize;
        while t < group.len() {
            let pts = match group.get(t) {
                Some(v) => v,
                None => return Err(TrCode::SegmentBad.val()),
            };
            let tracking = (1000 + t as u16).saturating_add((g % 7) as u16);
            let mut i = 0usize;
            while i < pts.len() {
                if let Some(p) = pts.get(i) {
                    cache.push(tracking, *p)?;
                    points = points.saturating_add(1);
                }
                i += 1;
            }
            let window = cache.window(tracking, 0, 100)?;
            if window.len() != GROUP_FRAMES as usize {
                return Err(TrCode::LossBad.val());
            }
            t += 1;
        }
        cache.audit()?;
        g += 1;
    }
    Ok((groups, points))
}

/// 丢点注入：10 台触点静默超时，回收入账；再次触碰记为恢复。
pub fn golden_loss(injects: u32) -> Result<LossWatch, u32> {
    let mut watch = LossWatch::new(TIMEOUT_MS)?;
    let mut i = 0u32;
    while i < injects {
        let tracking = (2000 + i) as u16;
        watch.touch(tracking, 0)?;
        i += 1;
    }
    let recycled = watch.tick(TIMEOUT_MS.saturating_add(50))?;
    if recycled.len() != injects as usize {
        return Err(TrCode::RecoverBad.val());
    }
    let mut k = 0u32;
    while k < injects {
        let tracking = (2000 + k) as u16;
        let outcome = watch.touch(tracking, TIMEOUT_MS.saturating_add(100))?;
        if !outcome.recovered {
            return Err(TrCode::RecoverBad.val());
        }
        k += 1;
    }
    watch.audit()?;
    Ok(watch)
}

/// 对账判据：1000 帧 × 5 槽，逐帧全收，零丢零混淆。
pub fn golden_reconcile(frames: u32) -> Result<Reconcile, u32> {
    let mut rec = Reconcile::new();
    let mut f = 0u32;
    while f < frames {
        rec.begin_frame();
        let mut slot = 0usize;
        while slot < MAX_TRACK {
            let tracking = (700 + slot) as u16;
            rec.accept(slot, tracking)?;
            slot += 1;
        }
        f += 1;
    }
    rec.audit()?;
    Ok(rec)
}

/// 全链：轨迹零丢 → 丢点回收恢复 → 千帧对账。
pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let (groups, points) = golden_trajectory(GROUPS)?;
    let watch = golden_loss(LOSS_INJECT)?;
    let rec = golden_reconcile(RECON_FRAMES)?;
    if groups != GROUPS || points != GROUPS * MAX_TRACK as u32 * GROUP_FRAMES {
        return Err(TrCode::LossBad.val());
    }
    if watch.recycled() != LOSS_INJECT || watch.recovered() != LOSS_INJECT {
        return Err(TrCode::RecoverBad.val());
    }
    if rec.total_accepted() != RECON_FRAMES * MAX_TRACK as u32 || rec.total_lost() != 0 {
        return Err(TrCode::ReconcileBad.val());
    }
    let mut h = FNV_OFF;
    h = fold_word(h, groups);
    h = fold_word(h, points);
    h = fold_word(h, watch.recycled());
    h = fold_word(h, rec.frames());
    h = fold_word(h, (rec.seal() & 0xFFFF_FFFF) as u32);
    Ok((groups, points, watch.recovered(), h))
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

// ── 注入账（四路：倒序时间/坏窗口/未知轨迹/对账不平） ────────────
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
    let mut r = 0u32;
    while r < rounds {
        rng.next();
        // 路 1：时间倒退。
        let mut t = Trajectory::new(1);
        let time_bad = match t.push(Point::new(10, 1, 1, 1)) {
            Ok(()) => t.push(Point::new(9, 2, 2, 2)) == Err(TrCode::TimeBad.val()),
            Err(_) => false,
        };
        led.record(0, time_bad);
        // 路 2：反向窗口。
        let mut t2 = Trajectory::new(2);
        let _ = t2.push(Point::new(1, 1, 1, 1));
        let win_bad = t2.window(20, 10) == Err(TrCode::WindowBad.val());
        led.record(1, win_bad);
        // 路 3：未知轨迹。
        let cache = TrajCache::new();
        let track_bad = cache.get(1234).err() == Some(TrCode::TrackBad.val());
        led.record(2, track_bad);
        // 路 4：单槽只接不丢 → 期望与接收/丢弃不平。
        let mut rec = Reconcile::new();
        rec.begin_frame();
        let _ = rec.accept(0, 10);
        let unbalanced = rec.audit() == Err(TrCode::ReconcileBad.val());
        led.record(3, unbalanced);
        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let (groups, points, recovered, _) = golden_run()?;
    if groups == GROUPS
        && points == GROUPS * MAX_TRACK as u32 * GROUP_FRAMES
        && recovered == LOSS_INJECT
    {
        Ok(())
    } else {
        Err(TrCode::ReplayBad.val())
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 容量满后的旧窗口必须报丢点。
    let mut t = Trajectory::new(5);
    let mut i = 0u32;
    while i <= TRAJ_CAP as u32 {
        t.push(Point::new(i, 0, 0, 0))?;
        i += 1;
    }
    if t.window(0, 10) != Err(TrCode::LossBad.val()) {
        return Err(TrCode::LossBad.val());
    }
    // 水位之后的窄窗口仍可采（from == to 命中单点）。
    if t.window(1, 1).map(|v| v.len()) != Ok(1) {
        return Err(TrCode::LossBad.val());
    }
    // 回收后同 tracking 再触碰必须记恢复。
    let mut w = LossWatch::new(TIMEOUT_MS)?;
    w.touch(77, 0)?;
    let _ = w.tick(TIMEOUT_MS + 1)?;
    let out = w.touch(77, TIMEOUT_MS + 2)?;
    if !out.recovered {
        return Err(TrCode::RecoverBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    if TRAJ_CAP == 64
        && WINDOW_MS == 80
        && TIMEOUT_MS == 100
        && MAX_TRACK == 5
        && RECON_FRAMES == 1000
        && GROUPS == 100
    {
        Ok(())
    } else {
        Err(TrCode::SegmentBad.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TrCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < TrCode::ALL.len() {
        let v = TrCode::ALL[i].val();
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
    let traj = golden_trajectory(GROUPS);
    v.push((
        "F48848-J1-百组轨迹零丢点",
        match &traj {
            Ok((g, p)) => *g == 100 && *p == 10000,
            Err(_) => false,
        },
        match &traj {
            Ok((g, p)) => format!("groups={} points={}", g, p),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let loss = golden_loss(LOSS_INJECT);
    v.push((
        "F48849-J1-十台丢点回收全入账",
        match &loss {
            Ok(w) => w.recycled() == 10 && w.recovered() == 10 && w.anomalies() == 10,
            Err(_) => false,
        },
        match &loss {
            Ok(w) => format!(
                "recycled={} recovered={} anomalies={}",
                w.recycled(),
                w.recovered(),
                w.anomalies()
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let rec = golden_reconcile(RECON_FRAMES);
    v.push((
        "F48850-J1-千帧五槽零混淆",
        match &rec {
            Ok(r) => r.audit().is_ok() && r.total_accepted() == 5000 && r.total_lost() == 0,
            Err(_) => false,
        },
        match &rec {
            Ok(r) => format!(
                "frames={} accepted={} lost={}",
                r.frames(),
                r.total_accepted(),
                r.total_lost()
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    // 窗口淘汰侵入：旧窗口必须被拒。
    let mut ring = Trajectory::new(1);
    let mut i = 0u32;
    while i <= TRAJ_CAP as u32 {
        let _ = ring.push(Point::new(i, 0, 0, 0));
        i += 1;
    }
    let stale_win = ring.window(0, 5);
    v.push((
        "F48848-J1-淘汰水位拒旧窗",
        stale_win == Err(TrCode::LossBad.val()),
        format!("stale={:?} evicted={}", stale_win.err(), ring.evicted()),
    ));
    let fresh_win = ring.window(40, 64);
    v.push((
        "F48848-J1-新鲜窗口可采",
        match &fresh_win {
            Ok(w) => w.len() == 25,
            Err(_) => false,
        },
        match &fresh_win {
            Ok(w) => format!("window_points={}", w.len()),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48850-J1-串路必拒",
        {
            let mut r = Reconcile::new();
            r.begin_frame();
            let first = r.accept(0, 10);
            let second = r.accept(0, 11);
            first.is_ok() && second == Err(TrCode::ConfuseBad.val())
        },
        String::from("同槽 tracking 10→11 → ConfuseBad"),
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48849-J1-注入四路10/10",
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
        "F48848-J1-探针三序与双跑",
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
        "F48849-J1-段位守卫",
        segment_guard(),
        String::from("0x705E0 窗口 12 码"),
    ));
    v
}

pub fn run_unx48848_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48848 触点轨迹缓存与丢失恢复对账");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_keeps_order() {
        let mut t = Trajectory::new(1);
        assert!(t.push(Point::new(1, 10, 10, 1)).is_ok());
        assert!(t.push(Point::new(2, 20, 20, 2)).is_ok());
        assert_eq!(t.len(), 2);
        assert_eq!(t.last_point().map(|p| p.x), Some(20));
    }

    #[test]
    fn non_monotonic_time_is_rejected() {
        let mut t = Trajectory::new(1);
        assert!(t.push(Point::new(5, 0, 0, 0)).is_ok());
        assert_eq!(t.push(Point::new(5, 0, 0, 0)), Err(TrCode::TimeBad.val()));
        assert_eq!(t.push(Point::new(4, 0, 0, 0)), Err(TrCode::TimeBad.val()));
    }

    #[test]
    fn reverse_window_is_rejected() {
        let t = Trajectory::new(1);
        assert_eq!(t.window(9, 3), Err(TrCode::WindowBad.val()));
    }

    #[test]
    fn empty_window_is_empty() {
        let mut t = Trajectory::new(1);
        assert!(t.push(Point::new(5, 0, 0, 0)).is_ok());
        assert_eq!(t.window(6, 9).map(|v| v.len()), Ok(0));
    }

    #[test]
    fn ring_evicts_and_accounts() {
        let mut t = Trajectory::new(1);
        let mut i = 0u32;
        while i < TRAJ_CAP as u32 + 10 {
            assert!(t.push(Point::new(i, 0, 0, 0)).is_ok());
            i += 1;
        }
        assert_eq!(t.len(), TRAJ_CAP);
        assert_eq!(t.evicted(), 10);
        assert!(t.audit().is_ok());
    }

    #[test]
    fn stale_window_after_eviction_is_loss() {
        let mut t = Trajectory::new(1);
        let mut i = 0u32;
        while i <= TRAJ_CAP as u32 {
            assert!(t.push(Point::new(i, 0, 0, 0)).is_ok());
            i += 1;
        }
        assert_eq!(t.window(0, 10), Err(TrCode::LossBad.val()));
        assert!(t.window(1, 10).is_ok());
    }

    #[test]
    fn cache_rejects_sixth_track() {
        let mut c = TrajCache::new();
        let mut i = 0u16;
        while i < MAX_TRACK as u16 {
            assert!(c.push(i, Point::new(1, 0, 0, 0)).is_ok());
            i += 1;
        }
        assert_eq!(c.push(99, Point::new(1, 0, 0, 0)), Err(TrCode::TrackOver.val()));
    }

    #[test]
    fn cache_window_matches_input() {
        let mut c = TrajCache::new();
        let mut i = 0u32;
        while i < 10 {
            assert!(c.push(7, Point::new(i, i as u16, i as u16, 1)).is_ok());
            i += 1;
        }
        let w = c.window(7, 3, 6);
        assert_eq!(w.map(|v| v.len()), Ok(4));
    }

    #[test]
    fn trajectory_golden_zero_loss() {
        assert_eq!(golden_trajectory(100), Ok((100, 10000)));
    }

    #[test]
    fn loss_watch_recycles_after_timeout() {
        let mut w = LossWatch::new(TIMEOUT_MS);
        assert!(w.is_ok());
        if let Ok(mut ww) = w {
            assert!(ww.touch(1, 0).is_ok());
            let r = ww.tick(TIMEOUT_MS);
            assert_eq!(r.map(|v| v.len()), Ok(0));
            let r2 = ww.tick(TIMEOUT_MS + 1);
            assert_eq!(r2.map(|v| v.len()), Ok(1));
            assert_eq!(ww.recycled(), 1);
        }
    }

    #[test]
    fn recovery_after_recycle_is_recorded() {
        let mut w = LossWatch::new(TIMEOUT_MS);
        assert!(w.is_ok());
        if let Ok(mut ww) = w {
            assert!(ww.touch(2, 0).is_ok());
            assert!(ww.tick(TIMEOUT_MS + 1).is_ok());
            let out = ww.touch(2, TIMEOUT_MS + 2);
            assert!(out.is_ok());
            if let Ok(o) = out {
                assert!(o.recovered);
            }
            assert_eq!(ww.recovered(), 1);
            assert!(ww.audit().is_ok());
        }
    }

    #[test]
    fn ten_injections_are_all_accounted() {
        let w = golden_loss(10);
        assert!(w.is_ok());
        if let Ok(ww) = w {
            assert_eq!(ww.recycled(), 10);
            assert_eq!(ww.recovered(), 10);
            assert_eq!(ww.anomalies(), 10);
            assert!(ww.audit().is_ok());
        }
    }

    #[test]
    fn timeout_zero_is_rejected() {
        assert_eq!(LossWatch::new(0).err(), Some(TrCode::TimeoutBad.val()));
    }

    #[test]
    fn reconcile_thousand_frames() {
        let r = golden_reconcile(1000);
        assert!(r.is_ok());
        if let Ok(rr) = r {
            assert_eq!(rr.frames(), 1000);
            assert_eq!(rr.total_accepted(), 5000);
            assert_eq!(rr.total_lost(), 0);
            assert!(rr.audit().is_ok());
        }
    }

    #[test]
    fn reconcile_confusion_is_rejected() {
        let mut r = Reconcile::new();
        r.begin_frame();
        assert!(r.accept(0, 1).is_ok());
        assert_eq!(r.accept(0, 2), Err(TrCode::ConfuseBad.val()));
    }

    #[test]
    fn reconcile_unbalanced_is_rejected() {
        let mut r = Reconcile::new();
        r.begin_frame();
        assert_eq!(r.audit(), Err(TrCode::ReconcileBad.val()));
        assert!(r.accept(0, 1).is_ok());
        assert!(r.accept(1, 2).is_ok());
        assert!(r.accept(2, 3).is_ok());
        assert!(r.accept(3, 4).is_ok());
        assert!(r.accept(4, 5).is_ok());
        assert!(r.audit().is_ok());
    }

    #[test]
    fn golden_run_is_deterministic() {
        assert_eq!(golden_run(), golden_run());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn all_codes_inside_segment() {
        assert!(segment_guard());
        assert_eq!(TrCode::ALL.len(), 12);
        assert_eq!(TrCode::CapBad.val(), SEG_BASE + 1);
        assert_eq!(TrCode::SegmentBad.val(), SEG_BASE + 12);
    }
}
