//! perfkit — perfstar 十七域共用底盘（AI-K1 深化批次三 · 共用件）。
//!
//! 存在的理由（不是抽象冲动，是主册反复点名的四条共同纪律）：
//!
//! | 共用件 | 主册依据（原文摘） | 消费域 |
//! | --- | --- | --- |
//! | [`DiagSink`] 诊断报备环 | F045「策略决策日志**入诊断快照（F174）**」、F048 同、F042「归因器自身异常 → 静默停用 + **诊断报备**（不拖累合成器）」、F052「碎片率 >25% → **告警 + 归因**」 | F041/F042/F045/F048/F050/F052/F057 |
//! | [`Retention`] 保留期与轮转 | F042「保留 **7 天**」、F043「保留最近 **20 次**启动」、F044「上限 **8MB**/应用，LRU 驱逐」、F041「**24 小时**分钟聚合」、F050「合并统计入账本」 | F041/F042/F043/F044/F052/F057 |
//! | [`KnobTable`] 旋钮清单 | F048「全策略参数**进旋钮清单**（无隐藏魔法数）」、F046「档位判定依据……（MD2 附录 K 存储栈旋钮）」、通用十二查第 9 查「无新增魔法数（调优走旋钮清单）」 | F046/F047/F048/F050/F052/F057 |
//! | [`SecRing`] 60 秒逐秒环 | F046「最近 **60 秒**实际写入量曲线」、F047「最近 **60 秒**各类 p99 曲线」、F057「各队列**吞吐曲线**」、F049「纯桌面静置 **60 秒**」 | F045/F046/F049/F050/F057 |
//!
//! 零堆纪律同全域：全部定长数组，`const fn` 构造，无 alloc 进内核路径。
//! 本件不产 CheckSet——它是被十七域消费的底盘，正确性由消费域的检查项与
//! 本文件宿主单测共同把关（避免「为凑检查项而设无锚检查项」）。

// ---------------------------------------------------------------------------
// 1. 诊断报备环 DiagSink
// ---------------------------------------------------------------------------

/// 诊断环容量：十七域共用一条时间轴，64 条足够承载一次快照窗口（F174 口径
/// 「采集最近 30s 数据窗口」——三十秒内十七域同时报备也不会互相挤掉）。
pub const DIAG_RING: usize = 64;
/// 单条目说明文字字节数（定长，UTF-8 截断在字节边界由调用侧保证）。
pub const DIAG_MSG: usize = 48;

/// 严重度：与 F174 诊断快照四级口径对齐（info/warn/error/fatal）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagSev {
    Info = 0,
    Warn = 1,
    Error = 2,
    Fatal = 3,
}

/// 一条诊断报备。参数 `a/b` 是数值证据（不留字符串化：定长 + 零堆）。
#[derive(Clone, Copy, Debug)]
pub struct DiagEntry {
    /// 报备域（如 `"F045"`）。
    pub domain: &'static str,
    /// 域内错误码（各域自定，域内唯一）。
    pub code: u16,
    /// 时刻（毫秒钟，全域同源）。
    pub at_ms: u64,
    pub sev: DiagSev,
    /// 数值证据一（语义由 code 定义）。
    pub a: u64,
    /// 数值证据二。
    pub b: u64,
    msg: [u8; DIAG_MSG],
    msg_len: u8,
}

impl DiagEntry {
    /// 空条目（消费侧数组初始化用——定长结构的通用零值，不是「有效事实」）。
    pub const fn empty() -> Self {
        DiagEntry { domain: "", code: 0, at_ms: 0, sev: DiagSev::Info, a: 0, b: 0, msg: [0; DIAG_MSG], msg_len: 0 }
    }
    /// 说明文字（可能短于请求长度——定长缓冲，超长截断不 panic）。
    pub fn msg(&self) -> &[u8] {
        &self.msg[..self.msg_len as usize]
    }
    /// 严重度是否达到「需要用户看见」的门槛（F042「静默停用不拖累合成器」的
    /// 反面：fatal 不得静默——十三·补 异常零静默纪律）。
    pub fn needs_user(&self) -> bool {
        matches!(self.sev, DiagSev::Error | DiagSev::Fatal)
    }
}

/// 诊断报备环（定长、零堆、覆盖最旧——报备不丢最新事实）。
pub struct DiagSink {
    ring: [Option<DiagEntry>; DIAG_RING],
    head: usize,
    filled: usize,
    counts: [u32; 4],
    last_fatal_ms: Option<u64>,
    /// 因环满被挤掉的条数（诚实计数——不静默吞）。
    dropped: u32,
}

impl DiagSink {
    pub const fn new() -> Self {
        DiagSink {
            ring: [None; DIAG_RING],
            head: 0,
            filled: 0,
            counts: [0; 4],
            last_fatal_ms: None,
            dropped: 0,
        }
    }

    /// 报备一条。`msg` 超长按字节截断（定长纪律，不 panic、不分配）。
    pub fn push(
        &mut self,
        domain: &'static str,
        code: u16,
        at_ms: u64,
        sev: DiagSev,
        a: u64,
        b: u64,
        msg: &[u8],
    ) {
        let n = msg.len().min(DIAG_MSG);
        let mut buf = [0u8; DIAG_MSG];
        buf[..n].copy_from_slice(&msg[..n]);
        let e = DiagEntry { domain, code, at_ms, sev, a, b, msg: buf, msg_len: n as u8 };
        if self.filled == DIAG_RING {
            self.dropped += 1;
        }
        self.ring[self.head] = Some(e);
        self.head = (self.head + 1) % DIAG_RING;
        self.filled = (self.filled + 1).min(DIAG_RING);
        self.counts[sev as usize] += 1;
        if sev == DiagSev::Fatal {
            self.last_fatal_ms = Some(at_ms);
        }
    }

    /// 快照导出（时间升序：最旧 → 最新，供 F174 诊断快照与总日志中心消费）。
    pub fn snapshot(&self, out: &mut [DiagEntry]) -> usize {
        let n = self.filled.min(out.len());
        let start = (self.head + DIAG_RING - n) % DIAG_RING;
        for i in 0..n {
            if let Some(e) = self.ring[(start + i) % DIAG_RING] {
                out[i] = e;
            }
        }
        n
    }

    /// 指定严重度的累计条数。
    pub fn count(&self, sev: DiagSev) -> u32 {
        self.counts[sev as usize]
    }

    /// 最近一次 fatal 时刻（无则 None——「没有 fatal」是可回答的）。
    pub fn last_fatal_ms(&self) -> Option<u64> {
        self.last_fatal_ms
    }

    /// 被挤掉条数（环满覆盖计数）。
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// 清空（诊断快照导出后由 F174 调用；不复用旧事实）。
    pub fn clear(&mut self) {
        for slot in self.ring.iter_mut() {
            *slot = None;
        }
        self.head = 0;
        self.filled = 0;
        self.counts = [0; 4];
        self.dropped = 0;
    }
}

// ---------------------------------------------------------------------------
// 2. 保留期与轮转 Retention
// ---------------------------------------------------------------------------

/// 准入裁定结果：调用侧按裁定执行（本件只裁定，不替调用侧改数据结构——
/// 一处一事实，策略与存储分离）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetentionVerdict {
    /// 直接准入（容量与保留期均未满）。
    Admit,
    /// 需先按保留期过期 `n` 条（最旧的 n 条已超龄）。
    Expire(u32),
    /// 需先按 LRU/最旧驱逐 `n` 条（容量上限）。
    Evict(u32),
}

/// 保留期策略（条数上限 + 时长上限，二者独立生效）。
///
/// 主册口径举例：F042「保留 7 天」→ `new(0, 7*24*3600*1000)`；
/// F043「保留最近 20 次」→ `new(20, 0)`；F041「24 小时分钟聚合」→
/// `new(1440, 24*3600*1000)`。
#[derive(Clone, Copy, Debug)]
pub struct Retention {
    /// 条数上限（0 = 不限条数）。
    pub cap: u32,
    /// 保留时长毫秒（0 = 不限时长）。
    pub keep_ms: u64,
    /// 累计过期条数（调参依据，入账本）。
    pub expired: u64,
    /// 累计驱逐条数。
    pub evicted: u64,
}

impl Retention {
    pub const fn new(cap: u32, keep_ms: u64) -> Self {
        Retention { cap, keep_ms, expired: 0, evicted: 0 }
    }

    /// 准入裁定。`count` 为当前条数，`oldest_ms` 为最旧条目时刻（无条目传 `now_ms`）。
    pub fn admit(&mut self, now_ms: u64, count: u32, oldest_ms: u64) -> RetentionVerdict {
        // 先判过期：过期优先于驱逐（过期的条目本来就该走）。
        if self.keep_ms > 0 && count > 0 {
            let age = now_ms.saturating_sub(oldest_ms);
            if age > self.keep_ms {
                // 保守裁一条（调用侧逐条推进，避免一次性扫描未知数量的条目）。
                self.expired += 1;
                return RetentionVerdict::Expire(1);
            }
        }
        if self.cap > 0 && count >= self.cap {
            self.evicted += 1;
            return RetentionVerdict::Evict(1);
        }
        RetentionVerdict::Admit
    }

    /// 二分式批量裁定：一次性问「要腾出多少条」——调用侧已知全部条目时刻时
    /// 用这个（避免 N 次往返）。`ages` 自最旧到最新升序。
    pub fn reclaim(&mut self, now_ms: u64, count: u32, oldest_ms: u64) -> u32 {
        let mut need = 0u32;
        if self.cap > 0 && count >= self.cap {
            need = need.max(count + 1 - self.cap);
        }
        if self.keep_ms > 0 && count > 0 {
            let age = now_ms.saturating_sub(oldest_ms);
            if age > self.keep_ms {
                need = need.max(1);
            }
        }
        self.evicted += need as u64;
        need
    }
}

// ---------------------------------------------------------------------------
// 3. 旋钮清单 KnobTable
// ---------------------------------------------------------------------------

/// 单域旋钮上限（十七域各自一张表，32 个足够覆盖「全策略参数」）。
pub const KNOB_MAX: usize = 32;
/// 旋钮名长度上限（定长，零堆）。
pub const KNOB_NAME: usize = 32;

/// 一个可调参数（主册「无隐藏魔法数」的落实形态：每个数都有名字、范围、单位）。
#[derive(Clone, Copy, Debug)]
pub struct Knob {
    pub name: [u8; KNOB_NAME],
    pub name_len: u8,
    /// 当前值（i64 统一口径：定点值由 unit 说明量纲）。
    pub value: i64,
    pub min: i64,
    pub max: i64,
    /// 单位串（如 `"ms"`/`"permille"`/`"req/s"`），定长。
    pub unit: [u8; 8],
    pub unit_len: u8,
}

impl Knob {
    pub fn name(&self) -> &[u8] {
        &self.name[..self.name_len as usize]
    }
    pub fn unit(&self) -> &[u8] {
        &self.unit[..self.unit_len as usize]
    }
    /// 值是否在 [min,max] 内（钳制前的自检口径）。
    pub fn in_range(&self) -> bool {
        self.value >= self.min && self.value <= self.max
    }
}

fn copy_str(dst: &mut [u8], src: &str) -> u8 {
    let b = src.as_bytes();
    let n = b.len().min(dst.len());
    dst[..n].copy_from_slice(&b[..n]);
    n as u8
}

/// 旋钮表（域内单例；登记 → 可调 → 可审计导出三件套）。
pub struct KnobTable {
    knobs: [Option<Knob>; KNOB_MAX],
    n: usize,
    /// 越界写入被钳制的次数（审计：谁想越过边界）。
    clamped: u32,
    /// 未登记名查询次数（审计：有没有人偷偷读不存在的旋钮）。
    unknown: u32,
}

impl KnobTable {
    pub const fn new() -> Self {
        KnobTable { knobs: [None; KNOB_MAX], n: 0, clamped: 0, unknown: 0 }
    }

    /// 登记一个旋钮（同名重复登记 = 更新范围与值，不新增条目——一处一事实）。
    pub fn register(&mut self, name: &str, value: i64, min: i64, max: i64, unit: &str) -> bool {
        if let Some(i) = self.find(name) {
            if let Some(k) = self.knobs[i].as_mut() {
                k.value = value.clamp(min, max);
                k.min = min;
                k.max = max;
                k.unit_len = copy_str(&mut k.unit, unit);
                return true;
            }
        }
        if self.n >= KNOB_MAX {
            return false;
        }
        let mut k = Knob {
            name: [0u8; KNOB_NAME],
            name_len: 0,
            value: value.clamp(min, max),
            min,
            max,
            unit: [0u8; 8],
            unit_len: 0,
        };
        k.name_len = copy_str(&mut k.name, name);
        k.unit_len = copy_str(&mut k.unit, unit);
        self.knobs[self.n] = Some(k);
        self.n += 1;
        true
    }

    fn find(&self, name: &str) -> Option<usize> {
        let b = name.as_bytes();
        for i in 0..self.n {
            if let Some(k) = self.knobs[i] {
                if k.name() == b {
                    return Some(i);
                }
            }
        }
        None
    }

    /// 调值（越界钳制并计数，不静默接受越界值）。
    pub fn set(&mut self, name: &str, v: i64) -> bool {
        match self.find(name) {
            Some(i) => {
                if let Some(k) = self.knobs[i].as_mut() {
                    let c = v.clamp(k.min, k.max);
                    if c != v {
                        self.clamped += 1;
                    }
                    k.value = c;
                    true
                } else {
                    false
                }
            }
            None => {
                self.unknown += 1;
                false
            }
        }
    }

    pub fn get(&self, name: &str) -> Option<i64> {
        self.find(name).and_then(|i| self.knobs[i].map(|k| k.value))
    }

    /// 审计导出（设置中心「旋钮清单」页与诊断快照共用一份）。
    pub fn audit(&self, out: &mut [Knob]) -> usize {
        let n = self.n.min(out.len());
        for i in 0..n {
            if let Some(k) = self.knobs[i] {
                out[i] = k;
            }
        }
        n
    }

    /// 全部旋钮是否都在范围内（自检项：旋钮表自身不许被写坏）。
    pub fn all_in_range(&self) -> bool {
        (0..self.n).all(|i| self.knobs[i].map(|k| k.in_range()).unwrap_or(true))
    }

    pub fn clamped(&self) -> u32 {
        self.clamped
    }
    pub fn unknown(&self) -> u32 {
        self.unknown
    }
    pub fn len(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// 4. 60 秒逐秒环 SecRing
// ---------------------------------------------------------------------------

/// 60 个秒槽（主册多处「最近 60 秒」口径统一到这一个数，不再各域自造）。
pub const SEC_SLOTS: usize = 60;

/// 逐秒计数环：写入按秒归并，滑出窗自动清零（诚实：不把旧秒当新秒）。
#[derive(Clone, Copy, Debug)]
pub struct SecRing {
    slots: [u64; SEC_SLOTS],
    /// 当前秒序号（秒 = 毫秒/1000），用于判定翻页与滑出。
    sec: u64,
    /// 是否有过写入（区分「零样本秒」与「从未开始」——F047「某类长期零样本
    /// → 观察窗说明（不代表无风险）」的判断依据）。
    started: bool,
}

impl SecRing {
    pub const fn new() -> Self {
        SecRing { slots: [0u64; SEC_SLOTS], sec: 0, started: false }
    }

    fn roll(&mut self, now_ms: u64) {
        let s = now_ms / 1_000;
        if !self.started {
            self.sec = s;
            self.started = true;
            return;
        }
        let gap = s.saturating_sub(self.sec);
        if gap == 0 {
            return;
        }
        if gap >= SEC_SLOTS as u64 {
            // 整个窗滑出：全部清零（旧秒不是新秒）。
            self.slots = [0u64; SEC_SLOTS];
        } else {
            for _ in 0..gap {
                self.sec += 1;
                self.slots[(self.sec as usize) % SEC_SLOTS] = 0;
            }
        }
        self.sec = s;
    }

    /// 累加一次计数（同秒累加，跨秒翻页并清零新槽）。
    pub fn note(&mut self, now_ms: u64, delta: u64) {
        self.roll(now_ms);
        let i = (self.sec as usize) % SEC_SLOTS;
        self.slots[i] = self.slots[i].saturating_add(delta);
    }

    /// 取最近 60 秒序列（时间升序：最旧 → 最新；不足 60 秒的前段为 0）。
    pub fn series(&self, out: &mut [u64]) -> usize {
        let n = out.len().min(SEC_SLOTS);
        for i in 0..n {
            // 最新槽 = sec % 60；最旧槽 = (sec+1) % 60。
            let idx = (self.sec as usize + 1 + i) % SEC_SLOTS;
            out[i] = self.slots[idx];
        }
        n
    }

    /// 当前秒值（瞬时速率口径）。
    pub fn current(&self) -> u64 {
        self.slots[(self.sec as usize) % SEC_SLOTS]
    }

    /// 窗口内峰值。
    pub fn peak(&self) -> u64 {
        self.slots.iter().copied().max().unwrap_or(0)
    }

    /// 窗口内累计。
    pub fn sum(&self) -> u64 {
        self.slots.iter().copied().fold(0u64, u64::saturating_add)
    }

    /// 是否已有样本（零样本窗与未启动窗的区分面）。
    pub fn started(&self) -> bool {
        self.started
    }
}

// ---------------------------------------------------------------------------
// 共用单测（底盘正确性由本件自证，消费域不再重复验证底盘）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_ring_covers_oldest_and_counts_severity() {
        let mut d = DiagSink::new();
        d.push("F045", 1, 1_000, DiagSev::Info, 1, 2, b"decision");
        d.push("F045", 2, 2_000, DiagSev::Warn, 3, 4, b"pressure");
        d.push("F042", 3, 3_000, DiagSev::Fatal, 5, 6, b"disabled");
        assert_eq!(d.count(DiagSev::Info), 1);
        assert_eq!(d.count(DiagSev::Warn), 1);
        assert_eq!(d.count(DiagSev::Fatal), 1);
        assert_eq!(d.last_fatal_ms(), Some(3_000));
        let mut out = [DiagEntry { domain: "", code: 0, at_ms: 0, sev: DiagSev::Info, a: 0, b: 0, msg: [0; DIAG_MSG], msg_len: 0 }; 8];
        let n = d.snapshot(&mut out);
        assert_eq!(n, 3);
        // 时间升序
        assert_eq!(out[0].at_ms, 1_000);
        assert_eq!(out[2].at_ms, 3_000);
        assert!(out[2].needs_user());
        assert!(!out[0].needs_user());
    }

    #[test]
    fn diag_msg_truncates_without_panic() {
        let mut d = DiagSink::new();
        let long = [b'x'; 200];
        d.push("F052", 9, 0, DiagSev::Error, 0, 0, &long);
        let mut out = [DiagEntry { domain: "", code: 0, at_ms: 0, sev: DiagSev::Info, a: 0, b: 0, msg: [0; DIAG_MSG], msg_len: 0 }; 4];
        d.snapshot(&mut out);
        assert_eq!(out[0].msg().len(), DIAG_MSG);
    }

    #[test]
    fn diag_ring_full_counts_dropped() {
        let mut d = DiagSink::new();
        for i in 0..(DIAG_RING + 10) {
            d.push("F050", 1, i as u64, DiagSev::Info, 0, 0, b"x");
        }
        assert_eq!(d.dropped(), 10);
        assert_eq!(d.count(DiagSev::Info) as usize, DIAG_RING + 10);
    }

    #[test]
    fn retention_expire_precedes_evict() {
        // 7 天保留期（F042 口径）+ 无条数上限：超龄先过期。
        let mut r = Retention::new(0, 7 * 24 * 3600 * 1000);
        let now = 100 * 24 * 3600 * 1000;
        let oldest = now - 8 * 24 * 3600 * 1000;
        assert_eq!(r.admit(now, 5, oldest), RetentionVerdict::Expire(1));
        // 未超龄且无上限 → 准入
        let mut r2 = Retention::new(0, 7 * 24 * 3600 * 1000);
        assert_eq!(r2.admit(now, 5, now - 1000), RetentionVerdict::Admit);
    }

    #[test]
    fn retention_cap_evicts_when_full() {
        // F043「保留最近 20 次启动」
        let mut r = Retention::new(20, 0);
        assert_eq!(r.admit(1_000, 19, 0), RetentionVerdict::Admit);
        assert_eq!(r.admit(1_000, 20, 0), RetentionVerdict::Evict(1));
        assert_eq!(r.evicted, 1);
    }

    #[test]
    fn knob_table_clamps_and_audits() {
        let mut t = KnobTable::new();
        assert!(t.register("burst_ms", 50, 10, 200, "ms"));
        assert!(t.register("dwell_s", 5, 1, 30, "s"));
        assert_eq!(t.get("burst_ms"), Some(50));
        // 越界钳制并计数
        assert!(t.set("burst_ms", 9999));
        assert_eq!(t.get("burst_ms"), Some(200));
        assert_eq!(t.clamped(), 1);
        // 未登记名查询
        assert!(!t.set("nope", 1));
        assert_eq!(t.unknown(), 1);
        assert!(t.all_in_range());
        let mut out = [Knob { name: [0; KNOB_NAME], name_len: 0, value: 0, min: 0, max: 0, unit: [0; 8], unit_len: 0 }; 8];
        assert_eq!(t.audit(&mut out), 2);
        assert_eq!(out[0].name(), b"burst_ms");
        assert_eq!(out[0].unit(), b"ms");
    }

    #[test]
    fn knob_register_same_name_updates_in_place() {
        let mut t = KnobTable::new();
        t.register("x", 1, 0, 10, "");
        t.register("x", 5, 0, 10, "");
        assert_eq!(t.len(), 1);
        assert_eq!(t.get("x"), Some(5));
    }

    #[test]
    fn sec_ring_same_second_accumulates_and_rolls() {
        let mut s = SecRing::new();
        s.note(1_000, 5);
        s.note(1_500, 7);
        assert_eq!(s.current(), 12);
        // 跨到下一秒：新槽独立
        s.note(2_000, 3);
        assert_eq!(s.current(), 3);
        assert_eq!(s.sum(), 15);
        assert_eq!(s.peak(), 12);
    }

    #[test]
    fn sec_ring_whole_window_slide_clears() {
        let mut s = SecRing::new();
        s.note(1_000, 9);
        // 跳过 60 秒以上：整窗滑出清零（旧秒不是新秒）
        s.note(120_000, 1);
        assert_eq!(s.sum(), 1);
        let mut out = [0u64; 60];
        s.series(&mut out);
        assert_eq!(out[59], 1);
        assert_eq!(out[..59].iter().sum::<u64>(), 0);
    }

    #[test]
    fn sec_ring_series_is_time_ascending() {
        let mut s = SecRing::new();
        for i in 0..60u64 {
            s.note(i * 1_000, i);
        }
        let mut out = [0u64; 60];
        s.series(&mut out);
        // 最旧 = 0（已被最新一轮覆盖前的值）……实际：60 槽写满后最旧槽即 sec+1
        // 位置值；这里断言升序关系：out[59] 是最新秒 59 的值。
        assert_eq!(out[59], 59);
        assert_eq!(out[58], 58);
    }

    #[test]
    fn sec_ring_started_flag_distinguishes_zero_samples() {
        let mut s = SecRing::new();
        assert!(!s.started(), "未启动 ≠ 零样本");
        s.note(0, 0);
        assert!(s.started());
        assert_eq!(s.sum(), 0, "启动了但零样本");
    }
}
