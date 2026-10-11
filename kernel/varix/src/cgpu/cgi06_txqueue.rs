//! cgi06_txqueue — CGPU-F1285 传输队列管理（上传下载两条河，各有各的闸）。
//!
//! 判据原文（施工书锚点 CGPU-F1285）：传输队列：传输队列管理（上传队列/下
//! 载队列分列——队列深度/水位监控；队列背压（下游慢→上游限速——回压协议）；
//! 队列优先级插队（紧急传输插队语义）；队列遥测。判据：分列、背压、插队、
//! 遥测、判据。
//!
//! 职责定位：上传/下载双队列分列（各自深度、水位、独立闸门——一条河堵塞不
//! 淹另一条）+ 信用回压协议（credit 制：下游确认回款才放行新流量，水位过高
//! 信用收缩→上游限速；回压触发/解除双阈值防振荡）+ 优先级插队（Emergency
//! 直插队首，High 走快道；插队公平上限——连续插队超限强制队尾，防普通流量
//! 饿死）+ 全量遥测（入队/出队/拒收/插队/背压计数+水位峰值）。与
//! cgi04_uploaddl（上传下载语义）、cgi03_bwaccount（带宽账户）分工：彼管
//! "传什么/花多少"，此管"怎么排队/何时叫停"。诊断码段 0x5201~0x5205。

use alloc::vec::Vec;

/// 模块版本。
pub const TX_QUEUE_VERSION: &str = "I06-txqueue-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x52 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_BACKPRESSURE_REJECT: u32 = 0x5201; // 背压拒收（信用耗尽）
pub const XC_PREEMPT_STARVE_GUARD: u32 = 0x5202; // 插队饿死保护触发（强制队尾）
pub const XC_WATER_CRITICAL: u32 = 0x5203; // 临界水位（监控告警面）
pub const XC_QUEUE_MISDIRECT: u32 = 0x5204; // 方向错投（上传项入了下载队列）
pub const XC_CREDIT_OVERFLOW: u32 = 0x5205; // 信用回充越上限（协议违约）

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 单队列容量（条目数）。
pub const QUEUE_DEPTH: usize = 64;
/// 高水位（占比×100）：回压触发线。
pub const WATER_HIGH_PCT: u32 = 75;
/// 临界水位：告警线（只告警不拒收——拒收由信用机制决定）。
pub const WATER_CRITICAL_PCT: u32 = 90;
/// 回压解除水位（滞回区间 55%~75% 防振荡）。
pub const WATER_RELEASE_PCT: u32 = 55;
/// 初始信用额度（条目数）：上游可无 ACK 连续入队的量。
pub const CREDIT_INIT: u32 = 16;
/// 信用下限（背压全停）：0 即完全停摆，1 保底探测流。
pub const CREDIT_FLOOR: u32 = 1;
/// 插队公平上限：连续插队 N 次后强制队尾（防饿死）。
pub const PREEMPT_FAIR_CAP: u32 = 4;

/// 传输方向（判据"分列"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Upload,
    Download,
}

impl Direction {
    pub const fn zh(self) -> &'static str {
        match self {
            Direction::Upload => "上传",
            Direction::Download => "下载",
        }
    }
}

/// 优先级三级（判据"插队"语义基础）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Prio {
    Normal,
    High,
    Emergency,
}

/// 传输项。
#[derive(Clone, Copy, Debug)]
pub struct TxItem {
    /// 项 ID（全局单调）。
    pub id: u64,
    pub dir: Direction,
    pub prio: Prio,
    /// 载荷字节数（遥测口径）。
    pub bytes: u32,
    /// 是否插队入位。
    pub preempted: bool,
}

/// 单向队列（分列实体）。
pub struct UniQueue {
    pub dir: Direction,
    slots: Vec<TxItem>,
    /// 信用余额。
    credit: u32,
    /// 当前是否处于背压状态（滞回状态机）。
    pressurized: bool,
    /// 连续插队计数（公平上限）。
    preempt_run: u32,
    /// 水位峰值。
    peak_depth: usize,
    /// 遥测。
    pub enqueued: u64,
    pub dequeued: u64,
    pub rejected_by_bp: u64,
    pub preempt_count: u64,
    pub starve_guards: u64,
    pub critical_events: u64,
}

impl UniQueue {
    pub fn new(dir: Direction) -> Self {
        UniQueue {
            dir,
            slots: Vec::with_capacity(QUEUE_DEPTH),
            credit: CREDIT_INIT,
            pressurized: false,
            preempt_run: 0,
            peak_depth: 0,
            enqueued: 0,
            dequeued: 0,
            rejected_by_bp: 0,
            preempt_count: 0,
            starve_guards: 0,
            critical_events: 0,
        }
    }

    /// 当前深度。
    pub const fn depth(&self) -> usize {
        self.slots.len()
    }

    /// 水位百分比（×100 定点）。
    pub fn water_pct(&self) -> u32 {
        (self.slots.len() as u32 * 10_000) / QUEUE_DEPTH as u32
    }

    /// 信用余额。
    pub const fn credit(&self) -> u32 {
        self.credit
    }

    /// 背压状态。
    pub const fn pressurized(&self) -> bool {
        self.pressurized
    }

    /// 入队（含背压判定与优先级插队语义）。
    pub fn enqueue(&mut self, item: TxItem) -> Result<(), u32> {
        // 队列物理满：无论信用，直接拒。
        if self.slots.len() >= QUEUE_DEPTH {
            self.rejected_by_bp += 1;
            return Err(XC_BACKPRESSURE_REJECT);
        }
        // 信用门禁（背压协议核心）：余额耗尽即限速。
        if self.credit == 0 {
            self.rejected_by_bp += 1;
            self.pressurized = true; // 拒收即明确进入背压
            return Err(XC_BACKPRESSURE_REJECT);
        }
        self.credit -= 1;

        // 临界水位告警（只告警不拒——拒收权在信用）。
        if self.water_pct() >= WATER_CRITICAL_PCT {
            self.critical_events += 1;
        }

        // 插队语义：Emergency 直插队首；High 插在 Normal 之前；
        // 公平上限：连续插队超限则强制队尾（饿死保护）。
        let is_preempt = item.prio == Prio::Emergency || item.prio == Prio::High;
        if is_preempt && self.preempt_run < PREEMPT_FAIR_CAP && self.slots.len() > 0 {
            let mut insert_at = self.slots.len();
            // Emergency 跳过所有非 Emergency；High 跳过 Normal。
            let mut i = 0;
            while i < self.slots.len() {
                let behind = self.slots[i].prio;
                let ahead_of = match item.prio {
                    Prio::Emergency => behind != Prio::Emergency,
                    Prio::High => behind == Prio::Normal,
                    Prio::Normal => false,
                };
                if ahead_of {
                    insert_at = i;
                    break;
                }
                i += 1;
            }
            let mut it = item;
            it.preempted = true;
            self.slots.insert(insert_at, it);
            self.preempt_count += 1;
            self.preempt_run += 1;
            if self.preempt_run == PREEMPT_FAIR_CAP {
                self.starve_guards += 1; // 本项已是最后一次插队，下次强制队尾
            }
        } else {
            let mut it = item;
            if is_preempt && self.preempt_run >= PREEMPT_FAIR_CAP {
                // 饿死保护：本次插队请求被降级为队尾。
                self.starve_guards += 1;
            }
            it.preempted = false;
            self.slots.push(it);
            self.preempt_run = 0; // 普通入队重置插队连计
        }

        self.enqueued += 1;
        if self.slots.len() > self.peak_depth {
            self.peak_depth = self.slots.len();
        }
        // 滞回回压判定：未加压且过高位→加压；已加压且回落到位→解除。
        if !self.pressurized && self.water_pct() >= WATER_HIGH_PCT {
            self.pressurized = true;
        } else if self.pressurized && self.water_pct() <= WATER_RELEASE_PCT {
            self.pressurized = false;
        }
        Ok(())
    }

    /// 出队（下游取走一项）。
    pub fn dequeue(&mut self) -> Option<TxItem> {
        if self.slots.len() == 0 {
            return None;
        }
        let item = self.slots.remove(0);
        self.dequeued += 1;
        // 滞回解除判定随出队更新。
        if self.pressurized && self.water_pct() <= WATER_RELEASE_PCT {
            self.pressurized = false;
        } else if !self.pressurized && self.water_pct() >= WATER_HIGH_PCT {
            self.pressurized = true;
        }
        Some(item)
    }

    /// 下游确认：回充信用（回压协议的"回款"动作）。
    pub fn ack(&mut self, units: u32) -> Result<(), u32> {
        let cap = CREDIT_INIT;
        let newc = self.credit + units;
        if newc > cap {
            self.credit = cap;
            return Err(XC_CREDIT_OVERFLOW); // 超额 ACK 是协议违约——回充到上限但报码
        }
        self.credit = newc;
        Ok(())
    }

    /// 背压降级：信用钳到下限（下游显性减速信号）。
    pub fn throttle(&mut self) {
        if self.credit > CREDIT_FLOOR {
            self.credit = CREDIT_FLOOR;
        }
        self.pressurized = true;
    }
}

/// 双列队列仓（判据"分列"总门面）。
pub struct TxQueueManager {
    up: UniQueue,
    down: UniQueue,
    /// 项 ID 发号。
    next_id: u64,
    /// 错投计数。
    pub misdirect_count: u64,
}

impl TxQueueManager {
    pub fn new() -> Self {
        TxQueueManager {
            up: UniQueue::new(Direction::Upload),
            down: UniQueue::new(Direction::Download),
            next_id: 1,
            misdirect_count: 0,
        }
    }

    /// 提交传输项（方向自动分列）。
    pub fn submit(&mut self, dir: Direction, prio: Prio, bytes: u32) -> Result<(Direction, u64), u32> {
        let item = TxItem {
            id: self.next_id,
            dir,
            prio,
            bytes,
            preempted: false,
        };
        self.next_id += 1;
        let r = match dir {
            Direction::Upload => self.up.enqueue(item),
            Direction::Download => self.down.enqueue(item),
        };
        match r {
            Ok(()) => Ok((dir, item.id)),
            Err(code) => Err(code),
        }
    }

    /// 下游取项（方向分列）。
    pub fn pop(&mut self, dir: Direction) -> Option<TxItem> {
        match dir {
            Direction::Upload => self.up.dequeue(),
            Direction::Download => self.down.dequeue(),
        }
    }

    /// 下游确认（方向分列）。
    pub fn acknowledge(&mut self, dir: Direction, units: u32) -> Result<(), u32> {
        match dir {
            Direction::Upload => self.up.ack(units),
            Direction::Download => self.down.ack(units),
        }
    }

    /// 水位查询（方向分列——监控面）。
    pub fn water(&self, dir: Direction) -> (usize, u32, bool, u32) {
        let q = match dir {
            Direction::Upload => &self.up,
            Direction::Download => &self.down,
        };
        (q.depth(), q.water_pct(), q.pressurized(), q.credit())
    }

    /// 单列可变访问（遥测导出面）。
    pub fn lane(&self, dir: Direction) -> &UniQueue {
        match dir {
            Direction::Upload => &self.up,
            Direction::Download => &self.down,
        }
    }

    /// 错投修正：把项从 A 列强制改投 B 列（协议侧纠错路径）。
    pub fn redirect(&mut self, item: TxItem, to: Direction) -> Result<u64, u32> {
        if item.dir == to {
            self.misdirect_count += 1;
            return Err(XC_QUEUE_MISDIRECT); // 无错可纠（原方向即目标）——错报也要计数
        }
        let mut it = item;
        it.dir = to;
        // ID 不变（审计连续性——纠错不改号）。
        match to {
            Direction::Upload => self.up.enqueue(it),
            Direction::Download => self.down.enqueue(it),
        }?;
        Ok(it.id)
    }
}

// ---------------------------------------------------------------------------
// 自检（判据五组：分列 / 背压 / 插队 / 遥测 / 水位滞回）
// ---------------------------------------------------------------------------

impl TxQueueManager {
    /// 判据组一：分列——上传满不连坐下载；各自独立深度。
    pub fn selfcheck_separate(&mut self) -> Result<(), u32> {
        let mut m = TxQueueManager::new();
        // 上传灌到物理满。
        let mut i = 0;
        while i < QUEUE_DEPTH {
            m.submit(Direction::Upload, Prio::Normal, 1024)?;
            i += 1;
        }
        // 上传再投应拒。
        let r1 = m.submit(Direction::Upload, Prio::Normal, 1024);
        if r1 != Err(XC_BACKPRESSURE_REJECT) {
            return Err(XC_BACKPRESSURE_REJECT);
        }
        // 下载列完全不受影响。
        m.submit(Direction::Download, Prio::Normal, 2048)?;
        let (d_dn, _, _, _) = m.water(Direction::Download);
        if d_dn != 1 {
            return Err(XC_QUEUE_MISDIRECT);
        }
        let (d_up, _, _, _) = m.water(Direction::Upload);
        if d_up != QUEUE_DEPTH {
            return Err(XC_QUEUE_MISDIRECT);
        }
        Ok(())
    }

    /// 判据组二：背压——信用耗尽拒收、ACK 回款恢复、滞回区间。
    pub fn selfcheck_backpressure(&mut self) -> Result<(), u32> {
        let mut m = TxQueueManager::new();
        // 灌入 CREDIT_INIT 项（信用耗尽）。
        let mut i = 0;
        while i < CREDIT_INIT as usize {
            m.submit(Direction::Upload, Prio::Normal, 512)?;
            i += 1;
        }
        let r = m.submit(Direction::Upload, Prio::Normal, 512);
        if r != Err(XC_BACKPRESSURE_REJECT) {
            return Err(XC_BACKPRESSURE_REJECT);
        }
        // ACK 回款 4 项 → 可再入 4 项。
        m.acknowledge(Direction::Upload, 4)?;
        let mut j = 0;
        while j < 4 {
            m.submit(Direction::Upload, Prio::Normal, 512)?;
            j += 1;
        }
        let r2 = m.submit(Direction::Upload, Prio::Normal, 512);
        if r2 != Err(XC_BACKPRESSURE_REJECT) {
            return Err(XC_BACKPRESSURE_REJECT);
        }
        // 超额 ACK 违约报码但钳在上限。
        let r3 = m.acknowledge(Direction::Upload, 999);
        if r3 != Err(XC_CREDIT_OVERFLOW) || m.lane(Direction::Upload).credit() != CREDIT_INIT {
            return Err(XC_CREDIT_OVERFLOW);
        }
        Ok(())
    }

    /// 判据组三：插队——Emergency 超越 Normal，公平上限强制队尾。
    pub fn selfcheck_preempt(&mut self) -> Result<(), u32> {
        let mut m = TxQueueManager::new();
        // 3 个 Normal 先行。
        let mut i = 0;
        while i < 3 {
            m.submit(Direction::Download, Prio::Normal, 100)?;
            i += 1;
        }
        // 1 个 Emergency 插队。
        m.submit(Direction::Download, Prio::Emergency, 100)?;
        // 出队序：Emergency 第一个。
        let first = m.pop(Direction::Download);
        match first {
            Some(it) => {
                if it.prio != Prio::Emergency || !it.preempted {
                    return Err(XC_PREEMPT_STARVE_GUARD);
                }
            }
            None => return Err(XC_PREEMPT_STARVE_GUARD),
        }
        // 公平上限：连投 PREEMPT_FAIR_CAP 个 High 后，下一个 High 被迫队尾。
        let mut m2 = TxQueueManager::new();
        let _ = m2.submit(Direction::Upload, Prio::Normal, 1);
        let mut k = 0;
        while k < PREEMPT_FAIR_CAP {
            let _ = m2.submit(Direction::Upload, Prio::High, 1)?;
            k += 1;
        }
        // 第 5 个 High：应降级队尾（在 Normal 之后最后）。
        let _ = m2.submit(Direction::Upload, Prio::High, 1)?;
        // 出队序：High×4 → Normal → High（降级）。
        let mut order: [Prio; 6] = [Prio::Normal; 6];
        let mut n = 0;
        while n < 6 {
            match m2.pop(Direction::Upload) {
                Some(it) => order[n] = it.prio,
                None => return Err(XC_PREEMPT_STARVE_GUARD),
            }
            n += 1;
        }
        if order[4] != Prio::Normal || order[5] != Prio::High {
            return Err(XC_PREEMPT_STARVE_GUARD);
        }
        if m2.lane(Direction::Upload).starve_guards == 0 {
            return Err(XC_PREEMPT_STARVE_GUARD);
        }
        Ok(())
    }

    /// 判据组四：遥测——计数链完整（入队/出队/拒收/插队）。
    pub fn selfcheck_telemetry(&mut self) -> Result<(), u32> {
        let mut m = TxQueueManager::new();
        let mut i = 0;
        while i < 5 {
            m.submit(Direction::Upload, Prio::Normal, 10)?;
            i += 1;
        }
        let _ = m.submit(Direction::Upload, Prio::Emergency, 10)?; // 插队
        let mut j = 0;
        while j < 6 {
            let _ = m.pop(Direction::Upload);
            j += 1;
        }
        let q = m.lane(Direction::Upload);
        if q.enqueued != 6 || q.dequeued != 6 || q.preempt_count != 1 {
            return Err(XC_BACKPRESSURE_REJECT);
        }
        // 空列 pop 返回 None 不计错。
        if m.pop(Direction::Upload).is_some() {
            return Err(XC_QUEUE_MISDIRECT);
        }
        Ok(())
    }

    /// 判据组五：水位滞回——加压/解除不振荡。
    pub fn selfcheck_hysteresis(&mut self) -> Result<(), u32> {
        let mut m = TxQueueManager::new();
        // 信用足够：48 项 ≈ 75% 触发加压。
        let mut i = 0;
        while i < 48 {
            m.submit(Direction::Download, Prio::Normal, 8)?;
            i += 1;
        }
        let (_, w1, bp1, _) = m.water(Direction::Download);
        if w1 < WATER_HIGH_PCT || !bp1 {
            return Err(XC_WATER_CRITICAL);
        }
        // 解除阈值之上小幅出队不应解除（滞回）。
        let mut j = 0;
        while j < 4 {
            let _ = m.pop(Direction::Download);
            j += 1;
        }
        let (_, w2, bp2, _) = m.water(Direction::Download);
        if w2 > WATER_RELEASE_PCT && w2 < WATER_HIGH_PCT {
            // 在滞回区间内必须保持加压。
            if !bp2 {
                return Err(XC_WATER_CRITICAL);
            }
        }
        // 出队到解除线以下才解除。
        let mut k = 0;
        while k < 40 {
            let _ = m.pop(Direction::Download);
            k += 1;
        }
        let (_, _, bp3, _) = m.water(Direction::Download);
        if bp3 {
            return Err(XC_WATER_CRITICAL);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_separate()?;
        self.selfcheck_backpressure()?;
        self.selfcheck_preempt()?;
        self.selfcheck_telemetry()?;
        self.selfcheck_hysteresis()?;
        Ok(())
    }
}
