//! VE-F0087 · 大资源流式绑定（VE-A 域 · A42 大资源组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0087`
//!
//! **职责定位（锚点原文）**：大资源流式绑定——超大纹理/几何的流式加载绑定
//! （分块上传+按需驻留），流式的可见性（哪块在哪一档如实显示），
//! 流式失败的区域降级（缺块区域低分渲染不黑块）；含流式与磁盘缓存策略的协同。
//!
//! **数据结构（锚点原文）**：流式绑定。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 缺块 → 低分降级（缺块的区域按重要度退到 1/2、1/4 或占位图，
//!   **永不黑块**——黑块让用户以为渲染坏了，占位让用户知道「还在来」）；
//! - 流式抖动 → 预取调优（窗口内驱了又要 = 抖动，抖动升高就加深预取）；
//! - 带宽超支 → 节流（超预算的请求排队并给出重试时刻，不静默丢弃）。
//!
//! **性能逐项分解（锚点原文）**：O(块)。定容槽位线性扫描；预取选择是
//! 一次「优先级 + 到达序」的线性最小值，块数上限 64，无需堆。
//!
//! **跨批对接点（锚点原文）**：A43 稀疏联动——稀疏资源的块表由 A43 提供，
//! 本模块只管「块现在在不在、按什么档位画」。两处以 `(tile id, bytes)`
//! 对齐；字节数不符即拒（同一个 id 在两处代表不同大小的块，必然画错）。
//!
//! **无障碍与隐私（锚点原文）**：流式状态读屏可达——`visibility_line()`
//! 输出纯文本「各档块数」，不含纹理内容。
//!
//! **判据（锚点原文）**：分块上传、按需驻留、缺块降级、可见性、判据。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数定点；
//! 零浮点、零墙钟、零 IO。
//!
//! 诊断段：0xE7B0 段（0xE7B0~0xE7BF 十六码）。

use alloc::string::String;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 诊断码（0xE7B0 段）
// ---------------------------------------------------------------------------

/// 未登记的块。
pub const XC_UNKNOWN_TILE: u16 = 0xE7B0;
/// 重复登记。
pub const XC_DUP_TILE: u16 = 0xE7B1;
/// 块未驻留（却要做只有驻留块能做的事）。
pub const XC_NOT_RESIDENT: u16 = 0xE7B2;
/// 块被钉住，不可驱逐。
pub const XC_PINNED: u16 = 0xE7B3;
/// 已驻留却又请求。
pub const XC_ALREADY_RESIDENT: u16 = 0xE7B4;
/// 带宽超预算。
pub const XC_BW_EXCEEDED: u16 = 0xE7B5;
/// 块表满。
pub const XC_TILE_FULL: u16 = 0xE7B6;
/// 档位与驻留态不符（声称 Full 却没驻留）。
pub const XC_TIER_MISMATCH: u16 = 0xE7B7;
/// 块大小为零或与登记不符。
pub const XC_BAD_BYTES: u16 = 0xE7B8;
/// 未请求就完成。
pub const XC_NOT_REQUESTED: u16 = 0xE7B9;
/// 账不平。
pub const XC_LEDGER_DRIFT: u16 = 0xE7BA;

// ---------------------------------------------------------------------------
// 定容常量
// ---------------------------------------------------------------------------

/// 块表容量。
pub const CAP: usize = 64;
/// 单 tick 带宽预算（4MB）。
pub const BW_BUDGET_BYTES: u32 = 4 * 1024 * 1024;
/// tick 长度（1ms）。
pub const TICK_NS: u64 = 1_000_000;
/// 抖动窗口（200ms）。
pub const CHURN_WINDOW_NS: u64 = 200_000_000;
/// 抖动率阈值（千分比）：超过即加深预取。
pub const CHURN_TRIGGER_PERMILLE: u32 = 200;
/// 预取深度下限 / 上限。
pub const PREFETCH_MIN: u8 = 2;
pub const PREFETCH_MAX: u8 = 8;

// ---------------------------------------------------------------------------
// 块与档位
// ---------------------------------------------------------------------------

/// 块状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TileState {
    /// 未请求。
    Absent,
    /// 已入队待传。
    Requested,
    /// 驻留（可全分绘制）。
    Resident,
    /// 传输失败（等重试）。
    Failed,
}

impl TileState {
    /// 读屏名。
    pub fn name(self) -> &'static str {
        match self {
            TileState::Absent => "absent",
            TileState::Requested => "requested",
            TileState::Resident => "resident",
            TileState::Failed => "failed",
        }
    }
}

/// 绘制档位（缺块时的降级阶梯；**没有 Black 这一档**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 全分辨率。
    Full,
    /// 半分辨率。
    Half,
    /// 四分之一分辨率。
    Quarter,
    /// 占位（纯色/棋盘，明确「还在来」）。
    Placeholder,
}

impl Tier {
    /// 读屏名。
    pub fn name(self) -> &'static str {
        match self {
            Tier::Full => "full",
            Tier::Half => "half",
            Tier::Quarter => "quarter",
            Tier::Placeholder => "placeholder",
        }
    }

    /// 档位序号（0 = 全分辨率）。
    pub fn level(self) -> u8 {
        match self {
            Tier::Full => 0,
            Tier::Half => 1,
            Tier::Quarter => 2,
            Tier::Placeholder => 3,
        }
    }
}

/// 一个块。
#[derive(Clone, Copy)]
pub struct Tile {
    /// 块 id（0 = 空槽）。
    pub id: u32,
    /// 块字节数。
    pub bytes: u32,
    /// 状态。
    pub state: TileState,
    /// 重要度（越大越先传）。
    pub prio: u8,
    /// 是否钉住（钉住不可驱逐）。
    pub pinned: bool,
    /// 最后一次被使用。
    pub last_use: u64,
    /// 入队时刻。
    pub req_tick: u64,
    /// 最近一次驱逐时刻（抖动判据用）。
    pub evicted_tick: u64,
}

impl Tile {
    /// 空槽。
    pub const EMPTY: Tile = Tile {
        id: 0,
        bytes: 0,
        state: TileState::Absent,
        prio: 0,
        pinned: false,
        last_use: 0,
        req_tick: 0,
        evicted_tick: 0,
    };
}

/// 请求结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Req {
    /// 已入队，`eta_tick` 是预计可见时刻。
    Staged { eta_tick: u64 },
    /// 带宽不足，`retry_at` 之后再来。
    Throttled { retry_at: u64 },
    /// 已驻留，什么也不用做。
    Already,
}

// ---------------------------------------------------------------------------
// 流式绑定
// ---------------------------------------------------------------------------

/// 分块流式绑定器（定容、无分配、无锁语义）。
#[derive(Clone, Copy)]
pub struct StreamBinder {
    tiles: [Tile; CAP],
    bw_used: u32,
    bw_tick_end: u64,
    /// 请求总数。
    requests: u32,
    /// 完成数。
    completed: u32,
    /// 失败数。
    failed: u32,
    /// 节流次数。
    throttled: u32,
    /// 驱逐次数。
    evictions: u32,
    /// 抖动次数（驱了又在窗口内要回来）。
    churns: u32,
    /// 带宽超支被拦的字节数。
    bw_rejected_bytes: u64,
    /// 请求时块已驻留的次数（缓存命中）。
    already: u32,
}

impl StreamBinder {
    /// 空绑定器。
    pub fn new() -> StreamBinder {
        StreamBinder {
            tiles: [Tile::EMPTY; CAP],
            bw_used: 0,
            bw_tick_end: 0,
            requests: 0,
            completed: 0,
            failed: 0,
            throttled: 0,
            evictions: 0,
            churns: 0,
            bw_rejected_bytes: 0,
            already: 0,
        }
    }

    /// 查找槽位。
    pub fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < CAP {
            if self.tiles[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 块视图。
    pub fn tile(&self, id: u32) -> Option<Tile> {
        match self.find(id) {
            Some(i) => Some(self.tiles[i]),
            None => None,
        }
    }

    /// 登记一个块（字节数必须为正）。
    pub fn register(&mut self, id: u32, bytes: u32, prio: u8) -> Result<(), u16> {
        if id == 0 || bytes == 0 {
            return Err(XC_BAD_BYTES);
        }
        if self.find(id).is_some() {
            return Err(XC_DUP_TILE);
        }
        let mut slot = usize::MAX;
        let mut i = 0usize;
        while i < CAP {
            if self.tiles[i].id == 0 {
                slot = i;
                break;
            }
            i += 1;
        }
        if slot == usize::MAX {
            return Err(XC_TILE_FULL);
        }
        let mut t = Tile::EMPTY;
        t.id = id;
        t.bytes = bytes;
        t.prio = prio;
        self.tiles[slot] = t;
        Ok(())
    }

    /// 钉住 / 解钉。
    pub fn set_pinned(&mut self, id: u32, pinned: bool) -> Result<(), u16> {
        let i = match self.find(id) {
            Some(i) => i,
            None => return Err(XC_UNKNOWN_TILE),
        };
        self.tiles[i].pinned = pinned;
        Ok(())
    }

    /// 新的带宽周期（惰性推进；幂等）。
    fn roll_bw(&mut self, now: u64) {
        if now >= self.bw_tick_end {
            self.bw_used = 0;
            self.bw_tick_end = now + TICK_NS;
        }
    }

    /// 请求一个块（按需驻留入口）。
    pub fn request(&mut self, id: u32, now: u64) -> Result<Req, u16> {
        let i = match self.find(id) {
            Some(i) => i,
            None => return Err(XC_UNKNOWN_TILE),
        };
        let t = self.tiles[i];
        if t.state == TileState::Resident {
            self.already = self.already.saturating_add(1);
            return Ok(Req::Already);
        }
        if t.state == TileState::Requested {
            // 已在队列里：幂等返回同一 ETA，不重复计账。
            return Ok(Req::Staged {
                eta_tick: t.req_tick + TICK_NS,
            });
        }
        self.roll_bw(now);
        if self.bw_used.saturating_add(t.bytes) > BW_BUDGET_BYTES {
            self.throttled = self.throttled.saturating_add(1);
            self.bw_rejected_bytes = self
                .bw_rejected_bytes
                .saturating_add(t.bytes as u64);
            return Ok(Req::Throttled {
                retry_at: self.bw_tick_end,
            });
        }
        self.bw_used = self.bw_used.saturating_add(t.bytes);
        // 抖动：刚被驱逐又在窗口内要回来。
        if t.evicted_tick != 0 && now.saturating_sub(t.evicted_tick) <= CHURN_WINDOW_NS {
            self.churns = self.churns.saturating_add(1);
        }
        self.tiles[i].state = TileState::Requested;
        self.tiles[i].req_tick = now;
        self.tiles[i].last_use = now;
        self.requests = self.requests.saturating_add(1);
        Ok(Req::Staged {
            eta_tick: now + TICK_NS,
        })
    }

    /// 传输完成（字节数必须与登记一致）。
    pub fn complete(&mut self, id: u32, bytes: u32, now: u64) -> Result<u32, u16> {
        let i = match self.find(id) {
            Some(i) => i,
            None => return Err(XC_UNKNOWN_TILE),
        };
        if self.tiles[i].bytes != bytes || bytes == 0 {
            return Err(XC_BAD_BYTES);
        }
        match self.tiles[i].state {
            TileState::Requested => {}
            TileState::Resident => return Err(XC_ALREADY_RESIDENT),
            _ => return Err(XC_NOT_REQUESTED),
        }
        self.tiles[i].state = TileState::Resident;
        self.tiles[i].last_use = now;
        self.completed = self.completed.saturating_add(1);
        Ok(bytes)
    }

    /// 传输失败（留在 Failed，档位退占位）。
    pub fn fail(&mut self, id: u32) -> Result<(), u16> {
        let i = match self.find(id) {
            Some(i) => i,
            None => return Err(XC_UNKNOWN_TILE),
        };
        if self.tiles[i].state != TileState::Requested {
            return Err(XC_NOT_REQUESTED);
        }
        self.tiles[i].state = TileState::Failed;
        self.failed = self.failed.saturating_add(1);
        Ok(())
    }

    /// 驱逐（只有驻留且未钉住的块可以驱逐）。
    pub fn evict(&mut self, id: u32, now: u64) -> Result<u32, u16> {
        let i = match self.find(id) {
            Some(i) => i,
            None => return Err(XC_UNKNOWN_TILE),
        };
        if self.tiles[i].state != TileState::Resident {
            return Err(XC_NOT_RESIDENT);
        }
        if self.tiles[i].pinned {
            return Err(XC_PINNED);
        }
        let bytes = self.tiles[i].bytes;
        self.tiles[i].state = TileState::Absent;
        self.tiles[i].evicted_tick = now;
        self.evictions = self.evictions.saturating_add(1);
        Ok(bytes)
    }

    /// 绘制档位（**缺块永不黑块**）。
    ///
    /// 规则：驻留 = 全分辨率；在传 = 按重要度给 1/2 或 1/4 的旧内容；
    /// 其余（缺席/失败）= 占位。
    pub fn tier_of(&self, id: u32) -> Tier {
        match self.tile(id) {
            None => Tier::Placeholder,
            Some(t) => match t.state {
                TileState::Resident => Tier::Full,
                TileState::Requested => {
                    if t.prio >= 200 {
                        Tier::Half
                    } else {
                        Tier::Quarter
                    }
                }
                TileState::Failed => Tier::Placeholder,
                TileState::Absent => Tier::Placeholder,
            },
        }
    }

    /// 档位自洽：任何块都不能既非驻留又声称全分辨率。
    pub fn tier_honest(&self) -> bool {
        let mut i = 0usize;
        while i < CAP {
            let t = self.tiles[i];
            if t.id != 0 {
                let full = self.tier_of(t.id) == Tier::Full;
                if full != (t.state == TileState::Resident) {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 下一个该传的块（优先级降序、到达序升序）。
    pub fn next_to_fetch(&self) -> Option<u32> {
        let mut best: Option<Tile> = None;
        let mut i = 0usize;
        while i < CAP {
            let t = self.tiles[i];
            if t.id != 0 && t.state == TileState::Requested {
                let better = match best {
                    None => true,
                    Some(b) => t.prio > b.prio || (t.prio == b.prio && t.req_tick < b.req_tick),
                };
                if better {
                    best = Some(t);
                }
            }
            i += 1;
        }
        match best {
            Some(t) => Some(t.id),
            None => None,
        }
    }

    /// 抖动率（千分比）。
    pub fn churn_permille(&self) -> u32 {
        if self.requests == 0 {
            return 0;
        }
        self.churns.saturating_mul(1000) / self.requests
    }

    /// 自适应预取深度。
    pub fn prefetch_depth(&self) -> u8 {
        let c = self.churn_permille();
        if c <= CHURN_TRIGGER_PERMILLE {
            return PREFETCH_MIN;
        }
        let steps = (c - CHURN_TRIGGER_PERMILLE) / 100 + 1;
        let d = PREFETCH_MIN as u32 + steps;
        if d > PREFETCH_MAX as u32 {
            PREFETCH_MAX
        } else {
            d as u8
        }
    }

    /// 缓存命中率（千分比）：已驻留的请求算命中。
    pub fn cache_hit_permille(&self) -> u32 {
        // 命中 = 请求时块已在驻留（`Req::Already`）。分母用「传输请求 +
        // 命中」而不是只用传输请求——否则命中越多命中率越低。
        let denom = self.requests.saturating_add(self.already);
        if denom == 0 {
            return 0;
        }
        self.already.saturating_mul(1000) / denom
    }

    /// 计数快照 `(请求, 完成, 失败, 节流, 驱逐, 抖动, 已在驻留)`。
    pub fn counters(&self) -> (u32, u32, u32, u32, u32, u32, u32) {
        (
            self.requests,
            self.completed,
            self.failed,
            self.throttled,
            self.evictions,
            self.churns,
            self.already,
        )
    }

    /// 在途块数。
    pub fn in_flight(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CAP {
            let t = self.tiles[i];
            if t.id != 0 && t.state == TileState::Requested {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 账平衡：请求 ≡ 完成 + 失败 + 在途。
    ///
    /// 注意：`Already` 不入 `requests`（它不是一次传输），故不参与本等式。
    pub fn ledger_balanced(&self) -> bool {
        self.requests == self.completed + self.failed + self.in_flight()
    }

    /// 可见性行（各档块数，读屏可直接播报）。
    pub fn visibility_line(&self) -> String {
        let mut full = 0u32;
        let mut half = 0u32;
        let mut quarter = 0u32;
        let mut ph = 0u32;
        let mut i = 0usize;
        while i < CAP {
            let t = self.tiles[i];
            if t.id != 0 {
                match self.tier_of(t.id) {
                    Tier::Full => full += 1,
                    Tier::Half => half += 1,
                    Tier::Quarter => quarter += 1,
                    Tier::Placeholder => ph += 1,
                }
            }
            i += 1;
        }
        let mut s = String::new();
        s.push_str("全 ");
        push_u32(&mut s, full);
        s.push_str(" 半 ");
        push_u32(&mut s, half);
        s.push_str(" 四 ");
        push_u32(&mut s, quarter);
        s.push_str(" 占位 ");
        push_u32(&mut s, ph);
        s.push_str(" 预取 ");
        push_u32(&mut s, self.prefetch_depth() as u32);
        s
    }
}

impl Default for StreamBinder {
    fn default() -> StreamBinder {
        StreamBinder::new()
    }
}

fn push_u32(s: &mut String, mut v: u32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    while v > 0 && n < buf.len() {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        s.push(buf[n] as char);
    }
}

// ---------------------------------------------------------------------------
// 判据套件
// ---------------------------------------------------------------------------

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, if ok { "" } else { detail });
}

/// VE-F0087 判据入口。
pub fn run_vea87_checks() -> CheckSet {
    let mut set = CheckSet::new("vea87_streambind");

    // C87-01 登记与重复登记
    {
        let mut b = StreamBinder::new();
        let a = b.register(1, 1024, 100);
        let dup = b.register(1, 2048, 100);
        let ok = a.is_ok() && dup == Err(XC_DUP_TILE) && b.tile(1).is_some();
        chk(&mut set, "C87-01 登记与重复", ok, "重复登记未拒");
    }

    // C87-02 零字节/零 id 拒绝
    {
        let mut b = StreamBinder::new();
        let a = b.register(0, 1024, 0);
        let c = b.register(2, 0, 0);
        let ok = a == Err(XC_BAD_BYTES) && c == Err(XC_BAD_BYTES);
        chk(&mut set, "C87-02 块大小校验", ok, "零字节块被接受");
    }

    // C87-03 请求 → 驻留生命周期
    {
        let mut b = StreamBinder::new();
        let _ = b.register(1, 4096, 50);
        let r = b.request(1, 0);
        let st = b.tile(1).map(|t| t.state);
        let c = b.complete(1, 4096, TICK_NS);
        let ok = matches!(r, Ok(Req::Staged { .. }))
            && st == Some(TileState::Requested)
            && c == Ok(4096)
            && b.tile(1).map(|t| t.state) == Some(TileState::Resident)
            && b.ledger_balanced();
        chk(&mut set, "C87-03 请求到驻留", ok, "生命周期错或账不平");
    }

    // C87-04 重复请求幂等
    {
        let mut b = StreamBinder::new();
        let _ = b.register(1, 4096, 50);
        let a = b.request(1, 0);
        let c = b.request(1, 100);
        let ok = a == c && b.counters().0 == 1 && b.in_flight() == 1;
        chk(&mut set, "C87-04 重复请求幂等", ok, "重复入队或计数翻倍");
    }

    // C87-05 未请求就完成被拒
    {
        let mut b = StreamBinder::new();
        let _ = b.register(1, 4096, 50);
        let c = b.complete(1, 4096, 0);
        let ok = c == Err(XC_NOT_REQUESTED) && b.counters().1 == 0;
        chk(&mut set, "C87-05 未请求不完成", ok, "凭空驻留");
    }

    // C87-06 完成字节数必须与登记一致
    {
        let mut b = StreamBinder::new();
        let _ = b.register(1, 4096, 50);
        let _ = b.request(1, 0);
        let bad = b.complete(1, 4095, 0);
        let ok = bad == Err(XC_BAD_BYTES) && b.tile(1).map(|t| t.state) == Some(TileState::Requested);
        chk(&mut set, "C87-06 字节数对齐", ok, "字节不符仍驻留");
    }

    // C87-07 缺块降级阶梯（永不黑块）
    {
        let mut b = StreamBinder::new();
        let _ = b.register(1, 4096, 250);
        let _ = b.register(2, 4096, 100);
        let _ = b.register(3, 4096, 100);
        let absent = b.tier_of(2);
        let _ = b.request(1, 0);
        let _ = b.request(2, 0);
        let hi = b.tier_of(1);
        let lo = b.tier_of(2);
        let _ = b.request(3, 0);
        let _ = b.fail(3);
        let failed = b.tier_of(3);
        let ok = absent == Tier::Placeholder
            && hi == Tier::Half
            && lo == Tier::Quarter
            && failed == Tier::Placeholder
            && b.tier_of(99) == Tier::Placeholder;
        chk(&mut set, "C87-07 缺块降级阶梯", ok, "缺块档位错或出现黑块");
    }

    // C87-08 可见性如实（档位与驻留态一致）
    {
        let mut b = StreamBinder::new();
        let _ = b.register(1, 1024, 10);
        let _ = b.register(2, 1024, 10);
        let _ = b.request(1, 0);
        let _ = b.complete(1, 1024, 10);
        let ok = b.tier_honest()
            && b.tier_of(1) == Tier::Full
            && b.tier_of(2) != Tier::Full
            && b.visibility_line().contains("全 1");
        chk(&mut set, "C87-08 可见性如实", ok, "非驻留块声称全分辨率");
    }

    // C87-09 带宽超支节流并给重试时刻
    {
        let mut b = StreamBinder::new();
        let big = BW_BUDGET_BYTES / 2 + 1;
        let _ = b.register(1, big, 10);
        let _ = b.register(2, big, 10);
        let _ = b.register(3, big, 10);
        let a = b.request(1, 0);
        let c = b.request(2, 0);
        let d = b.request(3, 0);
        let ok = matches!(a, Ok(Req::Staged { .. }))
            && matches!(c, Ok(Req::Throttled { retry_at }) if retry_at == TICK_NS)
            && d == c
            && b.counters().3 == 2;
        chk(&mut set, "C87-09 带宽节流", ok, "超支未拦或重试时刻错");
    }

    // C87-10 节流恢复：下一 tick 放行
    {
        let mut b = StreamBinder::new();
        let big = BW_BUDGET_BYTES / 2 + 1;
        let _ = b.register(1, big, 10);
        let _ = b.register(2, big, 10);
        let _ = b.request(1, 0);
        let blocked = b.request(2, 0);
        let later = b.request(2, TICK_NS);
        let ok = matches!(blocked, Ok(Req::Throttled { .. })) && matches!(later, Ok(Req::Staged { .. }));
        chk(&mut set, "C87-10 节流恢复", ok, "过了 tick 仍被拦");
    }

    // C87-11 驱逐仅限驻留块
    {
        let mut b = StreamBinder::new();
        let _ = b.register(1, 512, 10);
        let e1 = b.evict(1, 0);
        let _ = b.request(1, 0);
        let e2 = b.evict(1, 0);
        let _ = b.complete(1, 512, 10);
        let e3 = b.evict(1, 20);
        let ok = e1 == Err(XC_NOT_RESIDENT) && e2 == Err(XC_NOT_RESIDENT) && e3 == Ok(512);
        chk(&mut set, "C87-11 驱逐前提", ok, "非驻留块被驱逐");
    }

    // C87-12 钉住的块不可驱逐
    {
        let mut b = StreamBinder::new();
        let _ = b.register(1, 512, 10);
        let _ = b.request(1, 0);
        let _ = b.complete(1, 512, 10);
        let _ = b.set_pinned(1, true);
        let e = b.evict(1, 20);
        let _ = b.set_pinned(1, false);
        let e2 = b.evict(1, 30);
        let ok = e == Err(XC_PINNED) && e2 == Ok(512) && b.counters().4 == 1;
        chk(&mut set, "C87-12 钉住保护", ok, "钉住块被驱逐");
    }

    // C87-13 抖动检测（窗口内重请求）
    {
        let mut b = StreamBinder::new();
        let _ = b.register(1, 512, 10);
        let _ = b.request(1, 0);
        let _ = b.complete(1, 512, 10);
        let _ = b.evict(1, 100);
        let in_window = b.request(1, 100 + CHURN_WINDOW_NS);
        let n1 = b.counters().5;
        let _ = b.complete(1, 512, 200_000_000);
        let _ = b.evict(1, 200_000_100);
        let out_window = b.request(1, 200_000_100 + CHURN_WINDOW_NS + 1);
        let ok = matches!(in_window, Ok(Req::Staged { .. }))
            && n1 == 1
            && matches!(out_window, Ok(Req::Staged { .. }))
            && b.counters().5 == 1;
        chk(&mut set, "C87-13 抖动窗口", ok, "窗口边界判错");
    }

    // C87-14 预取深度自适应并封顶
    {
        let mut b = StreamBinder::new();
        let _ = b.register(1, 64, 10);
        let base = b.prefetch_depth();
        let mut k = 0u32;
        while k < 40 {
            let _ = b.request(1, k as u64 * 1000);
            let _ = b.complete(1, 64, k as u64 * 1000 + 1);
            let _ = b.evict(1, k as u64 * 1000 + 2);
            k += 1;
        }
        let deep = b.prefetch_depth();
        let ok = base == PREFETCH_MIN && deep > base && deep <= PREFETCH_MAX;
        chk(&mut set, "C87-14 预取自适应", ok, "抖动未加深预取或越上界");
    }

    // C87-15 高优先块先传
    {
        let mut b = StreamBinder::new();
        let _ = b.register(1, 64, 10);
        let _ = b.register(2, 64, 200);
        let _ = b.request(1, 0);
        let _ = b.request(2, 1000);
        let first = b.next_to_fetch();
        let _ = b.complete(2, 64, 2000);
        let second = b.next_to_fetch();
        let ok = first == Some(2) && second == Some(1);
        chk(&mut set, "C87-15 优先级出队", ok, "低优先块先出队");
    }

    // C87-16 失败块退占位且可重试
    {
        let mut b = StreamBinder::new();
        let _ = b.register(1, 128, 10);
        let _ = b.request(1, 0);
        let _ = b.fail(1);
        let tier = b.tier_of(1);
        let again = b.request(1, 100);
        let ok = tier == Tier::Placeholder
            && matches!(again, Ok(Req::Staged { .. }))
            && b.counters().2 == 1
            && b.ledger_balanced();
        chk(&mut set, "C87-16 失败退占位可重试", ok, "失败块声称有内容或不可重试");
    }

    // C87-17 满账诚实
    {
        let mut b = StreamBinder::new();
        let mut i = 0u32;
        while i < CAP as u32 {
            let _ = b.register(i + 1, 64, 0);
            i += 1;
        }
        let full = b.register(CAP as u32 + 1, 64, 0);
        let ok = full == Err(XC_TILE_FULL) && b.find(CAP as u32 + 1).is_none();
        chk(&mut set, "C87-17 块表满", ok, "满表静默覆盖");
    }

    // C87-18 账平衡（多路径混合）
    {
        let mut b = StreamBinder::new();
        let _ = b.register(1, 64, 10);
        let _ = b.register(2, 64, 10);
        let _ = b.register(3, 64, 10);
        let _ = b.request(1, 0);
        let _ = b.complete(1, 64, 10);
        let _ = b.request(2, 20);
        let _ = b.fail(2);
        let _ = b.request(3, 30);
        let n = b.counters();
        let ok = b.ledger_balanced() && n.0 == 3 && n.1 == 1 && n.2 == 1 && b.in_flight() == 1;
        chk(&mut set, "C87-18 账平衡", ok, "账不平");
    }

    // C87-19 未知块一律拒绝（不隐式登记）
    {
        let mut b = StreamBinder::new();
        let a = b.request(7, 0);
        let c = b.complete(7, 64, 0);
        let d = b.evict(7, 0);
        let e = b.set_pinned(7, true);
        let ok = a == Err(XC_UNKNOWN_TILE)
            && c == Err(XC_UNKNOWN_TILE)
            && d == Err(XC_UNKNOWN_TILE)
            && e == Err(XC_UNKNOWN_TILE)
            && b.counters().0 == 0;
        chk(&mut set, "C87-19 未知块拒绝", ok, "隐式登记未知块");
    }

    // C87-20 可见性行可读（五段齐备）
    {
        let b = StreamBinder::new();
        let line = b.visibility_line();
        let ok = line.contains("全 ") && line.contains("半 ") && line.contains("四 ") && line.contains("占位") && line.contains("预取");
        chk(&mut set, "C87-20 可见性可读", ok, "读屏字段缺失");
    }

    set
}

/// 单点演练。
pub fn drill() -> (bool, String) {
    let mut b = StreamBinder::new();
    let mut ok = true;
    let mut s = String::new();
    if b.register(1, 1024, 100).is_err() {
        ok = false;
        s.push_str("reg ");
    }
    if !matches!(b.request(1, 0), Ok(Req::Staged { .. })) {
        ok = false;
        s.push_str("req ");
    }
    if b.complete(1, 1024, 10) != Ok(1024) {
        ok = false;
        s.push_str("done ");
    }
    if !b.tier_honest() || !b.ledger_balanced() {
        ok = false;
        s.push_str("invariant ");
    }
    if s.is_empty() {
        s.push_str("流式绑定正常：");
        s.push_str(&b.visibility_line());
    }
    (ok, s)
}
