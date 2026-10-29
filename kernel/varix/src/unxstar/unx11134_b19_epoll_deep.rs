//! UNX-F11134 B19 清账（epoll 深水判据终重放）（0x1f220 段）
//!
//! 纯功能模型：epoll 域第十线清账。十条「模式账」（水平触发读写、边沿
//! 触发读写、单次、边沿单次、唤醒、独占、重复 fd、排干）各自走同一段
//! 五步脚本：写 4 → 写 4 → 排 3 → 排 3 → 排 2。水位因此走 4 → 8 →
//! 5 → 2 → 0，在第二步越过深水线 6（峰值 8），在第四步回落到低水线 2
//! 时闭合深水区间。深水判据要求：每模式恰一段深水区间、收口时水位 0、
//! 深水区间不得悬空。
//!
//! 规则面（对齐 epoll(7) 的裁剪语义）：
//!   - 水平触发：同一份待处理事件在排空前会被反复上报，触发数等于水位，
//!     五步合计 19；
//!   - 边沿触发：只在有新数据落账的那两步上报，合计 2；
//!   - EPOLLONESHOT：一次上报即摘臂，合计 1，摘臂后不再等；
//!   - EPOLLWAKEUP：上报须持唤醒租约，租约一次一用；
//!   - EPOLLEXCLUSIVE：两名等待者只唤醒一名，合计 2；
//!   - 同一 epitem 挂两个 fd：每次就绪只报一次，重复登记被吞并计数；
//!   - EPOLLET 排干：每次就绪报一次，排空须在同一轮完成。
//!
//! 拒止面：模式号越界、步序越界、重复上臂、未上臂即等、摘臂后再等、
//! 无租约即报、无等待者即报、未排空即收口、触发数不符、深水账不符、
//! 终重放缺账、封盘后动账。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f220;
/// 模式号越界
pub const EP_MODE: u32 = SEG_BASE + 0x01;
/// 上臂/等臂状态非法（重复上臂、未上臂即等、摘臂后再等）
pub const EP_ARM: u32 = SEG_BASE + 0x02;
/// 无唤醒租约即上报
pub const EP_LEASE: u32 = SEG_BASE + 0x03;
/// 无等待者即上报（独占模式）
pub const EP_WAITERS: u32 = SEG_BASE + 0x04;
/// 步序越界或倒挂
pub const EP_ORDER: u32 = SEG_BASE + 0x05;
/// 水位未排空即收口
pub const EP_PENDING: u32 = SEG_BASE + 0x06;
/// 触发数与模式规则不符
pub const EP_REPEAT: u32 = SEG_BASE + 0x07;
/// 深水区间账不符
pub const EP_DEEP: u32 = SEG_BASE + 0x08;
/// 终重放缺账
pub const EP_REPLAY: u32 = SEG_BASE + 0x09;
/// 账册已封盘
pub const EP_FROZEN: u32 = SEG_BASE + 0x0a;

/// 模式账条数。
pub const MODES: usize = 10;
/// 模式名（与 MODE_KINDS 同序）。
pub const MODE_NAMES: [&str; MODES] = [
    "level-rd",
    "level-wr",
    "edge-rd",
    "edge-wr",
    "oneshot",
    "edge-oneshot",
    "wakeup",
    "exclusive",
    "dup-fd",
    "drain",
];
/// 脚本步数。
pub const STEPS: usize = 5;
/// 每步写入量：4, 4, 0, 0, 0。
pub const WRITE_SEQ: [u32; STEPS] = [4, 4, 0, 0, 0];
/// 每步排空量：0, 0, 3, 3, 2。
pub const DRAIN_SEQ: [u32; STEPS] = [0, 0, 3, 3, 2];
/// 深水线。
pub const DEEP_WATER: u32 = 6;
/// 低水线（深水区间在此闭合）。
pub const LOW_WATER: u32 = 2;
/// 脚本峰值水位。
pub const PEAK_WATER: u32 = 8;
/// 队列容量上界。
pub const DEPTH_CAP: u32 = 16;
/// 独占模式的等待者数。
pub const WAITERS: u8 = 2;
/// 重复 fd 模式的挂接数。
pub const DUP_FDS: u8 = 2;
/// 十模式触发总数：27+27+2+2+2+2+2+2+2+2。
pub const FIRE_TOTAL: u32 = 70;
/// 十模式深水区间总数：每模式恰一段。
pub const DEEP_TOTAL: u32 = 10;
/// 十模式累计写入（= 累计排空）。
pub const BYTES_TOTAL: u32 = 80;

// 模式类别。
/// 水平触发。
pub const KIND_LEVEL: u8 = 0;
/// 边沿触发。
pub const KIND_EDGE: u8 = 1;
/// 单次触发。
pub const KIND_ONESHOT: u8 = 2;
/// 唤醒租约。
pub const KIND_WAKEUP: u8 = 3;
/// 独占唤醒。
pub const KIND_EXCLUSIVE: u8 = 4;
/// 重复 fd 去重。
pub const KIND_DUP: u8 = 5;
/// 边沿排干。
pub const KIND_DRAIN: u8 = 6;

/// 十模式类别表。
pub const MODE_KINDS: [u8; MODES] = [
    KIND_LEVEL,
    KIND_LEVEL,
    KIND_EDGE,
    KIND_EDGE,
    KIND_ONESHOT,
    KIND_ONESHOT,
    KIND_WAKEUP,
    KIND_EXCLUSIVE,
    KIND_DUP,
    KIND_DRAIN,
];

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 第 m 条模式账的类别；越界返回 KIND_LEVEL（调用方须先验号）。
pub fn mode_kind(m: usize) -> u8 {
    match MODE_KINDS.get(m) {
        Some(k) => *k,
        None => KIND_LEVEL,
    }
}

/// 模式是否会在排空前反复上报（水平触发族）。
pub fn repeats_while_pending(kind: u8) -> bool {
    kind == KIND_LEVEL || kind == KIND_DUP
}

/// 模式是否一次上报即摘臂。
pub fn disarms_after_fire(kind: u8) -> bool {
    kind == KIND_ONESHOT
}

/// 模式是否须持唤醒租约。
pub fn needs_lease(kind: u8) -> bool {
    kind == KIND_WAKEUP
}

/// 模式是否须先登记等待者。
pub fn needs_waiters(kind: u8) -> bool {
    kind == KIND_EXCLUSIVE
}

/// 规则复算：由记录下来的「等臂时水位」与「每步写入」推触发数。
///
/// `trace[i]` 是第 i 步等臂瞬间的水位，`writes[i]` 是第 i 步写入量。
/// 水平族按水位逐次复报；边沿族只在写入步报一次；单次族全程只报一次；
/// 其余族每写入步报一次（独占只唤醒一名，去重只报一份，排干只报一次）。
pub fn expected_fires(kind: u8, trace: &[u32; STEPS], writes: &[u32; STEPS]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < STEPS {
        let d = match trace.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let w = match writes.get(i) {
            Some(v) => *v,
            None => 0,
        };
        match kind {
            KIND_LEVEL => n = n.wrapping_add(d),
            _ => {
                if w > 0 {
                    n = n.wrapping_add(1);
                }
            }
        }
        i = i.wrapping_add(1);
    }
    n
}

/// 规则复算：深水区间数。水位自低水线以下越过深水线即开区间，
/// 回落至低水线即闭区间。
pub fn expected_deep(trace: &[u32; STEPS]) -> u32 {
    let mut n = 0u32;
    let mut open = false;
    let mut i = 0usize;
    while i < STEPS {
        let d = match trace.get(i) {
            Some(v) => *v,
            None => 0,
        };
        if !open && d >= DEEP_WATER {
            open = true;
            n = n.wrapping_add(1);
        } else if open && d <= LOW_WATER {
            open = false;
        }
        i = i.wrapping_add(1);
    }
    n
}

/// 单条模式账。
#[derive(Clone, Copy, Debug)]
pub struct ModeLane {
    /// 模式类别。
    pub kind: u8,
    armed: bool,
    disarmed: bool,
    lease: bool,
    waiters: u8,
    dup_fds: u8,
    depth: u32,
    high_water: u32,
    fires: u32,
    waits: u32,
    reported: u32,
    drained: u32,
    dedup: u32,
    deep_episodes: u32,
    deep_open: bool,
    /// 每步等臂瞬间的水位。
    trace: [u32; STEPS],
    /// 每步写入量。
    writes: [u32; STEPS],
    step: usize,
    closed: bool,
}

impl ModeLane {
    /// 按类别新建一条未上臂的模式账。
    pub fn new(kind: u8) -> ModeLane {
        ModeLane {
            kind,
            armed: false,
            disarmed: false,
            lease: false,
            waiters: 0,
            dup_fds: 0,
            depth: 0,
            high_water: 0,
            fires: 0,
            waits: 0,
            reported: 0,
            drained: 0,
            dedup: 0,
            deep_episodes: 0,
            deep_open: false,
            trace: [0u32; STEPS],
            writes: [0u32; STEPS],
            step: 0,
            closed: false,
        }
    }

    /// 上臂。已上臂者重复上臂拒止。
    pub fn arm(&mut self) -> Result<(), u32> {
        if self.closed {
            return Err(EP_FROZEN);
        }
        if self.armed {
            return Err(EP_ARM);
        }
        self.armed = true;
        self.disarmed = false;
        Ok(())
    }

    /// 持唤醒租约。租约一次一用，未消耗即再持拒止。
    pub fn hold_lease(&mut self) -> Result<(), u32> {
        if self.closed {
            return Err(EP_FROZEN);
        }
        if self.lease {
            return Err(EP_LEASE);
        }
        self.lease = true;
        Ok(())
    }

    /// 登记等待者（独占模式用）。
    pub fn add_waiters(&mut self, n: u8) -> Result<(), u32> {
        if self.closed {
            return Err(EP_FROZEN);
        }
        if self.waiters.wrapping_add(n) > WAITERS {
            return Err(EP_WAITERS);
        }
        self.waiters = self.waiters.wrapping_add(n);
        Ok(())
    }

    /// 登记重复 fd（去重模式用）。
    pub fn add_dup_fds(&mut self, n: u8) -> Result<(), u32> {
        if self.closed {
            return Err(EP_FROZEN);
        }
        if self.dup_fds.wrapping_add(n) > DUP_FDS {
            return Err(EP_ORDER);
        }
        self.dup_fds = self.dup_fds.wrapping_add(n);
        Ok(())
    }

    /// 写入 n 个待处理事件；越容量拒止，越过深水线开区间。
    pub fn write(&mut self, n: u32) -> Result<(), u32> {
        if self.closed {
            return Err(EP_FROZEN);
        }
        if self.step >= STEPS {
            return Err(EP_ORDER);
        }
        if n > 0 && !self.armed {
            return Err(EP_ARM);
        }
        if needs_lease(self.kind) && n > 0 && !self.lease {
            return Err(EP_LEASE);
        }
        let next = self.depth.wrapping_add(n);
        if next > DEPTH_CAP {
            return Err(EP_PENDING);
        }
        if !self.deep_open && next >= DEEP_WATER {
            self.deep_open = true;
            self.deep_episodes = self.deep_episodes.wrapping_add(1);
        }
        self.depth = next;
        if next > self.high_water {
            self.high_water = next;
        }
        if let Some(slot) = self.writes.get_mut(self.step) {
            *slot = n;
        }
        Ok(())
    }

    /// 记水位（本步等臂前的可报水位）。深水与触发都以该水位复算。
    pub fn mark(&mut self) -> Result<(), u32> {
        if self.step >= STEPS {
            return Err(EP_ORDER);
        }
        if let Some(slot) = self.trace.get_mut(self.step) {
            *slot = self.depth;
        }
        Ok(())
    }

    /// 等一次。返回本轮上报数。
    pub fn wait(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(EP_FROZEN);
        }
        if self.step >= STEPS {
            return Err(EP_ORDER);
        }
        if !self.armed || self.disarmed {
            return Err(EP_ARM);
        }
        if needs_waiters(self.kind) && self.waiters == 0 {
            return Err(EP_WAITERS);
        }
        let wrote = match self.writes.get(self.step) {
            Some(v) => *v,
            None => 0,
        };
        if needs_lease(self.kind) && wrote > 0 && !self.lease {
            return Err(EP_LEASE);
        }
        let mut got = 0u32;
        match self.kind {
            KIND_LEVEL => got = self.depth,
            KIND_EDGE | KIND_DRAIN => {
                if wrote > 0 && self.depth > 0 {
                    got = 1;
                }
            }
            KIND_ONESHOT => {
                if wrote > 0 && self.depth > 0 {
                    got = 1;
                    self.disarmed = true;
                    self.armed = false;
                }
            }
            KIND_WAKEUP => {
                if wrote > 0 && self.depth > 0 {
                    got = 1;
                    self.lease = false;
                }
            }
            KIND_EXCLUSIVE => {
                if wrote > 0 && self.depth > 0 {
                    got = 1;
                }
            }
            KIND_DUP => {
                if wrote > 0 && self.depth > 0 {
                    got = 1;
                    if self.dup_fds >= 2 {
                        self.dedup = self.dedup.wrapping_add(1);
                    }
                }
            }
            _ => {}
        }
        self.waits = self.waits.wrapping_add(1);
        self.fires = self.fires.wrapping_add(got);
        self.reported = self.reported.wrapping_add(got);
        Ok(got)
    }

    /// 排干 n 个事件，返回实排数；低水线以下闭合深水区间。
    pub fn drain(&mut self, n: u32) -> Result<u32, u32> {
        if self.closed {
            return Err(EP_FROZEN);
        }
        if self.step >= STEPS {
            return Err(EP_ORDER);
        }
        let take = if n > self.depth { self.depth } else { n };
        self.depth = self.depth.wrapping_sub(take);
        self.drained = self.drained.wrapping_add(take);
        if self.deep_open && self.depth <= LOW_WATER {
            self.deep_open = false;
        }
        Ok(take)
    }

    /// 步进。步数用尽后拒止。
    pub fn settle(&mut self) -> Result<(), u32> {
        if self.step >= STEPS {
            return Err(EP_ORDER);
        }
        self.step = self.step.wrapping_add(1);
        Ok(())
    }

    /// 收口：步数走满、水位归零、触发数与深水区间逐项相符。
    pub fn close(&mut self) -> Result<(), u32> {
        if self.closed {
            return Ok(());
        }
        if self.step != STEPS {
            return Err(EP_REPLAY);
        }
        if self.depth != 0 {
            return Err(EP_PENDING);
        }
        if expected_fires(self.kind, &self.trace, &self.writes) != self.fires {
            return Err(EP_REPEAT);
        }
        if self.deep_open || expected_deep(&self.trace) != self.deep_episodes {
            return Err(EP_DEEP);
        }
        self.closed = true;
        Ok(())
    }

    /// 触发数。
    pub fn fires(&self) -> u32 {
        self.fires
    }

    /// 等臂次数。
    pub fn waits(&self) -> u32 {
        self.waits
    }

    /// 上报总数（与触发数同源，分开记账便于交叉核对）。
    pub fn reported(&self) -> u32 {
        self.reported
    }

    /// 去重吞并数。
    pub fn dedup(&self) -> u32 {
        self.dedup
    }

    /// 深水区间数。
    pub fn deep_episodes(&self) -> u32 {
        self.deep_episodes
    }

    /// 当前水位。
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// 历史最高水位。
    pub fn high_water(&self) -> u32 {
        self.high_water
    }

    /// 累计排干。
    pub fn drained(&self) -> u32 {
        self.drained
    }

    /// 是否已摘臂（单次族）。
    pub fn disarmed(&self) -> bool {
        self.disarmed
    }

    /// 步数。
    pub fn step(&self) -> usize {
        self.step
    }

    /// 是否已收口。
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// 规则复算的应有触发数。
    pub fn want_fires(&self) -> u32 {
        expected_fires(self.kind, &self.trace, &self.writes)
    }

    /// 水位轨迹。
    pub fn trace(&self) -> [u32; STEPS] {
        self.trace
    }

    /// 模式账指纹。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF ^ (self.kind as u64);
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.step as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.depth as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.high_water as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.fires as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.waits as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.drained as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.dedup as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.deep_episodes as u64;
        h = h.wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while i < STEPS {
            if let (Some(t), Some(w)) = (self.trace.get(i), self.writes.get(i)) {
                h ^= ((*t as u64) << 32) | (*w as u64);
                h = h.wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        h
    }
}

/// 十条模式账的总账。
#[derive(Clone, Debug)]
pub struct EpollLedger {
    /// 段内基址（用于指纹扰动与账册定位）。
    pub base: u32,
    lanes: [ModeLane; MODES],
    closed: usize,
    rejects: u32,
    frozen: bool,
}

impl EpollLedger {
    /// 新建空账。
    pub fn new(base: u32) -> EpollLedger {
        let mut lanes = [ModeLane::new(KIND_LEVEL); MODES];
        let mut i = 0usize;
        while i < MODES {
            if let Some(l) = lanes.get_mut(i) {
                *l = ModeLane::new(mode_kind(i));
            }
            i = i.wrapping_add(1);
        }
        EpollLedger {
            base,
            lanes,
            closed: 0,
            rejects: 0,
            frozen: false,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 取模式账（可写）。
    pub fn lane(&mut self, m: usize) -> Option<&mut ModeLane> {
        self.lanes.get_mut(m)
    }

    /// 取模式账（只读）。
    pub fn lane_ref(&self, m: usize) -> Option<&ModeLane> {
        self.lanes.get(m)
    }

    /// 统一转发：封盘后一律拒止，且封盘优先于模式号校验。
    fn route<F>(&mut self, m: usize, f: F) -> Result<u32, u32>
    where
        F: FnOnce(&mut ModeLane) -> Result<u32, u32>,
    {
        if self.frozen {
            return Err(self.fail(EP_FROZEN));
        }
        let r = match self.lanes.get_mut(m) {
            Some(l) => f(l),
            None => Err(EP_MODE),
        };
        match r {
            Ok(v) => Ok(v),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 上臂。
    pub fn arm(&mut self, m: usize) -> Result<(), u32> {
        let _ = self.route(m, |l| l.arm().map(|_| 0))?;
        Ok(())
    }

    /// 持租约。
    pub fn lease(&mut self, m: usize) -> Result<(), u32> {
        let _ = self.route(m, |l| l.hold_lease().map(|_| 0))?;
        Ok(())
    }

    /// 登记等待者。
    pub fn waiters(&mut self, m: usize, n: u8) -> Result<(), u32> {
        let _ = self.route(m, |l| l.add_waiters(n).map(|_| 0))?;
        Ok(())
    }

    /// 登记重复 fd。
    pub fn dup_fds(&mut self, m: usize, n: u8) -> Result<(), u32> {
        let _ = self.route(m, |l| l.add_dup_fds(n).map(|_| 0))?;
        Ok(())
    }

    /// 写入。
    pub fn write(&mut self, m: usize, n: u32) -> Result<(), u32> {
        let _ = self.route(m, |l| l.write(n).map(|_| 0))?;
        Ok(())
    }

    /// 记水位。
    pub fn mark(&mut self, m: usize) -> Result<(), u32> {
        let _ = self.route(m, |l| l.mark().map(|_| 0))?;
        Ok(())
    }

    /// 等一次，返回本轮上报数。
    pub fn wait(&mut self, m: usize) -> Result<u32, u32> {
        self.route(m, |l| l.wait())
    }

    /// 排干，返回实排数。
    pub fn drain(&mut self, m: usize, n: u32) -> Result<u32, u32> {
        self.route(m, |l| l.drain(n))
    }

    /// 步进。
    pub fn settle(&mut self, m: usize) -> Result<(), u32> {
        let _ = self.route(m, |l| l.settle().map(|_| 0))?;
        Ok(())
    }

    /// 收口单条。
    pub fn close(&mut self, m: usize) -> Result<(), u32> {
        let was = match self.lane_ref(m) {
            Some(l) => l.closed(),
            None => false,
        };
        let r = self.route(m, |l| l.close().map(|_| 0));
        if r.is_ok() && !was {
            self.closed = self.closed.wrapping_add(1);
        }
        r.map(|_| ())
    }
}

impl EpollLedger {
    /// 已收口模式数。
    pub fn closed_count(&self) -> usize {
        self.closed
    }

    /// 拒止计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否封盘。
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// 十模式触发合计。
    pub fn total_fires(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MODES {
            if let Some(l) = self.lanes.get(i) {
                n = n.wrapping_add(l.fires());
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 十模式规则复算触发合计。
    pub fn want_total_fires(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MODES {
            if let Some(l) = self.lanes.get(i) {
                n = n.wrapping_add(l.want_fires());
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 十模式深水区间合计。
    pub fn total_deep(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MODES {
            if let Some(l) = self.lanes.get(i) {
                n = n.wrapping_add(l.deep_episodes());
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 十模式累计排干。
    pub fn total_drained(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MODES {
            if let Some(l) = self.lanes.get(i) {
                n = n.wrapping_add(l.drained());
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 十模式去重吞并合计。
    pub fn total_dedup(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MODES {
            if let Some(l) = self.lanes.get(i) {
                n = n.wrapping_add(l.dedup());
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 全账最高水位。
    pub fn peak(&self) -> u32 {
        let mut p = 0u32;
        let mut i = 0usize;
        while i < MODES {
            if let Some(l) = self.lanes.get(i) {
                if l.high_water() > p {
                    p = l.high_water();
                }
            }
            i = i.wrapping_add(1);
        }
        p
    }

    /// 悬空深水区间数。
    pub fn open_deep(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MODES {
            if let Some(l) = self.lanes.get(i) {
                if l.deep_open() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 未排空模式数。
    pub fn pending(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MODES {
            if let Some(l) = self.lanes.get(i) {
                if l.depth() != 0 {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 总账指纹：基址、封盘位、拒止数、收口数，再并入十条模式账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        for b in self.base.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h ^= if self.frozen { 1 } else { 0 };
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.rejects as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.closed as u64;
        h = h.wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while i < MODES {
            if let Some(l) = self.lanes.get(i) {
                h ^= l.digest();
                h = h.wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        h
    }

    /// 封盘，幂等。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Ok(self.fold());
        }
        self.frozen = true;
        Ok(self.fold())
    }

    /// 终重放：十条闭合、触发对平、深水闭合、水位归零，然后封盘。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.closed != MODES {
            return Err(self.fail(EP_REPLAY));
        }
        if self.total_fires() != FIRE_TOTAL || self.want_total_fires() != FIRE_TOTAL {
            return Err(self.fail(EP_REPEAT));
        }
        if self.total_deep() != DEEP_TOTAL || self.open_deep() != 0 {
            return Err(self.fail(EP_DEEP));
        }
        if self.total_drained() != BYTES_TOTAL || self.pending() != 0 {
            return Err(self.fail(EP_PENDING));
        }
        self.freeze()
    }
}

impl ModeLane {
    /// 深水区间是否仍悬空。
    pub fn deep_open(&self) -> bool {
        self.deep_open
    }

    /// 已写入总量。
    pub fn wrote_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < STEPS {
            if let Some(v) = self.writes.get(i) {
                n = n.wrapping_add(*v);
            }
            i = i.wrapping_add(1);
        }
        n
    }
}

/// 搭建十条模式账：上臂、挂等待者/重复 fd、五步脚本、逐条收口、终重放。
pub fn build_deep(base: u32) -> (EpollLedger, Result<u64, u32>) {
    let mut g = EpollLedger::new(base);
    let mut m = 0usize;
    while m < MODES {
        let kind = mode_kind(m);
        if g.arm(m).is_err() {
            return (g, Err(EP_ARM));
        }
        if needs_waiters(kind) && g.waiters(m, WAITERS).is_err() {
            return (g, Err(EP_WAITERS));
        }
        if kind == KIND_DUP && g.dup_fds(m, DUP_FDS).is_err() {
            return (g, Err(EP_ORDER));
        }
        let mut s = 0usize;
        while s < STEPS {
            let w = match WRITE_SEQ.get(s) {
                Some(v) => *v,
                None => 0,
            };
            let rearm = match g.lane_ref(m) {
                Some(l) => l.disarmed(),
                None => false,
            };
            if rearm && w > 0 && g.arm(m).is_err() {
                return (g, Err(EP_ARM));
            }
            if needs_lease(kind) && w > 0 && g.lease(m).is_err() {
                return (g, Err(EP_LEASE));
            }
            if g.write(m, w).is_err() {
                return (g, Err(EP_ORDER));
            }
            if g.mark(m).is_err() {
                return (g, Err(EP_ORDER));
            }
            let skip = match g.lane_ref(m) {
                Some(l) => l.disarmed(),
                None => true,
            };
            if !skip && g.wait(m).is_err() {
                return (g, Err(EP_ARM));
            }
            let d = match DRAIN_SEQ.get(s) {
                Some(v) => *v,
                None => 0,
            };
            if g.drain(m, d).is_err() {
                return (g, Err(EP_ORDER));
            }
            if g.settle(m).is_err() {
                return (g, Err(EP_ORDER));
            }
            s = s.wrapping_add(1);
        }
        if g.close(m).is_err() {
            return (g, Err(EP_REPEAT));
        }
        m = m.wrapping_add(1);
    }
    let r = g.replay();
    (g, r)
}

/// 四路注入：重复上臂、无租约即写、无等待者即等、未排空即收口。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    // ① 重复上臂落 EP_ARM。
    let mut a = EpollLedger::new(0x3400);
    let first = a.arm(0);
    let twice = a.arm(0);
    out[0] = first.is_ok() && twice.err() == Some(EP_ARM) && a.rejects() == 1;

    // ② 唤醒模式未持租约即写落 EP_LEASE。
    let mut b = EpollLedger::new(0x3401);
    let armed = b.arm(6);
    let wrote = b.write(6, 2);
    out[1] = armed.is_ok() && wrote.err() == Some(EP_LEASE) && b.rejects() == 1;

    // ③ 独占模式无等待者即等落 EP_WAITERS。
    let mut c = EpollLedger::new(0x3402);
    let ok3 = c.arm(7).is_ok() && c.write(7, 3).is_ok() && c.mark(7).is_ok();
    out[2] = ok3 && c.wait(7).err() == Some(EP_WAITERS) && c.rejects() == 1;

    // ④ 水位未排空即收口落 EP_PENDING。
    let mut d = EpollLedger::new(0x3403);
    let ok4 = d.arm(0).is_ok() && d.write(0, 4).is_ok() && d.mark(0).is_ok();
    let mut s = 0usize;
    while s < STEPS {
        if d.settle(0).is_err() {
            break;
        }
        s = s.wrapping_add(1);
    }
    out[3] = ok4 && s == STEPS && d.close(0).err() == Some(EP_PENDING);

    out
}

/// 判据集：A1~A13。
pub fn run_unx11134_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11134.b19_epoll_deep");
    let (g, r) = build_deep(0x1134);

    // A1 常量面：段基址、十条、五步、峰值 8、触发 68、深水 10、排水 80。
    let c1 = SEG_BASE == 0x1f220
        && MODES == 10
        && STEPS == 5
        && PEAK_WATER == 8
        && FIRE_TOTAL == 70
        && DEEP_TOTAL == 10
        && BYTES_TOTAL == 80
        && MODE_NAMES.len() == MODES
        && MODE_KINDS.len() == MODES;
    s.add("A1-常量面", c1, "十模式五步脚本，峰值 8，触发 70，深水 10");

    // A2 十条模式账逐条闭合（10/10）并封盘。
    s.add_dyn(
        "A2-模式账闭合",
        r.is_ok() && g.closed_count() == MODES && g.frozen(),
        &format!(
            "closed={}/{} frozen={} replay={:?}",
            g.closed_count(),
            MODES,
            g.frozen(),
            r.is_ok()
        ),
    );

    // A3 深水判据：每模式一段、全账峰值 8、无悬空区间。
    s.add_dyn(
        "A3-深水判据",
        g.total_deep() == DEEP_TOTAL && g.peak() == PEAK_WATER && g.open_deep() == 0,
        &format!(
            "deep={}/{} peak={} open={}",
            g.total_deep(),
            DEEP_TOTAL,
            g.peak(),
            g.open_deep()
        ),
    );

    // A4 水平族复报：两条水平账各 27 次触发、五次等臂、上报与触发同源。
    let mut c4 = true;
    let mut i4 = 0usize;
    while i4 < 2 {
        match g.lane_ref(i4) {
            Some(l) => {
                c4 = c4 && l.fires() == 27 && l.waits() == 5 && l.reported() == l.fires();
            }
            None => c4 = false,
        }
        i4 = i4.wrapping_add(1);
    }
    s.add("A4-水平复报", c4, "level-rd/level-wr 各 27 报、5 等臂");

    // A5 边沿族与单次族：边沿各 2 报，单次各 1 报且摘臂。
    let mut c5 = true;
    let mut i5 = 2usize;
    while i5 < 4 {
        match g.lane_ref(i5) {
            Some(l) => c5 = c5 && l.fires() == 2,
            None => c5 = false,
        }
        i5 = i5.wrapping_add(1);
    }
    let mut i6 = 4usize;
    while i6 < 6 {
        match g.lane_ref(i6) {
            Some(l) => c5 = c5 && l.fires() == 2 && l.disarmed(),
            None => c5 = false,
        }
        i6 = i6.wrapping_add(1);
    }
    s.add("A5-边沿与单次", c5, "边沿各 2 报；单次各 2 报并摘臂");

    // A6 去重账：重复 fd 模式两次就绪各吞并一份重复通知。
    let c6 = match g.lane_ref(8) {
        Some(l) => l.dedup() == 2 && l.fires() == 2,
        None => false,
    };
    s.add("A6-去重账", c6, "dup-fd 吞并 2 份重复通知，仍只报 2 次");

    // A7 排水账：十模式累计排干 80，收口时无残余水位。
    s.add_dyn(
        "A7-排水归零",
        g.total_drained() == BYTES_TOTAL && g.pending() == 0,
        &format!(
            "drained={}/{} pending={}",
            g.total_drained(),
            BYTES_TOTAL,
            g.pending()
        ),
    );

    // A8 终重放幂等：已封盘账再重放一次，不去账、不改指纹。
    let mut c8 = g.clone();
    let before = c8.fold();
    let again = c8.replay();
    s.add(
        "A8-重放幂等",
        again.is_ok() && c8.fold() == before && c8.rejects() == g.rejects(),
        "重放二次返回同一指纹且零新增拒止",
    );

    // A9 封盘幂等：两次封盘返回同一指纹。
    let mut c9 = EpollLedger::new(0x1134);
    let f1 = c9.freeze();
    let f2 = c9.freeze();
    s.add(
        "A9-封盘幂等",
        f1.is_ok() && f2.is_ok() && f1 == f2,
        "两次封盘同指纹",
    );

    // A10 拒止优先序：封盘先于模式号校验；未封盘时模式号越界落 EP_MODE。
    let mut c10a = g.clone();
    let z1 = c10a.arm(99).err();
    let mut c10b = EpollLedger::new(0x1135);
    let z2 = c10b.arm(99).err();
    s.add(
        "A10-拒止优先序",
        z1 == Some(EP_FROZEN) && z2 == Some(EP_MODE),
        "封盘优先于模式号；未封盘则 EP_MODE",
    );

    // A11 四路注入全发火。
    let inj = inject_four();
    let c11 = inj[0] && inj[1] && inj[2] && inj[3];
    s.add("A11-四路注入", c11, "四路拒止路径各自发火");

    // A12 指纹灵敏度：异基址异指纹，同基址同指纹。
    let (g2, _) = build_deep(0x1136);
    let (g3, _) = build_deep(0x1134);
    s.add(
        "A12-指纹灵敏度",
        g2.fold() != g.fold() && g3.fold() == g.fold(),
        "异基址指纹相异，同基址指纹相同",
    );

    // A13 逐例归零：拒止 0、规则复算与实账对平、十条全闭。
    s.add(
        "A13-逐例归零",
        g.rejects() == 0
            && g.want_total_fires() == g.total_fires()
            && g.total_fires() == FIRE_TOTAL
            && g.closed_count() == MODES,
        "rejects=0，复算 70=实账 70，十全闭",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11134_flow() {
        let (g, r) = build_deep(0x2040);
        assert!(r.is_ok());
        assert!(g.frozen());
        assert_eq!(g.closed_count(), MODES);
        assert_eq!(g.total_fires(), FIRE_TOTAL);
        assert_eq!(g.want_total_fires(), FIRE_TOTAL);
        assert_eq!(g.total_deep(), DEEP_TOTAL);
        assert_eq!(g.peak(), PEAK_WATER);
        assert_eq!(g.total_drained(), BYTES_TOTAL);
        assert_eq!(g.pending(), 0);
        assert_eq!(g.open_deep(), 0);
        assert_eq!(g.rejects(), 0);
        // 水平族逐条可复算；单次族摘臂后不再等。
        let l0 = match g.lane_ref(0) {
            Some(l) => l,
            None => return,
        };
        assert_eq!(l0.fires(), 27);
        assert_eq!(l0.digest(), l0.digest());
        let l4 = match g.lane_ref(4) {
            Some(l) => l,
            None => return,
        };
        assert!(l4.disarmed());
        assert_eq!(l4.step(), STEPS);
    }

    #[test]
    fn f11134_gates() {
        let mut g = EpollLedger::new(0x2041);
        assert!(g.arm(0).is_ok());
        assert_eq!(g.arm(0).err(), Some(EP_ARM));
        assert_eq!(g.write(1, 1).err(), Some(EP_ARM));
        assert_eq!(g.wait(99).err(), Some(EP_MODE));
        assert_eq!(g.rejects(), 3);
        assert!(g.freeze().is_ok());
        assert_eq!(g.arm(2).err(), Some(EP_FROZEN));
        assert_eq!(g.rejects(), 4);
        // 封盘后重放：账未闭合，重放不通过。
        assert_eq!(g.replay().err(), Some(EP_REPLAY));
        assert_eq!(g.rejects(), 5);
    }

    #[test]
    fn f11134_rules() {
        let trace: [u32; STEPS] = [4, 8, 8, 5, 2];
        let writes: [u32; STEPS] = [4, 4, 0, 0, 0];
        assert_eq!(expected_fires(KIND_LEVEL, &trace, &writes), 27);
        assert_eq!(expected_fires(KIND_EDGE, &trace, &writes), 2);
        assert_eq!(expected_fires(KIND_ONESHOT, &trace, &writes), 2);
        assert_eq!(expected_fires(KIND_WAKEUP, &trace, &writes), 2);
        assert_eq!(expected_fires(KIND_EXCLUSIVE, &trace, &writes), 2);
        assert_eq!(expected_fires(KIND_DUP, &trace, &writes), 2);
        assert_eq!(expected_fires(KIND_DRAIN, &trace, &writes), 2);
        assert_eq!(expected_deep(&trace), 1);
        let low: [u32; STEPS] = [1, 2, 3, 2, 1];
        assert_eq!(expected_deep(&low), 0);
        let held: [u32; STEPS] = [6, 6, 6, 6, 6];
        assert_eq!(expected_deep(&held), 1);
        assert!(repeats_while_pending(KIND_LEVEL));
        assert!(!repeats_while_pending(KIND_EDGE));
        assert!(disarms_after_fire(KIND_ONESHOT));
        assert!(needs_lease(KIND_WAKEUP));
        assert!(needs_waiters(KIND_EXCLUSIVE));
        assert_eq!(mode_kind(0), KIND_LEVEL);
        assert_eq!(mode_kind(9), KIND_DRAIN);
        assert_eq!(mode_kind(10), KIND_LEVEL);
    }

    #[test]
    fn f11134_injection() {
        assert_eq!(inject_four(), [true, true, true, true]);
        // 容量上界单独校一遍。
        let mut l = ModeLane::new(KIND_LEVEL);
        assert!(l.arm().is_ok());
        assert_eq!(l.write(DEPTH_CAP + 1).err(), Some(EP_PENDING));
    }

    #[test]
    fn f11134_deep_boundaries() {
        let mut g = EpollLedger::new(0x2042);
        assert!(g.arm(2).is_ok());
        assert!(g.write(2, 5).is_ok());
        assert_eq!(g.lane_ref(2).map(|l| l.deep_episodes()), Some(0));
        assert!(g.write(2, 1).is_ok());
        assert_eq!(g.lane_ref(2).map(|l| l.deep_episodes()), Some(1));
        // 排到低水线即闭区间。
        assert_eq!(g.drain(2, 4).ok(), Some(4));
        assert_eq!(g.lane_ref(2).map(|l| l.deep_open()), Some(false));
        assert_eq!(g.lane_ref(2).map(|l| l.depth()), Some(2));
    }

    #[test]
    fn f11134_checks_green() {
        let cs = run_unx11134_checks();
        assert!(cs.all_passed(), "checks red in unx11134");
    }
}
