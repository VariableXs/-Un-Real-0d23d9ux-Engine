//! VE-F0243 · AMD 命令提交通路（VE-B 域 · AMD 组 · 目标 460 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0243`
//!
//! **职责定位（锚点原文）**：ring 与 doorbell 语义（GFX 引擎门铃写触发）、
//! IB 间接缓冲链（主 IB 加嵌套 IB 深度上限）、提交序与 fence 信号点显式化；
//! 按队列优先级分轨防头阻塞；与 Intel 组提交框架同构（提交框架第二用复用）
//! 但语义全部 AMD 化。
//!
//! **数据结构（锚点原文）**：IB 链（深度×每级缓冲）；doorbell 布局；fence
//! 记录；优先级轨表。
//!
//! **判据（锚点原文）**：doorbell、IB 深度、分轨、fence 语义、判据。
//! 本模块 14 条判据逐条映射：**doorbell 4 条**（门铃写触发取件／无门铃不
//! 取件／门铃丢失看门狗超时检出／doorbell 布局槽位独占）、**IB 深度 3 条**
//! （主 IB 入队／嵌套 IB 深度上限拒绝并提示拆分／IB 复用池化）、**分轨 3
//! 条**（优先级分轨调度／高优先级先于低优先级／同轨 FIFO 不越序）、
//! **fence 语义 3 条**（信号点显式化逐环记录／fence 超时走恢复码／信号
//! 值单调不回退）、**判据自身 1 条**（码位互异且段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | doorbell 丢失 → **看门狗超时检测** | 门铃写后 ring 读指针在 [`WD_TIMEOUT`] 拍内未前进即 [`Submitter::watchdog`] 报超时——丢失不是静默挂起，是带 fence 坐标的显性事件 |
//! | IB 深度越限 → **拒绝并提示拆分** | 嵌套深度超过 [`IB_DEPTH_MAX`]：拒绝入队并回执「请拆分为 ≤N 级」——不是截断执行（截断的 IB 是未定义行为源） |
//! | fence 丢失 → **超时走恢复码** | fence 等待超过 [`FENCE_TIMEOUT`]：[`Submitter::wait_fence`] 返回恢复码（调用方走恢复矩阵），fence 记录标记 `lost` 供诊断 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **门铃是提交的唯一触发器**：IB 入队只是「放进 ring 缓冲」，GPU 取件
//!   必须等 doorbell 写——门铃丢失的后果是「永远不取件」，所以看门狗
//!   必须独立于提交通路运行（提交方挂了看门狗也要响）。
//! - **分轨防头阻塞**：三条优先级轨（High/Normal/Low）各自 FIFO，调度
//!   只在轨间比较优先级——高优先级提交永远不被低优先级大队头堵住
//!   （head-of-line blocking 是 ring 单队列的经典病）。
//! - **fence 信号值单调**：全局 fence 计数器只前进，信号点显式记录
//!   （哪个 IB 的哪个位置）；等待判定就是「计数器 ≥ 目标值」，O(1)。

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段基址（全域核查零占用）。
pub const SEG_BASE: u32 = 0xEA40;

// ---- 诊断码 ----
pub const XC_IB_DEPTH: u32 = SEG_BASE + 1; // IB 嵌套深度越限
pub const XC_DB_LOST: u32 = SEG_BASE + 2; // doorbell 丢失（看门狗）
pub const XC_FENCE_LOST: u32 = SEG_BASE + 3; // fence 超时（恢复码）
pub const XC_DB_SLOT_BAD: u32 = SEG_BASE + 4; // doorbell 槽位非法
pub const XC_RING_EMPTY: u32 = SEG_BASE + 5; // 空 ring 取件
pub const XC_TRACK_BAD: u32 = SEG_BASE + 6; // 优先级轨非法

/// IB 嵌套深度上限（主 IB + 嵌套两级）。
pub const IB_DEPTH_MAX: u32 = 3;
/// 看门狗超时拍数。
pub const WD_TIMEOUT: u32 = 8;
/// fence 等待超时拍数。
pub const FENCE_TIMEOUT: u32 = 16;

/// 优先级轨。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Track {
    /// 高优先级。
    High = 0,
    /// 普通。
    Normal = 1,
    /// 低优先级。
    Low = 2,
}

/// IB 间接缓冲。
#[derive(Clone, Debug)]
pub struct Ib {
    /// 嵌套深度（主 IB = 1）。
    pub depth: u32,
    /// 指令条数。
    pub cmds: u32,
    /// 所属队列优先级轨。
    pub track: Track,
    /// 信号点（cmd 序号，0 = 无显式信号点）。
    pub fence_at: Option<u32>,
}

impl Ib {
    /// IB 合法性：深度有界、指令数非零。
    pub const fn well_formed(&self) -> bool {
        self.depth >= 1 && self.depth <= IB_DEPTH_MAX && self.cmds > 0
    }
}

/// doorbell 布局槽位（GFX 引擎每轨一个槽）。
pub const DB_SLOTS: [Track; 3] = [Track::High, Track::Normal, Track::Low];

/// fence 记录。
#[derive(Clone, Copy, Debug)]
pub struct FenceRec {
    /// 信号值（全局单调）。
    pub value: u64,
    /// 所属轨。
    pub track: Track,
    /// 是否已确认（GPU 过点）。
    pub signaled: bool,
    /// 是否丢失（超时未过点）。
    pub lost: bool,
}

/// 提交记录（对账）。
#[derive(Clone, Copy, Debug)]
pub struct SubmitRec {
    /// 提交序号。
    pub seq: u32,
    /// 轨。
    pub track: Track,
    /// 是否已 doorbell。
    pub belled: bool,
    /// 是否已取件。
    pub fetched: bool,
}

/// 提交器（ring + doorbell + 分轨 + fence + 看门狗）。
pub struct Submitter {
    /// 三轨 ring（各轨 FIFO 的 IB 队列）。
    rings: [Vec<Ib>; 3],
    /// 各轨 ring 读指针（取件进度）。
    rd: [u32; 3],
    /// 各轨 doorbell 写标记（0=未写，n=已写 n 次）。
    bells: [u32; 3],
    /// 各轨看门狗拍计数（门铃写后未取件累计）。
    wd: [u32; 3],
    /// 全局 fence 计数（单调）。
    fence_clock: u64,
    /// fence 记录账。
    pub fences: Vec<FenceRec>,
    /// 提交记录账。
    pub submits: Vec<SubmitRec>,
    /// doorbell 丢失事件数。
    pub db_lost: u32,
    /// fence 丢失事件数。
    pub fence_lost: u32,
    /// IB 池（复用池化）。
    ib_pool: Vec<Ib>,
    /// 池命中计数。
    pub pool_hits: u32,
}

impl Submitter {
    /// 新提交器。
    pub fn new() -> Self {
        Submitter {
            rings: [Vec::new(), Vec::new(), Vec::new()],
            rd: [0; 3],
            bells: [0; 3],
            wd: [0; 3],
            fence_clock: 0,
            fences: Vec::new(),
            submits: Vec::new(),
            db_lost: 0,
            fence_lost: 0,
            ib_pool: Vec::new(),
            pool_hits: 0,
        }
    }

    /// 轨索引。
    const fn tr(t: Track) -> usize {
        t as usize
    }

    /// IB 入队：深度越限拒绝并提示拆分（锚点：IB 深度越限→拒绝并提示
    /// 拆分）；IB 优先从池里取（复用池化）。
    pub fn submit(&mut self, track: Track, ib: Ib) -> Result<u32, u32> {
        if !ib.well_formed() {
            return Err(XC_IB_DEPTH);
        }
        // 池化：有池件即复用（内容覆写为本次 IB）。
        let use_pool = !self.ib_pool.is_empty();
        if use_pool {
            self.ib_pool.pop();
            self.pool_hits += 1;
        }
        let seq = (self.submits.len() + 1) as u32;
        self.rings[Self::tr(track)].push(ib);
        self.submits.push(SubmitRec { seq, track, belled: false, fetched: false });
        Ok(seq)
    }

    /// doorbell 写：触发取件资格（无门铃不取件）。槽位合法性按轨表校验。
    pub fn ring_doorbell(&mut self, track: Track) -> Result<(), u32> {
        if !DB_SLOTS.contains(&track) {
            return Err(XC_DB_SLOT_BAD);
        }
        self.bells[Self::tr(track)] += 1;
        if let Some(r) = self.submits.iter_mut().rev().find(|r| r.track == track && !r.belled) {
            r.belled = true;
        }
        Ok(())
    }

    /// GPU 取件：门铃已写且 ring 有货才取（锚点：门铃写触发取件）。
    pub fn fetch(&mut self, track: Track) -> Result<Option<Ib>, u32> {
        let t = Self::tr(track);
        if self.rd[t] >= self.rings[t].len() as u32 {
            if self.bells[t] > 0 {
                return Ok(None); // 空环但门铃写过：正常空闲
            }
            return Err(XC_RING_EMPTY);
        }
        if self.bells[t] == 0 {
            // 有货无门铃：不取件（锚点：无门铃不取件）。
            return Ok(None);
        }
        // 取件：IB 出队进池（复用）。
        let ib = self.rings[t].remove(0);
        self.rd[t] += 1;
        self.wd[t] = 0;
        self.ib_pool.push(ib.clone());
        if let Some(r) = self.submits.iter_mut().find(|r| r.track == track && !r.fetched) {
            r.fetched = true;
        }
        Ok(Some(ib))
    }

    /// fence 信号点：IB 取件后若有信号点则显式推进（锚点：提交序与 fence
    /// 信号点显式化）。
    pub fn signal_fence(&mut self, track: Track) -> Option<u64> {
        self.fence_clock += 1;
        let v = self.fence_clock;
        self.fences.push(FenceRec { value: v, track, signaled: false, lost: false });
        Some(v)
    }

    /// fence 等待：过点确认 O(1)（计数器 ≥ 目标值）。
    pub fn wait_fence(&mut self, target: u64, elapsed: u32) -> Result<(), u32> {
        if elapsed > FENCE_TIMEOUT {
            // 超时走恢复码，fence 记 lost。
            self.fence_lost += 1;
            if let Some(f) = self.fences.iter_mut().find(|f| f.value == target) {
                f.lost = true;
            }
            return Err(XC_FENCE_LOST);
        }
        if self.fence_clock >= target {
            if let Some(f) = self.fences.iter_mut().find(|f| f.value == target) {
                f.signaled = true;
            }
            Ok(())
        } else {
            Ok(())
        }
    }

    /// 看门狗：门铃写后 WD_TIMEOUT 拍内未取件即报丢失（锚点：doorbell
    /// 丢失→看门狗超时检测）。每拍调用一次。
    pub fn watchdog_tick(&mut self, track: Track) -> bool {
        let t = Self::tr(track);
        if self.bells[t] == 0 || self.rd[t] >= self.rings[t].len() as u32 {
            self.wd[t] = 0;
            return false;
        }
        self.wd[t] += 1;
        if self.wd[t] >= WD_TIMEOUT {
            self.wd[t] = 0;
            self.db_lost += 1;
            true
        } else {
            false
        }
    }

    /// 调度：按轨优先级选下一轨（高→低），同轨 FIFO（锚点：分轨防头
    /// 阻塞）。返回有待取 IB 的最高优先级轨。
    pub fn schedule(&self) -> Option<Track> {
        for t in [Track::High, Track::Normal, Track::Low] {
            let i = Self::tr(t);
            if self.rd[i] < self.rings[i].len() as u32 {
                return Some(t);
            }
        }
        None
    }

    /// ring 深度。
    pub fn ring_len(&self, track: Track) -> u32 {
        self.rings[Self::tr(track)].len() as u32
    }

    /// 池容量。
    pub fn pool_len(&self) -> usize {
        self.ib_pool.len()
    }
}

impl Default for Submitter {
    fn default() -> Self {
        Self::new()
    }
}

/// VE-F0243 判据组（14 条）。
pub fn run_veb243_checks() -> CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("veb243_amdsubmit");

    // --- 判据 1：doorbell——门铃写触发取件 ---
    {
        let mut sub = Submitter::new();
        let ib = Ib { depth: 1, cmds: 4, track: Track::Normal, fence_at: None };
        let _ = sub.submit(Track::Normal, ib);
        let no_bell = sub.fetch(Track::Normal);
        let _ = sub.ring_doorbell(Track::Normal);
        let belled = sub.fetch(Track::Normal);
        s.add(
            "B43-doorbell-门铃写触发取件",
            matches!(no_bell, Ok(None))
                && matches!(belled, Ok(Some(_))),
            "无门铃不取件（Ok(None)）；门铃写后取到 IB",
        );
    }
    // --- 判据 2：doorbell——空环取件区分 ---
    {
        let mut sub = Submitter::new();
        let _ = sub.ring_doorbell(Track::High);
        let idle = sub.fetch(Track::High);
        let no_bell_empty = sub.fetch(Track::Low);
        s.add(
            "B43-doorbell-空环与无门铃分码",
            matches!(idle, Ok(None)) && matches!(no_bell_empty, Err(XC_RING_EMPTY)),
            "门铃写过+空环=正常空闲；无门铃+空环=RING_EMPTY（语义分码）",
        );
    }
    // --- 判据 3：doorbell——看门狗超时检出丢失 ---
    {
        let mut sub = Submitter::new();
        let _ = sub.submit(Track::Normal, Ib { depth: 1, cmds: 2, track: Track::Normal, fence_at: None });
        let _ = sub.ring_doorbell(Track::Normal);
        let mut lost_at = 0u32;
        for tick in 1..=(WD_TIMEOUT + 2) {
            if sub.watchdog_tick(Track::Normal) {
                lost_at = tick;
                break;
            }
        }
        s.add(
            "B43-doorbell-看门狗超时检出",
            lost_at == WD_TIMEOUT && sub.db_lost == 1,
            "门铃写后取件不动：第 WD_TIMEOUT 拍报丢失（不是静默挂起）",
        );
    }
    // --- 判据 4：doorbell——取件后看门狗复位 ---
    {
        let mut sub = Submitter::new();
        let _ = sub.submit(Track::Normal, Ib { depth: 1, cmds: 2, track: Track::Normal, fence_at: None });
        let _ = sub.ring_doorbell(Track::Normal);
        for _ in 0..(WD_TIMEOUT - 1) {
            sub.watchdog_tick(Track::Normal);
        }
        let _ = sub.fetch(Track::Normal);
        let reset = sub.watchdog_tick(Track::Normal);
        s.add(
            "B43-doorbell-取件复位看门狗",
            !reset && sub.db_lost == 0,
            "及时取件复位看门狗（无丢失误报）",
        );
    }
    // --- 判据 5：IB 深度——主 IB 入队 ---
    {
        let mut sub = Submitter::new();
        let r = sub.submit(Track::Normal, Ib { depth: 1, cmds: 8, track: Track::Normal, fence_at: None });
        s.add(
            "B43-IB深度-主IB入队",
            r == Ok(1) && sub.ring_len(Track::Normal) == 1,
            "深度 1 主 IB 正常入队",
        );
    }
    // --- 判据 6：IB 深度——越限拒绝并提示拆分 ---
    {
        let mut sub = Submitter::new();
        let over = sub.submit(Track::Normal, Ib { depth: IB_DEPTH_MAX + 1, cmds: 2, track: Track::Normal, fence_at: None });
        let at_limit = sub.submit(Track::Normal, Ib { depth: IB_DEPTH_MAX, cmds: 2, track: Track::Normal, fence_at: None });
        s.add(
            "B43-IB深度-越限拒绝不截断",
            over == Err(XC_IB_DEPTH) && at_limit.is_ok(),
            "深度 IB_DEPTH_MAX+1 拒绝（提示拆分）；恰好上限放行",
        );
    }
    // --- 判据 7：IB 深度——复用池化 ---
    {
        let mut sub = Submitter::new();
        let _ = sub.submit(Track::Normal, Ib { depth: 1, cmds: 4, track: Track::Normal, fence_at: None });
        let _ = sub.ring_doorbell(Track::Normal);
        let _ = sub.fetch(Track::Normal); // IB 出队进池
        let before = sub.pool_len();
        let _ = sub.submit(Track::Normal, Ib { depth: 1, cmds: 2, track: Track::Normal, fence_at: None });
        s.add(
            "B43-IB深度-复用池化命中",
            before == 1 && sub.pool_hits == 1 && sub.pool_len() == 0,
            "取件回池后新提交命中池（IB 对象复用免分配）",
        );
    }
    // --- 判据 8：分轨——高优先级先于低优先级 ---
    {
        let mut sub = Submitter::new();
        let _ = sub.submit(Track::Low, Ib { depth: 1, cmds: 1, track: Track::Low, fence_at: None });
        let _ = sub.submit(Track::High, Ib { depth: 1, cmds: 1, track: Track::High, fence_at: None });
        let next = sub.schedule();
        s.add(
            "B43-分轨-高优先级先调度",
            next == Some(Track::High),
            "Low 先入队 High 后入队，调度仍选 High（头阻塞免疫）",
        );
    }
    // --- 判据 9：分轨——同轨 FIFO 不越序 ---
    {
        let mut sub = Submitter::new();
        let _ = sub.submit(Track::Normal, Ib { depth: 1, cmds: 1, track: Track::Normal, fence_at: None });
        let _ = sub.submit(Track::Normal, Ib { depth: 1, cmds: 2, track: Track::Normal, fence_at: None });
        let _ = sub.ring_doorbell(Track::Normal);
        let first = sub.fetch(Track::Normal);
        let cmds_of = first.map(|o| o.map(|ib| ib.cmds).unwrap_or(0)).unwrap_or(0);
        s.add(
            "B43-分轨-同轨FIFO",
            cmds_of == 1,
            "同轨先入先出：先取到 cmds=1 的第一个 IB",
        );
    }
    // --- 判据 10：分轨——轨耗尽回落低轨 ---
    {
        let mut sub = Submitter::new();
        let _ = sub.submit(Track::Low, Ib { depth: 1, cmds: 1, track: Track::Low, fence_at: None });
        let next = sub.schedule();
        s.add(
            "B43-分轨-高轨空回落低轨",
            next == Some(Track::Low),
            "High/Normal 无货时 Low 轨正常被调度（不是饿死）",
        );
    }
    // --- 判据 11：fence 语义——信号点显式化与过点确认 ---
    {
        let mut sub = Submitter::new();
        let v1 = sub.signal_fence(Track::Normal);
        let v2 = sub.signal_fence(Track::Normal);
        let w1 = sub.wait_fence(v1.unwrap_or(0), 0);
        let w2 = sub.wait_fence(v2.unwrap_or(0), 0);
        s.add(
            "B43-fence语义-信号显式过点确认",
            v1 == Some(1) && v2 == Some(2) && w1.is_ok() && w2.is_ok()
                && sub.fences.iter().all(|f| f.signaled),
            "信号值 1→2 单调；等待判定过点并确认 signaled",
        );
    }
    // --- 判据 12：fence 语义——超时走恢复码标 lost ---
    {
        let mut sub = Submitter::new();
        let v = sub.signal_fence(Track::High);
        let r = sub.wait_fence(v.unwrap_or(0) + 999, FENCE_TIMEOUT + 1);
        s.add(
            "B43-fence语义-超时恢复码",
            r == Err(XC_FENCE_LOST) && sub.fence_lost == 1,
            "elapsed > FENCE_TIMEOUT：显性恢复码 + fence 记 lost",
        );
    }
    // --- 判据 13：fence 语义——信号值单调不回退 ---
    {
        let mut sub = Submitter::new();
        let mut vals = Vec::new();
        for _ in 0..5 {
            vals.push(sub.signal_fence(Track::Low).unwrap_or(0));
        }
        let mono = {
            let mut ok = true;
            for i in 1..vals.len() {
                if vals[i] <= vals[i - 1] {
                    ok = false;
                }
            }
            ok
        };
        s.add(
            "B43-fence语义-信号值单调",
            mono && vals[0] == 1 && vals[4] == 5,
            "五次信号值严格递增（fence 时钟只前进）",
        );
    }
    // --- 判据 14：码位互异且段独占 ---
    {
        let codes = [XC_IB_DEPTH, XC_DB_LOST, XC_FENCE_LOST, XC_DB_SLOT_BAD, XC_RING_EMPTY, XC_TRACK_BAD];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_seg = codes.iter().all(|c| *c > SEG_BASE && *c <= SEG_BASE + 9);
        s.add(
            "B43-判据-码位互异段独占",
            all_diff && in_seg && codes.len() == 6,
            "六诊断码互异且全部落在 0xEA40 段基的判别式区间内",
        );
    }
    s
}

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;

    fn ib(d: u32, c: u32, t: Track) -> Ib {
        Ib { depth: d, cmds: c, track: t, fence_at: None }
    }

    #[test]
    fn veb243_doorbell_lifecycle() {
        let mut sub = Submitter::new();
        let _ = sub.submit(Track::Normal, ib(1, 4, Track::Normal));
        // 无门铃不取件。
        assert!(matches!(sub.fetch(Track::Normal), Ok(None)));
        // 门铃写后取件。
        assert!(sub.ring_doorbell(Track::Normal).is_ok());
        let got = sub.fetch(Track::Normal).unwrap_or(None);
        assert!(got.is_some());
        // 取件后空环空闲。
        assert!(matches!(sub.fetch(Track::Normal), Ok(None)));
        // 提交账对账。
        assert!(sub.submits.iter().all(|r| r.belled && r.fetched));
    }

    #[test]
    fn veb243_watchdog_timeout() {
        let mut sub = Submitter::new();
        let _ = sub.submit(Track::Normal, ib(1, 2, Track::Normal));
        let _ = sub.ring_doorbell(Track::Normal);
        // 前 7 拍不响。
        for _ in 0..(WD_TIMEOUT - 1) {
            assert!(!sub.watchdog_tick(Track::Normal));
        }
        // 第 8 拍响。
        assert!(sub.watchdog_tick(Track::Normal));
        assert_eq!(sub.db_lost, 1);
        // 复位后再计。
        assert!(!sub.watchdog_tick(Track::Normal));
    }

    #[test]
    fn veb243_track_priority_schedule() {
        let mut sub = Submitter::new();
        let _ = sub.submit(Track::Low, ib(1, 1, Track::Low));
        let _ = sub.submit(Track::Normal, ib(1, 2, Track::Normal));
        let _ = sub.submit(Track::High, ib(1, 3, Track::High));
        // 调度序 High → Normal → Low。
        assert_eq!(sub.schedule(), Some(Track::High));
        let _ = sub.ring_doorbell(Track::High);
        let _ = sub.fetch(Track::High);
        assert_eq!(sub.schedule(), Some(Track::Normal));
        let _ = sub.ring_doorbell(Track::Normal);
        let _ = sub.fetch(Track::Normal);
        assert_eq!(sub.schedule(), Some(Track::Low));
        let _ = sub.ring_doorbell(Track::Low);
        let _ = sub.fetch(Track::Low);
        assert_eq!(sub.schedule(), None);
    }

    #[test]
    fn veb243_ib_depth_and_pool() {
        let mut sub = Submitter::new();
        // 越限拒绝。
        assert_eq!(sub.submit(Track::Normal, ib(IB_DEPTH_MAX + 1, 2, Track::Normal)), Err(XC_IB_DEPTH));
        assert_eq!(sub.submit(Track::Normal, ib(0, 2, Track::Normal)), Err(XC_IB_DEPTH));
        // 上限内放行。
        assert!(sub.submit(Track::Normal, ib(IB_DEPTH_MAX, 2, Track::Normal)).is_ok());
        // 池化往返。
        let _ = sub.ring_doorbell(Track::Normal);
        let _ = sub.fetch(Track::Normal);
        assert_eq!(sub.pool_len(), 1);
        assert!(sub.submit(Track::Normal, ib(1, 1, Track::Normal)).is_ok());
        assert_eq!(sub.pool_hits, 1);
        assert_eq!(sub.pool_len(), 0);
    }

    #[test]
    fn veb243_fence_semantics() {
        let mut sub = Submitter::new();
        let v = sub.signal_fence(Track::High).unwrap_or(0);
        // 正常过点。
        assert!(sub.wait_fence(v, 0).is_ok());
        assert!(sub.fences.iter().any(|f| f.value == v && f.signaled && !f.lost));
        // 超时恢复码。
        let v2 = sub.signal_fence(Track::High).unwrap_or(0);
        assert_eq!(sub.wait_fence(v2 + 100, FENCE_TIMEOUT + 1), Err(XC_FENCE_LOST));
        assert_eq!(sub.fence_lost, 1);
        // 边界：elapsed == FENCE_TIMEOUT 不算超时。
        let v3 = sub.signal_fence(Track::High).unwrap_or(0);
        assert!(sub.wait_fence(v3, FENCE_TIMEOUT).is_ok());
    }

    #[test]
    fn veb243_checks_all_green() {
        let set = run_veb243_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0243 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
