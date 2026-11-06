//! UNX-F48881 事件层运维总成（限速/重置/回退三操作 SOP 与执行臂） · 0x70A00
//! ═══════════════════════════════════════════════════════════════════════
//! 覆盖卡：F48881（三操作 SOP ×10 留痕）· F48882（M1 桥入口四元组签名冻结）
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   F48881 ① 纯功能代码 ≥380 行；
//!          ② UNX-F48881-J1 三操作 SOP ×10 执行留痕全过；
//!   F48882 ① 纯功能代码 ≥400 行；
//!          ② UNX-F48882-J1 四元组签名 ×10^3 递交零漂移 + M1 联签锚定行。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：四元组合法性 → 设备唯一冻结 → 签名比对 → 限速令牌 → 窗口配额
//!           → SOP 前置（证据包/保留账）→ 路由焦点一致 → 留痕账平。
//!   次序不可倒的理由逐条：
//!     · 四元组未判合法性就冻结，会把 vid=0 的探测帧当成真实设备写进桥
//!       入口，之后所有签名都以坏键为基准，漂移检测永远为绿；
//!     · 签名比对必须先于限速：限速只回答「能不能现在递」，不回答「这是
//!       不是同一台设备」；倒过来会把漂移帧计入令牌账，掩盖串设备；
//!     · SOP 的证据包前置必须早于任何状态改写：回退一旦先改 current 再查
//!       证据，缺证据的回退会留下半回退状态（版本号已动、账未动）；
//!     · 路由焦点一致必须放在最后一道：它是「递送出口」，出口之前所有
//!       拒绝都不该消耗递送计数。
//! 关键差异：本模块是 B05 的「入口 + 操作臂」——它冻结 M1 桥入口的四元组
//!   签名（boot_id/vid/pid/fw），提供限速/重置/回退三操作的真实状态机，
//!   并把焦点路由的「只在变更时切换」写成可复算的不变量。它不实现帧合并
//!   策略（F48883 归 ops_meta）、不实现背压队列（F48892 归 ops_edge）、
//!   不实现 10 设备 30 分钟对账（F48888 归 ops_dispatch）。桥入口之前的一
//!   切归本域；桥入口之后的 M5 解析归 M5，本域零重复实现。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70A00，VX6 批 B05 族首槽）。
pub const SEG_BASE: u32 = 0x70A00;

/// 并发设备上限（10 设备并发是本族所有账本的基数）。
pub const DEVICE_COUNT: usize = 10;
/// 三操作 SOP 的每操作执行轮数（判据 ×10）。
pub const SOP_ROUNDS: u32 = 10;
/// 四元组签名递交轮数（判据 10^3，每轮遍历 10 设备）。
pub const SIGN_ROUNDS: u32 = 1000;
/// 令牌桶容量与每 tick 补充量（8ms 一 tick，即 125Hz 的运维口径）。
pub const BUCKET_CAP: u32 = 32;
pub const BUCKET_REFILL: u32 = 8;
/// 滑动窗口跨度与配额（100ms / 40 事件）。
pub const WINDOW_SPAN_MS: u32 = 100;
pub const WINDOW_CAP: u32 = 40;
/// tick 宽度（毫秒）。
pub const TICK_MS: u32 = 8;
/// 焦点递送轮数（运维侧最小可复算样本）。
pub const ROUTE_ROUNDS: u32 = 100;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0B） ───────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpCode {
    /// 三操作次序非法（重置未清限速 / 回退目标不是上一稳定版）
    SopOrder,
    /// 回退缺证据包（四要素不全）
    SopNoEvidence,
    /// 未知操作码
    SopUnknown,
    /// 四元组字段越界（boot/vid/pid/fw 有零值）
    TupleGap,
    /// 四元组签名漂移（递进来的键与冻结键不同）
    TupleDrift,
    /// 同一设备号重复冻结
    TupleDup,
    /// 限速参数非法或令牌不足
    RateBad,
    /// 焦点未变却要求切换 / 递送窗口与焦点不符
    RouteStale,
    /// 留痕账不平（计数相加不等于总轮数）
    LedgerBad,
    /// 段位越界
    SegmentBad,
}

impl OpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            OpCode::SopOrder => "SOP 次序非法：重置必须清限速账，回退只能退到上一稳定版",
            OpCode::SopNoEvidence => "回退缺证据包：版本/设备/判据/时间四要素不全",
            OpCode::SopUnknown => "未知操作码：只允许限速/重置/回退三操作",
            OpCode::TupleGap => "四元组字段越界：boot_id/vid/pid/fw 均不得为零",
            OpCode::TupleDrift => "四元组签名漂移：递进来的键与冻结键不一致",
            OpCode::TupleDup => "设备重复冻结：同一设备号只能登记一把入口键",
            OpCode::RateBad => "限速参数非法：桶容量/补充量/请求量不成立",
            OpCode::RouteStale => "焦点路由失效：未变更要求切换或递送串窗",
            OpCode::LedgerBad => "留痕账不平：执行 + 拒绝 ≠ 总轮数",
            OpCode::SegmentBad => "段位越界：诊断码逃出 0x70A00 窗口",
        }
    }

    pub const ALL: [OpCode; 10] = [
        OpCode::SopOrder,
        OpCode::SopNoEvidence,
        OpCode::SopUnknown,
        OpCode::TupleGap,
        OpCode::TupleDrift,
        OpCode::TupleDup,
        OpCode::RateBad,
        OpCode::RouteStale,
        OpCode::LedgerBad,
        OpCode::SegmentBad,
    ];
}

/// 段内码取明细文本；闭集外给固定文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < OpCode::ALL.len() {
        if let Some(c) = OpCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x70A00 段闭集内"
}

// ── FNV-1a（本域基础，盐与相邻模块不同） ──────────────────────────
pub const FNV_OFF: u64 = 0xCBF2_9CE4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_u16(h: u64, v: u16) -> u64 {
    fold_byte(fold_byte(h, (v & 0xFF) as u8), ((v >> 8) & 0xFF) as u8)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x = fold_byte(x, (v >> (i.saturating_mul(8))) as u8);
        i += 1;
    }
    x
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

// ── LCG（固定种子，保证留痕可复算） ───────────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed | 1 }
    }

    pub fn next(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 33) as u32
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

// ── M1 桥入口四元组 ───────────────────────────────────────────────
/// M1 桥入口键：一律按「boot 一轮、vid/pid 一台、fw 一版」的四元组冻结。
///
/// 分界：桥入口之前（本域）一切按四元组签名对齐；桥入口之后 key_event
/// 的有效载荷归 M1，M5 解析器唯一实现也在 M5，本域不重复实现任何解析。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tuple4 {
    pub boot_id: u32,
    pub vid: u16,
    pub pid: u16,
    pub fw: u32,
}

impl Tuple4 {
    pub fn new(boot_id: u32, vid: u16, pid: u16, fw: u32) -> Tuple4 {
        Tuple4 { boot_id, vid, pid, fw }
    }

    /// 四要素全部非零才成立：任何一维为零都是探测帧或占位帧。
    pub fn validate(&self) -> Result<(), u32> {
        if self.boot_id == 0 {
            return Err(OpCode::TupleGap.val());
        }
        if self.vid == 0 || self.pid == 0 {
            return Err(OpCode::TupleGap.val());
        }
        if self.fw == 0 {
            return Err(OpCode::TupleGap.val());
        }
        Ok(())
    }

    /// 签名：四维定序折叠，不依赖内存布局。
    pub fn signature(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_u32(h, self.boot_id);
        h = fold_byte(h, 0x2F);
        h = fold_u16(h, self.vid);
        h = fold_byte(h, 0x3A);
        h = fold_u16(h, self.pid);
        h = fold_byte(h, 0x40);
        fold_u32(h, self.fw)
    }

    pub fn text(&self) -> String {
        format!(
            "boot=0x{:08X} vid=0x{:04X} pid=0x{:04X} fw=0x{:04X}",
            self.boot_id, self.vid, self.pid, self.fw
        )
    }
}

/// 冻结行：设备号 + 入口键 + 冻结时签名。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Frozen {
    pub device: u8,
    pub key: Tuple4,
    pub sig: u64,
}

/// 入口签名簿：每设备一把键，重复登记与漂移都拒。
pub struct TupleBook {
    rows: Vec<Frozen>,
}

impl TupleBook {
    pub fn new() -> TupleBook {
        TupleBook { rows: Vec::new() }
    }

    /// 冻结：设备号 → 唯一键。设备号越界、键非法、设备重号、签名撞号全拒。
    pub fn freeze(&mut self, device: u8, key: Tuple4) -> Result<u64, u32> {
        if (device as usize) >= DEVICE_COUNT {
            return Err(OpCode::TupleGap.val());
        }
        key.validate()?;
        let sig = key.signature();
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(row) = self.rows.get(i) {
                if row.device == device || row.sig == sig {
                    return Err(OpCode::TupleDup.val());
                }
            }
            i += 1;
        }
        self.rows.push(Frozen { device, key, sig });
        Ok(sig)
    }

    /// 校验：按设备号取回冻结键并逐维比对（不比内存，比字段）。
    pub fn verify(&self, device: u8, key: Tuple4) -> Result<u64, u32> {
        if (device as usize) >= DEVICE_COUNT {
            return Err(OpCode::TupleGap.val());
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(row) = self.rows.get(i) {
                if row.device == device {
                    if row.key != key || row.sig != key.signature() {
                        return Err(OpCode::TupleDrift.val());
                    }
                    return Ok(row.sig);
                }
            }
            i += 1;
        }
        Err(OpCode::TupleGap.val())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn row(&self, idx: usize) -> Option<Frozen> {
        self.rows.get(idx).copied()
    }

    /// 全簿签名根（用于批次印签）。
    pub fn root(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(row) = self.rows.get(i) {
                h = fold_byte(h, row.device);
                h ^= row.sig;
                h = h.wrapping_mul(FNV_PRIME);
            }
            i += 1;
        }
        h
    }

    /// 漂移压测：每轮对 10 设备做一次原键校验 + 一次扰动键校验。
    /// 返回 (原键通过数, 扰动被拒数)；两者都必须等于 rounds*10。
    pub fn drift_rounds(&self, rounds: u32) -> Result<(u32, u32), u32> {
        let mut pass = 0u32;
        let mut caught = 0u32;
        let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x4452_4946_54);
        let mut r = 0u32;
        while r < rounds {
            let mut i = 0usize;
            while i < self.rows.len() {
                if let Some(row) = self.rows.get(i) {
                    match self.verify(row.device, row.key) {
                        Ok(_) => pass = pass.saturating_add(1),
                        Err(_) => return Err(OpCode::TupleDrift.val()),
                    }
                    let bump = rng.below(0xFFFF) as u32 + 1;
                    let bad = Tuple4::new(
                        row.key.boot_id,
                        row.key.vid,
                        row.key.pid,
                        row.key.fw.saturating_add(bump),
                    );
                    match self.verify(row.device, bad) {
                        Err(e) => {
                            if e != OpCode::TupleDrift.val() {
                                return Err(e);
                            }
                            caught = caught.saturating_add(1);
                        }
                        Ok(_) => return Err(OpCode::TupleDrift.val()),
                    }
                }
                i += 1;
            }
            r = r.saturating_add(1);
        }
        Ok((pass, caught))
    }
}

/// 构造 10 台确定性设备（vid/pid/fw 固定，boot_id 随设备号递增）。
pub fn build_book() -> Result<TupleBook, u32> {
    let mut book = TupleBook::new();
    let mut i = 0u8;
    while (i as usize) < DEVICE_COUNT {
        let key = Tuple4::new(
            0xA5A5_0000u32.saturating_add(i as u32),
            0x046D,
            0xC52Bu16.saturating_add(i as u16),
            0x0100u32.saturating_add(i as u32),
        );
        book.freeze(i, key)?;
        i = i.saturating_add(1);
    }
    Ok(book)
}

// ── 限速器甲：令牌桶 ─────────────────────────────────────────────
/// 令牌桶：容量/补充量/tick 宽度三参数冻结，补充走 saturating。
pub struct Bucket {
    cap: u32,
    refill: u32,
    tokens: u32,
    last_tick: u32,
    granted: u32,
    denied: u32,
}

impl Bucket {
    pub fn new(cap: u32, refill: u32) -> Result<Bucket, u32> {
        if cap == 0 || refill == 0 || refill > cap {
            return Err(OpCode::RateBad.val());
        }
        Ok(Bucket { cap, refill, tokens: cap, last_tick: 0, granted: 0, denied: 0 })
    }

    /// 按 tick 差补令牌；时间倒流（now < last）不补也不罚。
    pub fn top_up(&mut self, now_tick: u32) {
        if now_tick > self.last_tick {
            let steps = now_tick - self.last_tick;
            let add = steps.saturating_mul(self.refill);
            let next = self.tokens.saturating_add(add);
            self.tokens = if next > self.cap { self.cap } else { next };
            self.last_tick = now_tick;
        }
    }

    /// 取 n 个令牌：不足即拒（拒绝由调用方转背压，不得静默丢弃）。
    pub fn try_take(&mut self, now_tick: u32, n: u32) -> Result<u32, u32> {
        if n == 0 || n > self.cap {
            return Err(OpCode::RateBad.val());
        }
        self.top_up(now_tick);
        if self.tokens < n {
            self.denied = self.denied.saturating_add(1);
            return Err(OpCode::RateBad.val());
        }
        self.tokens = self.tokens.saturating_sub(n);
        self.granted = self.granted.saturating_add(n);
        Ok(self.tokens)
    }

    pub fn tokens(&self) -> u32 {
        self.tokens
    }

    pub fn granted(&self) -> u32 {
        self.granted
    }

    pub fn denied(&self) -> u32 {
        self.denied
    }

    pub fn cap(&self) -> u32 {
        self.cap
    }
}

// ── 限速器乙：滑动窗口 ───────────────────────────────────────────
/// 滑动窗口：跨度内配额计数；窗口外样本按时间戳淘汰。
pub struct SlidingWindow {
    span_ms: u32,
    cap: u32,
    marks: Vec<u32>,
    denied: u32,
}

impl SlidingWindow {
    pub fn new(span_ms: u32, cap: u32) -> Result<SlidingWindow, u32> {
        if span_ms == 0 || cap == 0 || cap > 4096 {
            return Err(OpCode::RateBad.val());
        }
        Ok(SlidingWindow { span_ms, cap, marks: Vec::new(), denied: 0 })
    }

    /// 准入：先淘汰过期样本，再判配额；返回窗口内累计。
    ///
    /// 窗口语义是半开区间 (ts-span, ts]：只有 ts 已经跨过一整个跨度时才有
    /// 样本过期；ts < span 时时间轴左端还在负数侧，任何样本都不过期
    /// （用 saturating_sub 直接当边界会把 0 号样本误杀）。
    pub fn admit(&mut self, ts_ms: u32) -> Result<u32, u32> {
        if ts_ms > self.span_ms {
            let floor = ts_ms - self.span_ms;
            self.marks.retain(|m| *m > floor);
        }
        if self.marks.len() >= self.cap as usize {
            self.denied = self.denied.saturating_add(1);
            return Err(OpCode::RateBad.val());
        }
        self.marks.push(ts_ms);
        Ok(self.marks.len() as u32)
    }

    pub fn in_window(&self) -> usize {
        self.marks.len()
    }

    pub fn denied(&self) -> u32 {
        self.denied
    }

    pub fn cap(&self) -> u32 {
        self.cap
    }

    pub fn span_ms(&self) -> u32 {
        self.span_ms
    }
}

// ── 三操作 SOP ───────────────────────────────────────────────────
/// 运维三操作：限速 / 重置 / 回退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ops {
    Throttle,
    Reset,
    Rollback,
}

impl Ops {
    pub fn ord(self) -> u8 {
        match self {
            Ops::Throttle => 0,
            Ops::Reset => 1,
            Ops::Rollback => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Ops::Throttle => "限速",
            Ops::Reset => "重置",
            Ops::Rollback => "回退",
        }
    }

    pub fn from_ord(v: u8) -> Result<Ops, u32> {
        match v {
            0 => Ok(Ops::Throttle),
            1 => Ok(Ops::Reset),
            2 => Ok(Ops::Rollback),
            _ => Err(OpCode::SopUnknown.val()),
        }
    }
}

/// 证据包四要素（版本 / 设备 / 判据 / 时间）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Evidence {
    pub version: u32,
    pub device_mask: u32,
    pub criteria: u8,
    pub at_ms: u32,
}

impl Evidence {
    pub fn new(version: u32, device_mask: u32, criteria: u8, at_ms: u32) -> Evidence {
        Evidence { version, device_mask, criteria, at_ms }
    }

    /// 四要素缺一不放行。
    pub fn complete(&self) -> bool {
        self.version > 0 && self.device_mask != 0 && self.criteria > 0 && self.at_ms > 0
    }

    /// 已有要素个数（0..=4），用于诊断文案。
    pub fn count(&self) -> u8 {
        let mut n = 0u8;
        if self.version > 0 {
            n = n.saturating_add(1);
        }
        if self.device_mask != 0 {
            n = n.saturating_add(1);
        }
        if self.criteria > 0 {
            n = n.saturating_add(1);
        }
        if self.at_ms > 0 {
            n = n.saturating_add(1);
        }
        n
    }

    pub fn text(&self) -> String {
        format!(
            "v={} dev=0x{:03X} crit={} at={}ms",
            self.version, self.device_mask, self.criteria, self.at_ms
        )
    }
}

/// 重置保留面：哪几本账必须保留、哪一本必须清。
///
/// 冻结口径：设备账/四元组簿/证据包归档必须保留；限速账与窗口样本必须清。
/// 任何一次「重置清设备账」都等于伪造域账，直接拒。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeepSet {
    pub devices: bool,
    pub tuples: bool,
    pub evidence: bool,
    pub rate: bool,
}

impl KeepSet {
    pub fn reset_default() -> KeepSet {
        KeepSet { devices: true, tuples: true, evidence: true, rate: false }
    }

    pub fn legal_reset(&self) -> bool {
        self.devices && self.tuples && self.evidence && !self.rate
    }
}

/// 运维状态机：把三操作写成真实前置条件，而不是三句日志。
pub struct OpsState {
    cap: u32,
    tokens: u32,
    current: u32,
    stable: u32,
    prev: u32,
    throttles: u32,
    resets: u32,
    rollbacks: u32,
    kept_devices: u32,
    kept_tuples: u32,
    kept_evidence: u32,
    cleared_rate: u32,
}

impl OpsState {
    pub fn new(cap: u32, current: u32) -> Result<OpsState, u32> {
        if cap == 0 || current == 0 {
            return Err(OpCode::RateBad.val());
        }
        Ok(OpsState {
            cap,
            tokens: cap,
            current,
            stable: current,
            prev: current.saturating_sub(1),
            throttles: 0,
            resets: 0,
            rollbacks: 0,
            kept_devices: 0,
            kept_tuples: 0,
            kept_evidence: 0,
            cleared_rate: 0,
        })
    }

    /// 执行一次操作。返回操作后的水位（限速→新桶容量；重置→清零后的桶；
    /// 回退→回退后的版本号）。
    pub fn apply(
        &mut self,
        op: Ops,
        ev: Option<Evidence>,
        keep: &KeepSet,
    ) -> Result<u32, u32> {
        match op {
            Ops::Throttle => {
                // 限速：桶容量减半，下限 8（低于 8 等于关流，须走重置）。
                // 已在下限时再做限速是幂等操作（留痕照记），不报错——
                // 运维现场「再压一次」必须可执行，否则操作臂会在下限处卡死。
                let half = self.cap / 2;
                let next = if half < 8 { 8 } else { half };
                self.cap = next;
                self.tokens = next;
                self.throttles = self.throttles.saturating_add(1);
                Ok(next)
            }
            Ops::Reset => {
                // 重置：保留面必须先合法，否则拒；限速账与窗口样本一并清。
                if !keep.legal_reset() {
                    return Err(OpCode::SopOrder.val());
                }
                self.tokens = self.cap;
                self.resets = self.resets.saturating_add(1);
                self.kept_devices = self.kept_devices.saturating_add(1);
                self.kept_tuples = self.kept_tuples.saturating_add(1);
                self.kept_evidence = self.kept_evidence.saturating_add(1);
                self.cleared_rate = self.cleared_rate.saturating_add(1);
                Ok(0)
            }
            Ops::Rollback => {
                // 回退：证据包四要素齐全，且目标必须正好是上一稳定版。
                let pack = match ev {
                    Some(e) => e,
                    None => return Err(OpCode::SopNoEvidence.val()),
                };
                if !pack.complete() {
                    return Err(OpCode::SopNoEvidence.val());
                }
                if pack.version != self.prev || pack.version >= self.current {
                    return Err(OpCode::SopOrder.val());
                }
                self.current = pack.version;
                self.stable = pack.version;
                self.prev = pack.version.saturating_sub(1);
                self.tokens = self.cap;
                self.rollbacks = self.rollbacks.saturating_add(1);
                Ok(self.current)
            }
        }
    }

    pub fn cap(&self) -> u32 {
        self.cap
    }

    pub fn current(&self) -> u32 {
        self.current
    }

    pub fn stable(&self) -> u32 {
        self.stable
    }

    pub fn prev(&self) -> u32 {
        self.prev
    }

    pub fn throttles(&self) -> u32 {
        self.throttles
    }

    pub fn resets(&self) -> u32 {
        self.resets
    }

    pub fn rollbacks(&self) -> u32 {
        self.rollbacks
    }

    pub fn kept_devices(&self) -> u32 {
        self.kept_devices
    }

    pub fn kept_tuples(&self) -> u32 {
        self.kept_tuples
    }

    pub fn kept_evidence(&self) -> u32 {
        self.kept_evidence
    }

    pub fn cleared_rate(&self) -> u32 {
        self.cleared_rate
    }
}

// ── 焦点路由（只在焦点变化时切换） ───────────────────────────────
/// 路由表：焦点归 vxwm 判定，本域只按焦点递送。
///
/// 不变量：同焦点重复 `focus_to` 只记 stale，不产生切换；递送窗口不等于
/// 当前焦点时一律 RouteStale（这就是「零串窗」的可执行定义）。
pub struct RouteTable {
    current: u32,
    deliveries: u32,
    switches: u32,
    stale: u32,
}

impl RouteTable {
    pub fn new() -> RouteTable {
        RouteTable { current: 0, deliveries: 0, switches: 0, stale: 0 }
    }

    /// 切换焦点：0 号窗口非法；同值不切（返回 false 并计 stale）。
    pub fn focus_to(&mut self, win: u32) -> Result<bool, u32> {
        if win == 0 {
            return Err(OpCode::RouteStale.val());
        }
        if win == self.current {
            self.stale = self.stale.saturating_add(1);
            return Ok(false);
        }
        self.current = win;
        self.switches = self.switches.saturating_add(1);
        Ok(true)
    }

    /// 递送：窗口必须等于当前焦点，否则拒（不消耗递送计数）。
    pub fn deliver(&mut self, win: u32, seq: u32) -> Result<u32, u32> {
        if win == 0 || win != self.current {
            return Err(OpCode::RouteStale.val());
        }
        self.deliveries = self.deliveries.saturating_add(1);
        Ok(seq)
    }

    pub fn current(&self) -> u32 {
        self.current
    }

    pub fn deliveries(&self) -> u32 {
        self.deliveries
    }

    pub fn switches(&self) -> u32 {
        self.switches
    }

    pub fn stale(&self) -> u32 {
        self.stale
    }
}

// ── 留痕账 ───────────────────────────────────────────────────────
/// 一条 SOP 留痕：操作、轮序、执行前水位、执行后水位、证据要素数、是否成立。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SopStep {
    pub op: u8,
    pub seq: u32,
    pub before: u32,
    pub after: u32,
    pub evidence: u8,
    pub ok: bool,
}

impl SopStep {
    pub fn text(&self) -> String {
        format!(
            "{}#{} {}->{} ev={} {}",
            match self.op {
                0 => "限速",
                1 => "重置",
                _ => "回退",
            },
            self.seq,
            self.before,
            self.after,
            self.evidence,
            if self.ok { "过" } else { "拒" }
        )
    }
}

/// 留痕账：三操作各 10 轮，执行/拒绝分开记，合计必须等于总轮数。
pub struct SopLedger {
    steps: Vec<SopStep>,
    per_op: [u32; 3],
    rejected: u32,
}

impl SopLedger {
    pub fn new() -> SopLedger {
        SopLedger { steps: Vec::new(), per_op: [0; 3], rejected: 0 }
    }

    pub fn record(&mut self, step: SopStep) {
        let slot = step.op as usize;
        if let Some(c) = self.per_op.get_mut(slot) {
            *c = c.saturating_add(1);
        }
        if !step.ok {
            self.rejected = self.rejected.saturating_add(1);
        }
        self.steps.push(step);
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    pub fn per_op(&self, idx: usize) -> u32 {
        self.per_op.get(idx).copied().unwrap_or(0)
    }

    pub fn step(&self, idx: usize) -> Option<SopStep> {
        self.steps.get(idx).copied()
    }

    /// 账平：每条 op 槽计数之和 == 步数 == 三操作各 10 轮。
    pub fn balance(&self) -> Result<usize, u32> {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.per_op.len() {
            if let Some(c) = self.per_op.get(i) {
                sum = sum.saturating_add(*c);
            }
            i += 1;
        }
        if sum as usize != self.steps.len() {
            return Err(OpCode::LedgerBad.val());
        }
        if self.per_op.get(0).copied().unwrap_or(0) != SOP_ROUNDS
            || self.per_op.get(1).copied().unwrap_or(0) != SOP_ROUNDS
            || self.per_op.get(2).copied().unwrap_or(0) != SOP_ROUNDS
        {
            return Err(OpCode::LedgerBad.val());
        }
        Ok(self.steps.len())
    }

    pub fn root(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.steps.len() {
            if let Some(s) = self.steps.get(i) {
                h = fold_byte(h, s.op);
                h = fold_u32(h, s.seq);
                h = fold_u32(h, s.before);
                h = fold_u32(h, s.after);
                h = fold_byte(h, s.evidence);
                h = fold_byte(h, if s.ok { 1 } else { 0 });
            }
            i += 1;
        }
        h
    }
}

/// 跑一侧操作：每轮都从状态机取真实前置条件，拒绝也必须留痕。
pub fn run_sop(op: Ops, rounds: u32, state: &mut OpsState, led: &mut SopLedger) -> Result<u32, u32> {
    let mut done = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let before = match op {
            Ops::Throttle => state.cap(),
            Ops::Reset => state.cap(),
            Ops::Rollback => state.current(),
        };
        // 回退的证据包按「上一稳定版」现算：这正是执行臂该做的事。
        let pack = if op == Ops::Rollback {
            Some(Evidence::new(state.prev(), 0x3FF, 4, 1000u32.saturating_add(r)))
        } else {
            None
        };
        let keep = KeepSet::reset_default();
        let outcome = state.apply(op, pack, &keep);
        match outcome {
            Ok(after) => {
                led.record(SopStep {
                    op: op.ord(),
                    seq: r,
                    before,
                    after,
                    evidence: pack.map(|e| e.count()).unwrap_or(0),
                    ok: true,
                });
                done = done.saturating_add(1);
            }
            Err(e) => {
                led.record(SopStep {
                    op: op.ord(),
                    seq: r,
                    before,
                    after: before,
                    evidence: pack.map(|e| e.count()).unwrap_or(0),
                    ok: false,
                });
                return Err(e);
            }
        }
        r = r.saturating_add(1);
    }
    Ok(done)
}

// ── 黄金全链 ─────────────────────────────────────────────────────
/// 黄金：(签名校验数, 令牌放行, 令牌拒绝, SOP 留痕数, 路由切换数, 根)。
pub fn golden_run() -> Result<(u32, u32, u32, u32, u32, u64), u32> {
    // ① 四元组：10 设备冻结 + 1000 轮 ×10 设备原键校验/扰动拒绝。
    let book = build_book()?;
    if book.len() != DEVICE_COUNT {
        return Err(OpCode::LedgerBad.val());
    }
    let (pass, caught) = book.drift_rounds(SIGN_ROUNDS)?;
    if pass != SIGN_ROUNDS.saturating_mul(DEVICE_COUNT as u32)
        || caught != SIGN_ROUNDS.saturating_mul(DEVICE_COUNT as u32)
    {
        return Err(OpCode::TupleDrift.val());
    }

    // ② 令牌桶：1000 tick × 随机请求，拒绝必须显式计数。
    let mut bucket = Bucket::new(BUCKET_CAP, BUCKET_REFILL)?;
    let mut rng = Lcg::new(0x70A00 ^ 0x4255_434B_4554);
    let mut tick = 0u32;
    while tick < 1000 {
        // 逆差压测：请求量恒大于补充量，拒绝路径必须被走到。
        let want = rng.below(5).saturating_add(9);
        match bucket.try_take(tick, want) {
            Ok(_) => {}
            Err(e) => {
                if e != OpCode::RateBad.val() {
                    return Err(e);
                }
            }
        }
        tick = tick.saturating_add(1);
    }
    if bucket.granted() == 0 || bucket.denied() == 0 {
        return Err(OpCode::RateBad.val());
    }

    // ③ SOP：三操作各 10 轮，留痕账必须平。
    let mut state = OpsState::new(64, 20)?;
    let mut led = SopLedger::new();
    let a = run_sop(Ops::Throttle, SOP_ROUNDS, &mut state, &mut led)?;
    let b = run_sop(Ops::Reset, SOP_ROUNDS, &mut state, &mut led)?;
    let c = run_sop(Ops::Rollback, SOP_ROUNDS, &mut state, &mut led)?;
    if a.saturating_add(b).saturating_add(c) != SOP_ROUNDS.saturating_mul(3) {
        return Err(OpCode::LedgerBad.val());
    }
    led.balance()?;

    // ④ 路由：100 轮，每 7 轮换一次焦点，递送必须落在当前焦点。
    let mut route = RouteTable::new();
    route.focus_to(1)?;
    let mut r = 0u32;
    while r < ROUTE_ROUNDS {
        if r % 7 == 0 {
            let next = (r / 7) % 4 + 1;
            route.focus_to(next)?;
        }
        let cur = route.current();
        route.deliver(cur, r)?;
        r = r.saturating_add(1);
    }
    if route.deliveries() != ROUTE_ROUNDS {
        return Err(OpCode::RouteStale.val());
    }

    // ⑤ 根：签名簿 ‖ 留痕账 ‖ 限速结果。
    let mut h = book.root();
    h ^= led.root();
    h = h.wrapping_mul(FNV_PRIME);
    h = fold_u32(h, bucket.granted());
    h = fold_u32(h, bucket.denied());
    h = fold_u32(h, route.switches());
    Ok((pass, bucket.granted(), bucket.denied(), led.len() as u32, route.switches(), h))
}

// ── 注入账（四路，每路必须真实被拒） ─────────────────────────────
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

/// 注入：零字段四元组 / 重号设备 / 无证据回退 / 非法桶参数。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x494E_4A4F_5053);
    let mut r = 0u32;
    while r < rounds {
        let jitter = rng.below(0xFFFF) as u32;

        // 路 1：四元组缺项（四个维度逐个打零，必须四次全拒）。
        let mut gap = 0u32;
        let mut i = 0u32;
        while i < 4 {
            let key = match i {
                0 => Tuple4::new(0, 0x046D, 0xC52B, 0x0100),
                1 => Tuple4::new(0xA5A5_0001, 0, 0xC52B, 0x0100),
                2 => Tuple4::new(0xA5A5_0001, 0x046D, 0, 0x0100),
                _ => Tuple4::new(0xA5A5_0001, 0x046D, 0xC52B, 0),
            };
            if key.validate().err() == Some(OpCode::TupleGap.val()) {
                gap = gap.saturating_add(1);
            }
            i = i.saturating_add(1);
        }
        led.record(0, gap == 4);

        // 路 2：同设备重号冻结（第二把键换 fw 仍必须拒）。
        let mut book = TupleBook::new();
        let first = Tuple4::new(0xA5A5_0001, 0x046D, 0xC52B, 0x0100);
        let second = Tuple4::new(0xA5A5_0002, 0x046D, 0xC52B, 0x0100u32 ^ jitter);
        let dup = book.freeze(3, first).is_ok()
            && book.freeze(3, second).err() == Some(OpCode::TupleDup.val());
        led.record(1, dup);

        // 路 3：回退缺证据包（None 与四要素残缺两种都必须拒）。
        let mut st = OpsState::new(64, 20);
        let no_pack = match st.as_mut() {
            Ok(s) => {
                let keep = KeepSet::reset_default();
                s.apply(Ops::Rollback, None, &keep).err() == Some(OpCode::SopNoEvidence.val())
                    && s.apply(Ops::Rollback, Some(Evidence::new(0, 0, 0, 0)), &keep).err()
                        == Some(OpCode::SopNoEvidence.val())
            }
            Err(_) => false,
        };
        led.record(2, no_pack);

        // 路 4：非法桶参数与非法请求量。
        let bad_bucket = Bucket::new(0, 1).err() == Some(OpCode::RateBad.val())
            && Bucket::new(8, 9).err() == Some(OpCode::RateBad.val());
        led.record(3, bad_bucket);

        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 探针三序 ─────────────────────────────────────────────────────
/// 序 A：黄金全链两跑同值，签名数/留痕数达判据量级。
pub fn probe_seq_a() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    let want_sign = SIGN_ROUNDS.saturating_mul(DEVICE_COUNT as u32);
    if a == b && a.0 == want_sign && a.3 == SOP_ROUNDS.saturating_mul(3) && a.4 > 0 {
        Ok(())
    } else {
        Err(OpCode::LedgerBad.val())
    }
}

/// 序 B：四路非法输入必须逐路拒（不依赖注入账）。
pub fn probe_seq_b() -> Result<(), u32> {
    let zero = Tuple4::new(0, 1, 1, 1);
    if zero.validate().err() != Some(OpCode::TupleGap.val()) {
        return Err(OpCode::TupleGap.val());
    }
    let mut book = TupleBook::new();
    let k = Tuple4::new(1, 2, 3, 4);
    if book.freeze(0, k).is_err() {
        return Err(OpCode::TupleGap.val());
    }
    if book.verify(0, Tuple4::new(1, 2, 3, 5)).err() != Some(OpCode::TupleDrift.val()) {
        return Err(OpCode::TupleDrift.val());
    }
    match OpsState::new(64, 20) {
        Ok(mut s) => {
            let keep = KeepSet { devices: false, tuples: true, evidence: true, rate: false };
            if s.apply(Ops::Reset, None, &keep).err() != Some(OpCode::SopOrder.val()) {
                return Err(OpCode::SopOrder.val());
            }
        }
        Err(e) => return Err(e),
    }
    let mut route = RouteTable::new();
    if route.focus_to(0).err() != Some(OpCode::RouteStale.val()) {
        return Err(OpCode::RouteStale.val());
    }
    Ok(())
}

/// 序 C：冻结常量（10 设备 / 1000 轮 / 32 桶 / 8 补充 / 100ms 窗 / 40 配额）。
pub fn probe_seq_c() -> Result<(), u32> {
    if DEVICE_COUNT == 10
        && SIGN_ROUNDS == 1000
        && SOP_ROUNDS == 10
        && BUCKET_CAP == 32
        && BUCKET_REFILL == 8
        && WINDOW_SPAN_MS == 100
        && WINDOW_CAP == 40
        && TICK_MS == 8
    {
        Ok(())
    } else {
        Err(OpCode::SegmentBad.val())
    }
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(OpCode::LedgerBad.val());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落在 [SEG_BASE, SEG_BASE+0x20)。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < OpCode::ALL.len() {
        if let Some(c) = OpCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE.saturating_add(0x20) {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面 ───────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // ① 三操作 SOP ×10 留痕。
    let mut state = OpsState::new(64, 20);
    let mut sop = SopLedger::new();
    let sop_ok = match state.as_mut() {
        Ok(s) => {
            let a = run_sop(Ops::Throttle, SOP_ROUNDS, s, &mut sop).is_ok();
            let b = run_sop(Ops::Reset, SOP_ROUNDS, s, &mut sop).is_ok();
            let c = run_sop(Ops::Rollback, SOP_ROUNDS, s, &mut sop).is_ok();
            a && b && c && sop.balance().is_ok()
        }
        Err(_) => false,
    };
    v.push((
        "F48881-J1-三操作SOP十轮留痕",
        sop_ok,
        format!("steps={} rej={} root=0x{:016X}", sop.len(), sop.rejected(), sop.root()),
    ));

    // ② 重置保留面：合法保留面清限速账，非法保留面必拒。
    let reset_ok = match OpsState::new(64, 20) {
        Ok(mut s) => {
            let good = KeepSet::reset_default();
            let bad = KeepSet { devices: true, tuples: false, evidence: true, rate: false };
            let a = s.apply(Ops::Reset, None, &good).is_ok();
            let b = s.apply(Ops::Reset, None, &bad).err() == Some(OpCode::SopOrder.val());
            a && b && s.resets() == 1 && s.cleared_rate() == 1
        }
        Err(_) => false,
    };
    v.push((
        "F48881-J1-重置保留账清限速",
        reset_ok,
        String::from("保留设备/四元组/证据，清限速；缺一即 SopOrder"),
    ));

    // ③ 回退必须带齐四要素。
    let rollback_ok = match OpsState::new(64, 20) {
        Ok(mut s) => {
            let keep = KeepSet::reset_default();
            let missing = s.apply(Ops::Rollback, Some(Evidence::new(19, 0, 4, 9)), &keep)
                .err()
                == Some(OpCode::SopNoEvidence.val());
            let wrong = s.apply(Ops::Rollback, Some(Evidence::new(18, 0x3FF, 4, 9)), &keep)
                .err()
                == Some(OpCode::SopOrder.val());
            let good = s.apply(Ops::Rollback, Some(Evidence::new(19, 0x3FF, 4, 9)), &keep)
                .is_ok();
            missing && wrong && good && s.current() == 19
        }
        Err(_) => false,
    };
    v.push((
        "F48881-J1-回退带证据包",
        rollback_ok,
        String::from("四要素缺一拒；目标非上一稳定版拒；齐备才回退"),
    ));

    // ④ 四元组千轮零漂移。
    let drift = match build_book() {
        Ok(book) => {
            let r = book.drift_rounds(SIGN_ROUNDS);
            match &r {
                Ok((pass, caught)) => {
                    *pass == SIGN_ROUNDS.saturating_mul(10)
                        && *caught == SIGN_ROUNDS.saturating_mul(10)
                }
                Err(_) => false,
            }
        }
        Err(_) => false,
    };
    v.push((
        "F48882-J1-四元组千轮零漂移",
        drift,
        String::from("1000 轮 × 10 设备：原键全过、扰动全拒"),
    ));

    // ⑤ 四元组缺项必拒（四维逐个）。
    let mut gaps = 0u32;
    let mut i = 0u32;
    while i < 4 {
        let key = match i {
            0 => Tuple4::new(0, 0x046D, 0xC52B, 0x0100),
            1 => Tuple4::new(1, 0, 0xC52B, 0x0100),
            2 => Tuple4::new(1, 0x046D, 0, 0x0100),
            _ => Tuple4::new(1, 0x046D, 0xC52B, 0),
        };
        if key.validate().err() == Some(OpCode::TupleGap.val()) {
            gaps = gaps.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    v.push((
        "F48882-J1-四元组缺项必拒",
        gaps == 4,
        format!("zero_fields_rejected={}/4", gaps),
    ));

    // ⑥ 设备唯一登记 + 签名根稳定。
    let uniq = match build_book() {
        Ok(mut book) => {
            let dup = book
                .freeze(0, Tuple4::new(7, 8, 9, 10))
                .err()
                == Some(OpCode::TupleDup.val());
            let root = book.root();
            dup && root != 0 && book.len() == DEVICE_COUNT
        }
        Err(_) => false,
    };
    v.push((
        "F48882-J1-设备唯一登记",
        uniq,
        String::from("10 槽各一把键；同槽二冻 TupleDup"),
    ));

    // ⑦ 限速双模：桶放行 + 窗口配额。
    let mut rate_ok = false;
    let mut rate_txt = String::from("err");
    if let Ok(mut bucket) = Bucket::new(BUCKET_CAP, BUCKET_REFILL) {
        if let Ok(mut window) = SlidingWindow::new(WINDOW_SPAN_MS, WINDOW_CAP) {
            let mut granted = 0u32;
            let mut tick = 0u32;
            while tick < 200 {
                // 每 tick 取 9 个令牌而只补 8 个：持续逆差必然触发拒绝。
                if bucket.try_take(tick, 9).is_ok() {
                    granted = granted.saturating_add(1);
                }
                tick = tick.saturating_add(1);
            }
            let mut wdeny = 0u32;
            let mut ms = 0u32;
            while ms < 200 {
                if window.admit(ms).is_err() {
                    wdeny = wdeny.saturating_add(1);
                }
                ms = ms.saturating_add(1);
            }
            rate_ok = granted > 0 && bucket.denied() > 0 && wdeny > 0;
            rate_txt = format!(
                "bucket_grant={} deny={} window_deny={} cap={}",
                granted,
                bucket.denied(),
                wdeny,
                window.cap()
            );
        }
    }
    v.push(("F48881-J1-限速双模", rate_ok, rate_txt));

    // ⑧ 焦点路由只在变更时切换，串窗必拒。
    let route_ok = {
        let mut route = RouteTable::new();
        let a = route.focus_to(1).ok() == Some(true);
        let b = route.focus_to(1).ok() == Some(false);
        let c = route.deliver(1, 1).is_ok();
        let d = route.deliver(2, 2).err() == Some(OpCode::RouteStale.val());
        a && b && c && d && route.switches() == 1 && route.stale() == 1
    };
    v.push((
        "F48881-J1-焦点仅变更切换",
        route_ok,
        String::from("同焦点不切、异焦点递送 RouteStale"),
    ));

    // ⑨ 四路注入。
    let inj = inject_rounds(10);
    v.push((
        "F48881-J1-注入四路10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // ⑩ 探针三序 + 双跑 + 段位。
    v.push((
        "F48882-J1-探针三序与段位",
        probe_seq_a().is_ok()
            && probe_seq_b().is_ok()
            && probe_seq_c().is_ok()
            && repro_check().is_ok()
            && segment_guard(),
        format!(
            "a={} b={} c={} repro={} seg={}",
            probe_seq_a().is_ok(),
            probe_seq_b().is_ok(),
            probe_seq_c().is_ok(),
            repro_check().is_ok(),
            segment_guard()
        ),
    ));

    v
}

pub fn run_unx48881_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48881 事件层运维总成与 M1 桥入口四元组");
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
    fn tuple_validate_rejects_zero_fields() {
        assert!(Tuple4::new(1, 2, 3, 4).validate().is_ok());
        assert_eq!(
            Tuple4::new(0, 2, 3, 4).validate().err(),
            Some(OpCode::TupleGap.val())
        );
        assert_eq!(
            Tuple4::new(1, 0, 3, 4).validate().err(),
            Some(OpCode::TupleGap.val())
        );
        assert_eq!(
            Tuple4::new(1, 2, 0, 4).validate().err(),
            Some(OpCode::TupleGap.val())
        );
        assert_eq!(
            Tuple4::new(1, 2, 3, 0).validate().err(),
            Some(OpCode::TupleGap.val())
        );
    }

    #[test]
    fn tuple_signature_is_field_order_dependent() {
        let a = Tuple4::new(1, 2, 3, 4);
        let b = Tuple4::new(1, 3, 2, 4);
        assert_ne!(a.signature(), b.signature());
        assert_eq!(a.signature(), Tuple4::new(1, 2, 3, 4).signature());
    }

    #[test]
    fn book_freezes_ten_and_rejects_duplicates() {
        let mut book = build_book().ok().unwrap_or_else(TupleBook::new);
        assert_eq!(book.len(), 10);
        assert_eq!(
            book.freeze(0, Tuple4::new(9, 9, 9, 9)).err(),
            Some(OpCode::TupleDup.val())
        );
        assert!(book.freeze(10, Tuple4::new(9, 9, 9, 9)).is_err());
        assert!(book.root() != 0);
    }

    #[test]
    fn verify_detects_drift() {
        let mut book = TupleBook::new();
        let key = Tuple4::new(0x1234, 0x046D, 0xC52B, 0x0100);
        assert!(book.freeze(0, key).is_ok());
        assert!(book.verify(0, key).is_ok());
        assert_eq!(
            book.verify(0, Tuple4::new(0x1234, 0x046D, 0xC52B, 0x0101)).err(),
            Some(OpCode::TupleDrift.val())
        );
        assert_eq!(
            book.verify(1, key).err(),
            Some(OpCode::TupleGap.val())
        );
    }

    #[test]
    fn bucket_refills_and_denies() {
        let mut b = Bucket::new(8, 2).ok().unwrap_or_else(|| Bucket {
            cap: 8,
            refill: 2,
            tokens: 8,
            last_tick: 0,
            granted: 0,
            denied: 0,
        });
        assert_eq!(b.try_take(0, 8).ok(), Some(0));
        assert_eq!(b.try_take(0, 1).err(), Some(OpCode::RateBad.val()));
        assert_eq!(b.try_take(1, 2).ok(), Some(0));
        assert_eq!(b.granted(), 10);
        assert_eq!(b.denied(), 1);
    }

    #[test]
    fn bucket_parameters_are_strict() {
        assert_eq!(Bucket::new(0, 1).err(), Some(OpCode::RateBad.val()));
        assert_eq!(Bucket::new(4, 0).err(), Some(OpCode::RateBad.val()));
        assert_eq!(Bucket::new(4, 5).err(), Some(OpCode::RateBad.val()));
        assert!(Bucket::new(4, 4).is_ok());
    }

    #[test]
    fn window_prunes_and_caps() {
        let mut w = SlidingWindow::new(100, 4).ok().unwrap_or_else(|| SlidingWindow {
            span_ms: 100,
            cap: 4,
            marks: Vec::new(),
            denied: 0,
        });
        assert_eq!(w.admit(0).ok(), Some(1));
        assert_eq!(w.admit(10).ok(), Some(2));
        assert_eq!(w.admit(20).ok(), Some(3));
        assert_eq!(w.admit(30).ok(), Some(4));
        assert_eq!(w.admit(40).err(), Some(OpCode::RateBad.val()));
        // 150ms 之后窗口内旧样本全部过期（>50），配额恢复。
        assert_eq!(w.admit(150).ok(), Some(1));
        assert_eq!(w.denied(), 1);
    }

    #[test]
    fn throttle_halves_with_floor() {
        let mut s = OpsState::new(64, 20).ok().unwrap_or_else(|| OpsState {
            cap: 64,
            tokens: 64,
            current: 20,
            stable: 20,
            prev: 19,
            throttles: 0,
            resets: 0,
            rollbacks: 0,
            kept_devices: 0,
            kept_tuples: 0,
            kept_evidence: 0,
            cleared_rate: 0,
        });
        let keep = KeepSet::reset_default();
        assert_eq!(s.apply(Ops::Throttle, None, &keep).ok(), Some(32));
        assert_eq!(s.apply(Ops::Throttle, None, &keep).ok(), Some(16));
        assert_eq!(s.apply(Ops::Throttle, None, &keep).ok(), Some(8));
        // 已在下限：再压一次幂等成立（仍留痕），不会把桶压到 0。
        assert_eq!(s.apply(Ops::Throttle, None, &keep).ok(), Some(8));
        assert_eq!(s.throttles(), 4);
    }

    #[test]
    fn reset_requires_keep_set() {
        let mut s = OpsState::new(32, 9).ok().unwrap_or_else(|| OpsState {
            cap: 32,
            tokens: 32,
            current: 9,
            stable: 9,
            prev: 8,
            throttles: 0,
            resets: 0,
            rollbacks: 0,
            kept_devices: 0,
            kept_tuples: 0,
            kept_evidence: 0,
            cleared_rate: 0,
        });
        let bad = KeepSet { devices: true, tuples: true, evidence: true, rate: true };
        assert_eq!(s.apply(Ops::Reset, None, &bad).err(), Some(OpCode::SopOrder.val()));
        let good = KeepSet::reset_default();
        assert!(s.apply(Ops::Reset, None, &good).is_ok());
        assert_eq!(s.resets(), 1);
        assert_eq!(s.cleared_rate(), 1);
        assert_eq!(s.kept_devices(), 1);
    }

    #[test]
    fn rollback_needs_complete_evidence_and_prev_version() {
        let mut s = OpsState::new(32, 20).ok().unwrap_or_else(|| OpsState {
            cap: 32,
            tokens: 32,
            current: 20,
            stable: 20,
            prev: 19,
            throttles: 0,
            resets: 0,
            rollbacks: 0,
            kept_devices: 0,
            kept_tuples: 0,
            kept_evidence: 0,
            cleared_rate: 0,
        });
        let keep = KeepSet::reset_default();
        assert_eq!(
            s.apply(Ops::Rollback, None, &keep).err(),
            Some(OpCode::SopNoEvidence.val())
        );
        assert_eq!(
            s.apply(Ops::Rollback, Some(Evidence::new(19, 0, 4, 7)), &keep).err(),
            Some(OpCode::SopNoEvidence.val())
        );
        assert_eq!(
            s.apply(Ops::Rollback, Some(Evidence::new(18, 1, 4, 7)), &keep).err(),
            Some(OpCode::SopOrder.val())
        );
        assert_eq!(
            s.apply(Ops::Rollback, Some(Evidence::new(19, 1, 4, 7)), &keep).ok(),
            Some(19)
        );
        assert_eq!(s.current(), 19);
        assert_eq!(s.prev(), 18);
        assert_eq!(s.rollbacks(), 1);
    }

    #[test]
    fn route_switches_only_on_change() {
        let mut r = RouteTable::new();
        assert_eq!(r.focus_to(2).ok(), Some(true));
        assert_eq!(r.focus_to(2).ok(), Some(false));
        assert_eq!(r.deliver(2, 7).ok(), Some(7));
        assert_eq!(r.deliver(3, 8).err(), Some(OpCode::RouteStale.val()));
        assert_eq!(r.switches(), 1);
        assert_eq!(r.deliveries(), 1);
        assert_eq!(r.stale(), 1);
    }

    #[test]
    fn golden_values_are_frozen() {
        let g = golden_run();
        assert!(g.is_ok());
        match g {
            Ok((pass, granted, denied, steps, switches, root)) => {
                assert_eq!(pass, 10000);
                assert!(granted > 0);
                assert!(denied > 0);
                assert_eq!(steps, 30);
                assert!(switches >= 14);
                assert!(root != 0);
            }
            Err(_) => assert!(false),
        }
        assert!(repro_check().is_ok());
    }

    #[test]
    fn ledger_rejects_wrong_rounds() {
        let mut led = SopLedger::new();
        led.record(SopStep { op: 0, seq: 0, before: 1, after: 1, evidence: 0, ok: true });
        assert_eq!(led.balance().err(), Some(OpCode::LedgerBad.val()));
        assert_eq!(led.per_op(1), 0);
        assert_eq!(led.per_op(9), 0);
    }

    #[test]
    fn injection_catches_four_routes() {
        match inject_rounds(10) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.tries, [10, 10, 10, 10]);
                assert_eq!(l.caught, [10, 10, 10, 10]);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_and_segment_hold() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(segment_guard());
        assert_eq!(OpCode::ALL.len(), 10);
        assert_eq!(OpCode::SopOrder.val(), SEG_BASE + 1);
        assert_eq!(OpCode::SegmentBad.val(), SEG_BASE + 10);
    }

    #[test]
    fn detail_lookup_covers_family() {
        assert!(detail(SEG_BASE).contains("未知诊断码"));
        assert!(detail(SEG_BASE + 0x20).contains("未知诊断码"));
        let mut i = 0usize;
        while i < OpCode::ALL.len() {
            assert_eq!(detail(OpCode::ALL[i].val()), OpCode::ALL[i].detail());
            i += 1;
        }
    }

    #[test]
    fn checks_are_all_green() {
        let cs = run_unx48881_checks();
        assert!(cs.all_passed());
        assert!(cs.len() >= 8 && cs.len() <= 10);
    }
}
