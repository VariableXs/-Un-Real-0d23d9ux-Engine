//! m700sysc — VARIX-M700 AI-03 系统调用域 (F051~F075)
//!
//! 分发跳转表/成熟号段登记处/参数深查官/系统调用回执/热路径白名单/
//! 慢路径记账/调用契约书/号段考古队/兼容垫片表/系统调用沙盒策略/
//! 参数序列化规范/可重入门禁/调用耗时直方图/半途状态清理/系统调用回放流/
//! errno 语言表/零拷贝调用通道/调用限流阀/号段版本协商/系统调用金样本/
//! 内嵌断言网/调用树视图/长调用看门狗/调用隐私边界/系统调用年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。
//! 类型与常量统一加 `Sysc` 前缀，避免与 crate 内 syscall 既有符号撞名。

use crate::checks::CheckSet;

// ===========================================================================
// F051 — 分发跳转表：定容函数指针表，先到先得不许覆盖
// ===========================================================================

pub const SYSC_TABLE_SLOTS: usize = 64;

pub fn sysc_inc(x: u64) -> u64 {
    x.wrapping_add(1)
}

pub fn sysc_double(x: u64) -> u64 {
    x.wrapping_mul(2)
}

#[derive(Clone, Copy)]
pub struct SyscTable {
    handlers: [Option<fn(u64) -> u64>; SYSC_TABLE_SLOTS],
    count: u32,
}

impl SyscTable {
    pub const fn new() -> SyscTable {
        SyscTable { handlers: [None; SYSC_TABLE_SLOTS], count: 0 }
    }

    /// 登记处理函数；号段越界或已被占用（不许覆盖）返回 false。
    pub fn register(&mut self, nr: usize, h: fn(u64) -> u64) -> bool {
        if nr >= SYSC_TABLE_SLOTS || self.handlers[nr].is_some() {
            return false;
        }
        self.handlers[nr] = Some(h);
        self.count += 1;
        true
    }

    /// 分发；未登记/越界返回 None。
    pub fn dispatch(&self, nr: usize, arg: u64) -> Option<u64> {
        if nr >= SYSC_TABLE_SLOTS {
            return None;
        }
        match self.handlers[nr] {
            Some(h) => Some(h(arg)),
            None => None,
        }
    }

    pub fn registered(&self) -> u32 {
        self.count
    }
}

// ===========================================================================
// F052 — 成熟号段登记处：号段通过 maturity 后打标，去重 + 越界拒绝
// ===========================================================================

pub const SYSC_NR_SPACE: u32 = 128;

#[derive(Clone, Copy, Debug)]
pub struct SyscMaturity {
    marks: [bool; SYSC_NR_SPACE as usize],
    pub count: u32,
}

impl SyscMaturity {
    pub const fn new() -> SyscMaturity {
        SyscMaturity { marks: [false; SYSC_NR_SPACE as usize], count: 0 }
    }

    pub fn mark_mature(&mut self, nr: u32) -> bool {
        if nr >= SYSC_NR_SPACE || self.marks[nr as usize] {
            return false;
        }
        self.marks[nr as usize] = true;
        self.count += 1;
        true
    }

    pub fn is_mature(&self, nr: u32) -> bool {
        nr < SYSC_NR_SPACE && self.marks[nr as usize]
    }
}

// ===========================================================================
// F053 — 参数深查官：用户指针区间 [4096, user_top) 且长度受限
// ===========================================================================

pub const SYSC_MAX_ARG_BYTES: u64 = 4096;
pub const SYSC_USER_TOP: u64 = 0x0000_8000_0000_0000;

pub fn args_ok(ptr: u64, len: u64, user_top: u64) -> bool {
    ptr >= 4096
        && len > 0
        && len <= SYSC_MAX_ARG_BYTES
        && ptr.saturating_add(len) <= user_top
}

// ===========================================================================
// F054 — 系统调用回执：负返回值折算 errno
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyscReceipt {
    pub nr: u32,
    pub ret: i64,
    pub errno: u32,
}

/// ret < 0 时 errno = -ret；否则 0。
pub fn receipt_errno(ret: i64) -> u32 {
    if ret < 0 {
        (0i64.wrapping_sub(ret)) as u32
    } else {
        0
    }
}

pub fn make_receipt(nr: u32, ret: i64) -> SyscReceipt {
    SyscReceipt { nr, ret, errno: receipt_errno(ret) }
}

// ===========================================================================
// F055 — 热路径白名单：固定号段表，热路径只放行白名单成员
// ===========================================================================

pub const SYSC_HOT_PATH: [u32; 6] = [0, 1, 2, 3, 9, 10];

pub fn hot_allowed(nr: u32) -> bool {
    let mut i = 0usize;
    while i < SYSC_HOT_PATH.len() {
        if SYSC_HOT_PATH[i] == nr {
            return true;
        }
        i += 1;
    }
    false
}

// ===========================================================================
// F056 — 慢路径记账：周期数记账，均摊耗时
// ===========================================================================

pub const SYSC_SLOW_RECORD_CAP: u64 = 1_000_000;

#[derive(Clone, Copy, Debug, Default)]
pub struct SyscSlowBook {
    pub calls: u64,
    pub cycles: u64,
}

impl SyscSlowBook {
    /// 记一笔慢调用；到达样本上限拒绝。
    pub fn record(&mut self, cycles: u64) -> bool {
        if self.calls >= SYSC_SLOW_RECORD_CAP {
            return false;
        }
        self.calls += 1;
        self.cycles = self.cycles.wrapping_add(cycles);
        true
    }

    pub fn avg_cycles(&self) -> u64 {
        if self.calls == 0 {
            0
        } else {
            self.cycles / self.calls
        }
    }
}

// ===========================================================================
// F057 — 调用契约书：声明参数个数 ≤ 6，实参必须恰好一致
// ===========================================================================

pub const SYSC_MAX_ARGS: u32 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyscContract {
    pub nr: u32,
    pub args: u32,
    pub may_write_user: bool,
}

pub fn contract_ok(c: SyscContract, actual_args: u32) -> bool {
    c.args <= SYSC_MAX_ARGS && actual_args == c.args
}

// ===========================================================================
// F058 — 号段考古队：历史旧号 → 现行号的固定对照表
// ===========================================================================

pub const SYSC_ARCHAEO_MAP: [(u32, u32); 4] = [(1, 10), (2, 22), (3, 33), (9, 99)];

pub fn legacy_to_modern(old: u32) -> Option<u32> {
    let mut i = 0usize;
    while i < SYSC_ARCHAEO_MAP.len() {
        if SYSC_ARCHAEO_MAP[i].0 == old {
            return Some(SYSC_ARCHAEO_MAP[i].1);
        }
        i += 1;
    }
    None
}

pub fn is_legacy_nr(old: u32) -> bool {
    legacy_to_modern(old).is_some()
}

// ===========================================================================
// F059 — 兼容垫片表：32 位 ABI 的高低字拼拆
// ===========================================================================

pub fn shim_join(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | (lo as u64)
}

pub fn shim_hi(v: u64) -> u32 {
    (v >> 32) as u32
}

pub fn shim_lo(v: u64) -> u32 {
    v as u32
}

/// 参数是否能装进单个 32 位寄存器。
pub fn shim_is_u32(v: u64) -> bool {
    v <= u32::MAX as u64
}

// ===========================================================================
// F060 — 系统调用沙盒策略：按号段拉黑，去重登记
// ===========================================================================

pub const SYSC_SANDBOX_SLOTS: usize = 64;

#[derive(Clone, Copy, Debug)]
pub struct SyscSandbox {
    denied: [bool; SYSC_SANDBOX_SLOTS],
    pub count: u32,
}

impl SyscSandbox {
    pub const fn new() -> SyscSandbox {
        SyscSandbox { denied: [false; SYSC_SANDBOX_SLOTS], count: 0 }
    }

    /// 拉黑一个号；重复/越界返回 false。
    pub fn deny(&mut self, nr: usize) -> bool {
        if nr >= SYSC_SANDBOX_SLOTS || self.denied[nr] {
            return false;
        }
        self.denied[nr] = true;
        self.count += 1;
        true
    }

    pub fn allowed(&self, nr: usize) -> bool {
        nr < SYSC_SANDBOX_SLOTS && !self.denied[nr]
    }
}

// ===========================================================================
// F061 — 参数序列化规范：tag 占高 8 位，payload 占低 32 位
// ===========================================================================

pub fn serialize_args(tag: u8, payload: u32) -> u64 {
    ((tag as u64) << 56) | (payload as u64)
}

pub fn serde_tag(v: u64) -> u8 {
    (v >> 56) as u8
}

pub fn serde_payload(v: u64) -> u32 {
    (v & 0x0000_FFFF_FFFF_FFFF) as u32
}

// ===========================================================================
// F062 — 可重入门禁：嵌套深度封顶，退出不许下穿
// ===========================================================================

pub const SYSC_REENTRY_DEFAULT_MAX: u32 = 3;

#[derive(Clone, Copy, Debug)]
pub struct SyscReentry {
    pub depth: u32,
    pub max: u32,
}

impl SyscReentry {
    pub const fn new(max: u32) -> SyscReentry {
        SyscReentry { depth: 0, max }
    }

    pub fn enter(&mut self) -> bool {
        if self.depth >= self.max {
            return false;
        }
        self.depth += 1;
        true
    }

    pub fn leave(&mut self) -> bool {
        if self.depth == 0 {
            return false;
        }
        self.depth -= 1;
        true
    }
}

// ===========================================================================
// F063 — 调用耗时直方图：5 桶对数分界 1/4/16/64us
// ===========================================================================

pub const SYSC_LAT_BUCKETS: usize = 5;

#[derive(Clone, Copy, Debug)]
pub struct SyscHistogram {
    pub buckets: [u32; SYSC_LAT_BUCKETS],
}

impl SyscHistogram {
    pub const fn new() -> SyscHistogram {
        SyscHistogram { buckets: [0; SYSC_LAT_BUCKETS] }
    }

    pub fn bucket_of(us: u32) -> usize {
        if us <= 1 {
            0
        } else if us <= 4 {
            1
        } else if us <= 16 {
            2
        } else if us <= 64 {
            3
        } else {
            4
        }
    }

    pub fn record(&mut self, us: u32) {
        self.buckets[Self::bucket_of(us)] += 1;
    }

    pub fn total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < SYSC_LAT_BUCKETS {
            sum += self.buckets[i];
            i += 1;
        }
        sum
    }
}

// ===========================================================================
// F064 — 半途状态清理：staged 未提交量可 abort 清零或 commit 收账
// ===========================================================================

pub const SYSC_STAGE_CAP: u32 = 16;

#[derive(Clone, Copy, Debug, Default)]
pub struct SyscCleanup {
    pub staged: u32,
    pub committed: u32,
}

impl SyscCleanup {
    /// 暂存 n 单位；超出容量整体拒绝。
    pub fn stage(&mut self, n: u32) -> bool {
        if self.staged + n > SYSC_STAGE_CAP {
            return false;
        }
        self.staged += n;
        true
    }

    /// 提交全部暂存，返回提交量。
    pub fn commit(&mut self) -> u32 {
        let s = self.staged;
        self.committed += s;
        self.staged = 0;
        s
    }

    /// 丢弃全部暂存，返回丢弃量（半途状态清理）。
    pub fn abort(&mut self) -> u32 {
        let s = self.staged;
        self.staged = 0;
        s
    }
}

// ===========================================================================
// F065 — 系统调用回放流：8 槽环形回放，溢出丢样本记账
// ===========================================================================

pub const SYSC_REPLAY_RING: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct SyscReplay {
    nrs: [u32; SYSC_REPLAY_RING],
    args: [u64; SYSC_REPLAY_RING],
    head: usize,
    pub pushed: u64,
}

impl SyscReplay {
    pub const fn new() -> SyscReplay {
        SyscReplay { nrs: [0; SYSC_REPLAY_RING], args: [0; SYSC_REPLAY_RING], head: 0, pushed: 0 }
    }

    pub fn push(&mut self, nr: u32, arg: u64) {
        self.nrs[self.head] = nr;
        self.args[self.head] = arg;
        self.head = (self.head + 1) % SYSC_REPLAY_RING;
        self.pushed += 1;
    }

    pub fn dropped(&self) -> u64 {
        self.pushed.saturating_sub(SYSC_REPLAY_RING as u64)
    }

    /// 第 i 次推送对应的槽位内容（i ≥ 环容量的即最近一次覆盖值）。
    pub fn nr_at(&self, i: usize) -> u32 {
        self.nrs[i % SYSC_REPLAY_RING]
    }

    pub fn arg_at(&self, i: usize) -> u64 {
        self.args[i % SYSC_REPLAY_RING]
    }
}

// ===========================================================================
// F066 — errno 语言表：枚举 ↔ 编号双向映射
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyscErrno {
    Ok,
    NoEntry,
    BadFd,
    NoMem,
    AccessDenied,
    NotSupported,
}

pub fn errno_code(e: SyscErrno) -> u32 {
    match e {
        SyscErrno::Ok => 0,
        SyscErrno::NoEntry => 2,
        SyscErrno::BadFd => 9,
        SyscErrno::NoMem => 12,
        SyscErrno::AccessDenied => 13,
        SyscErrno::NotSupported => 38,
    }
}

pub fn errno_from_code(c: u32) -> Option<SyscErrno> {
    match c {
        0 => Some(SyscErrno::Ok),
        2 => Some(SyscErrno::NoEntry),
        9 => Some(SyscErrno::BadFd),
        12 => Some(SyscErrno::NoMem),
        13 => Some(SyscErrno::AccessDenied),
        38 => Some(SyscErrno::NotSupported),
        _ => None,
    }
}

// ===========================================================================
// F067 — 零拷贝调用通道：页对齐 + 整页长度 + 页数封顶
// ===========================================================================

pub const SYSC_ZC_MAX_PAGES: u64 = 64;

pub fn zero_copy_ok(ptr: u64, len: u64) -> bool {
    ptr != 0
        && ptr % SYSC_PAGE_BYTES == 0
        && len >= SYSC_PAGE_BYTES
        && len % SYSC_PAGE_BYTES == 0
        && len <= SYSC_ZC_MAX_PAGES * SYSC_PAGE_BYTES
}

/// 本域页大小（避免跨模块引用 m700vmm 的常量名）。
pub const SYSC_PAGE_BYTES: u64 = 4096;

// ===========================================================================
// F068 — 调用限流阀：令牌桶，refill 封顶
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct SyscLimiter {
    pub tokens: u32,
    pub cap: u32,
}

impl SyscLimiter {
    pub const fn new(cap: u32) -> SyscLimiter {
        SyscLimiter { tokens: 0, cap }
    }

    /// 补充令牌，返回实际补充数（不超过桶容量）。
    pub fn refill(&mut self, n: u32) -> u32 {
        let space = self.cap - self.tokens;
        let granted = if n < space { n } else { space };
        self.tokens += granted;
        granted
    }

    pub fn take(&mut self) -> bool {
        if self.tokens == 0 {
            return false;
        }
        self.tokens -= 1;
        true
    }
}

// ===========================================================================
// F069 — 号段版本协商：版本差 ≤ 1 即可互通
// ===========================================================================

pub fn version_compatible(a: u32, b: u32) -> bool {
    let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
    hi - lo <= 1
}

// ===========================================================================
// F070 — 系统调用金样本：固定输入 → 固定输出剧本
// ===========================================================================

/// 金样剧本：1 号加一，2 号翻倍，其余未定义。
pub fn golden(nr: u32, arg: u64) -> Option<u64> {
    if nr == 1 {
        Some(arg.wrapping_add(1))
    } else if nr == 2 {
        Some(arg.wrapping_mul(2))
    } else {
        None
    }
}

pub fn golden_matches(nr: u32, arg: u64, expect: Option<u64>) -> bool {
    golden(nr, arg) == expect
}

// ===========================================================================
// F071 — 内嵌断言网：入口参数 < 2^48，返回值落在 errno 语义区间
// ===========================================================================

pub fn pre_ok(arg: u64) -> bool {
    arg < (1u64 << 48)
}

/// 内核返回值要么 ≥ 0，要么是 -1..=-4095 的 errno 编码。
pub fn post_ok(ret: i64) -> bool {
    ret >= -4095
}

// ===========================================================================
// F072 — 调用树视图：嵌套深度追踪，记录历史最深
// ===========================================================================

pub const SYSC_TREE_MAX_DEPTH: u32 = 4;

#[derive(Clone, Copy, Debug)]
pub struct SyscTree {
    pub depth: u32,
    pub max_seen: u32,
}

impl SyscTree {
    pub const fn new() -> SyscTree {
        SyscTree { depth: 0, max_seen: 0 }
    }

    pub fn enter(&mut self) -> bool {
        if self.depth >= SYSC_TREE_MAX_DEPTH {
            return false;
        }
        self.depth += 1;
        if self.depth > self.max_seen {
            self.max_seen = self.depth;
        }
        true
    }

    pub fn leave(&mut self) -> bool {
        if self.depth == 0 {
            return false;
        }
        self.depth -= 1;
        true
    }
}

// ===========================================================================
// F073 — 长调用看门狗：按滞留 tick 三档裁决
// ===========================================================================

pub const SYSC_WDT_HEALTHY_TICKS: u32 = 100;
pub const SYSC_WDT_KILL_TICKS: u32 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyscWatchdog {
    Healthy,
    Warn,
    Kill,
}

pub fn watchdog_verdict(ticks: u32) -> SyscWatchdog {
    if ticks <= SYSC_WDT_HEALTHY_TICKS {
        SyscWatchdog::Healthy
    } else if ticks <= SYSC_WDT_KILL_TICKS {
        SyscWatchdog::Warn
    } else {
        SyscWatchdog::Kill
    }
}

// ===========================================================================
// F074 — 调用隐私边界：返回值不得携带内核空间指针
// ===========================================================================

pub fn ret_pointer_safe(ret: u64) -> bool {
    ret < SYSC_USER_TOP
}

// ===========================================================================
// F075 — 系统调用年报：年报章节完备性
// ===========================================================================

pub const SYSC_REPORT_SECTIONS: [&str; 5] =
    ["dispatch", "contracts", "sandbox", "replay", "watchdog"];

pub fn sysc_report_complete(sections_filled: u32) -> bool {
    sections_filled >= SYSC_REPORT_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m700sysc_checks() -> CheckSet {
    let mut set = CheckSet::new("m700sysc");

    // F051 分发跳转表
    let mut table = SyscTable::new();
    let r1 = table.register(5, sysc_inc);
    let rdup = table.register(5, sysc_double);
    let roob = table.register(SYSC_TABLE_SLOTS, sysc_inc);
    set.add(
        "F051 table registry",
        r1 && !rdup && !roob && table.registered() == 1,
        "dedup + range guard",
    );
    let d1 = table.dispatch(5, 41);
    let d2 = table.dispatch(6, 1);
    let d3 = table.dispatch(SYSC_TABLE_SLOTS, 1);
    set.add(
        "F051 table dispatch",
        d1 == Some(42) && d2.is_none() && d3.is_none(),
        "known hit, unknown miss",
    );
    let r2 = table.register(6, sysc_double);
    let d4 = table.dispatch(6, 21);
    set.add(
        "F051 table second slot",
        r2 && d4 == Some(42) && table.registered() == 2,
        "independent slots",
    );

    // F052 成熟号段登记处
    let mut mat = SyscMaturity::new();
    let m1 = mat.mark_mature(10);
    let mdup = mat.mark_mature(10);
    let moob = mat.mark_mature(200);
    set.add(
        "F052 maturity marks",
        m1 && !mdup && !moob && mat.is_mature(10) && !mat.is_mature(11),
        "dedup + range",
    );
    set.add("F052 maturity count", mat.count == 1, "one mark registered");

    // F053 参数深查官
    set.add(
        "F053 args happy path",
        args_ok(0x1000, 16, SYSC_USER_TOP),
        "in-range pointer",
    );
    set.add(
        "F053 args rejects",
        !args_ok(0, 16, SYSC_USER_TOP)
            && !args_ok(0x1000, 0, SYSC_USER_TOP)
            && !args_ok(0x1000, 5000, SYSC_USER_TOP)
            && !args_ok(SYSC_USER_TOP - 8, 16, SYSC_USER_TOP),
        "null, empty, oversized, overflow",
    );

    // F054 系统调用回执
    let r_err = make_receipt(9, -22);
    set.add(
        "F054 receipt error",
        r_err.errno == 22 && r_err.ret == -22 && r_err.nr == 9,
        "negative ret folded",
    );
    let r_ok = make_receipt(1, 5);
    set.add(
        "F054 receipt success",
        r_ok.errno == 0 && receipt_errno(-1) == 1,
        "success zero errno",
    );

    // F055 热路径白名单
    set.add(
        "F055 hot members",
        hot_allowed(0) && hot_allowed(3) && hot_allowed(10),
        "whitelist hit",
    );
    set.add(
        "F055 hot non-members",
        !hot_allowed(4) && !hot_allowed(999),
        "outside whitelist",
    );

    // F056 慢路径记账
    let mut book = SyscSlowBook::default();
    let b1 = book.record(100);
    let b2 = book.record(300);
    set.add("F056 slow avg", b1 && b2 && book.avg_cycles() == 200, "mean of two");
    let mut full_book = SyscSlowBook { calls: SYSC_SLOW_RECORD_CAP, cycles: 0 };
    set.add("F056 slow cap", !full_book.record(1), "sample cap enforced");

    // F057 调用契约书
    let c_ok = SyscContract { nr: 1, args: 3, may_write_user: true };
    set.add(
        "F057 contract match",
        contract_ok(c_ok, 3) && !contract_ok(c_ok, 2),
        "exact arg count",
    );
    let c_bad = SyscContract { nr: 2, args: 7, may_write_user: false };
    set.add("F057 contract overflow", !contract_ok(c_bad, 7), "args > 6 illegal");

    // F058 号段考古队
    set.add(
        "F058 archaeo found",
        legacy_to_modern(2) == Some(22) && legacy_to_modern(9) == Some(99),
        "table lookup",
    );
    set.add(
        "F058 archaeo miss",
        legacy_to_modern(7).is_none() && is_legacy_nr(9) && !is_legacy_nr(8),
        "unmapped legacy",
    );

    // F059 兼容垫片表
    let joined32 = shim_join(0, 0xFFFF_FFFF);
    set.add(
        "F059 shim u32",
        joined32 == 0xFFFF_FFFF && shim_is_u32(joined32),
        "fits in 32 bits",
    );
    let joined64 = shim_join(1, 0);
    set.add(
        "F059 shim pair",
        joined64 == 0x1_0000_0000 && !shim_is_u32(joined64),
        "needs hi/lo pair",
    );
    let rt = shim_join(0xDEAD_BEEF, 0xCAFE_BABE);
    set.add(
        "F059 shim roundtrip",
        shim_hi(rt) == 0xDEAD_BEEF && shim_lo(rt) == 0xCAFE_BABE,
        "join/split identity",
    );

    // F060 系统调用沙盒策略
    let mut sb = SyscSandbox::new();
    let s1 = sb.deny(9);
    let sdup = sb.deny(9);
    let soob = sb.deny(100);
    set.add(
        "F060 sandbox deny",
        s1 && !sdup && !soob && sb.count == 1,
        "dedup + range",
    );
    set.add(
        "F060 sandbox allow",
        sb.allowed(8) && !sb.allowed(9) && !sb.allowed(64),
        "deny list enforced",
    );

    // F061 参数序列化规范
    let ser1 = serialize_args(7, 0x1234_5678);
    set.add(
        "F061 serde roundtrip",
        serde_tag(ser1) == 7 && serde_payload(ser1) == 0x1234_5678,
        "tag + payload intact",
    );
    let ser2 = serialize_args(0xFF, 0xFFFF_FFFF);
    set.add(
        "F061 serde extremes",
        serde_tag(ser2) == 0xFF && serde_payload(ser2) == 0xFFFF_FFFF,
        "no bleed between fields",
    );

    // F062 可重入门禁
    let mut re = SyscReentry::new(SYSC_REENTRY_DEFAULT_MAX);
    let e1 = re.enter();
    let e2 = re.enter();
    let e3 = re.enter();
    let depth_full = re.depth;
    let e4 = re.enter();
    set.add(
        "F062 reentry fill",
        e1 && e2 && e3 && depth_full == 3 && !e4,
        "cap enforced",
    );
    let l1 = re.leave();
    let l2 = re.leave();
    let l3 = re.leave();
    let l4 = re.leave();
    set.add(
        "F062 reentry drain",
        l1 && l2 && l3 && !l4 && re.depth == 0,
        "no underflow",
    );

    // F063 调用耗时直方图
    let mut hist = SyscHistogram::new();
    hist.record(0);
    hist.record(2);
    hist.record(10);
    hist.record(50);
    hist.record(100);
    set.add(
        "F063 hist spread",
        hist.buckets == [1, 1, 1, 1, 1] && hist.total() == 5,
        "one per bucket",
    );
    set.add(
        "F063 hist boundaries",
        SyscHistogram::bucket_of(1) == 0
            && SyscHistogram::bucket_of(4) == 1
            && SyscHistogram::bucket_of(16) == 2
            && SyscHistogram::bucket_of(64) == 3
            && SyscHistogram::bucket_of(65) == 4,
        "edges land exactly",
    );

    // F064 半途状态清理
    let mut cl = SyscCleanup::default();
    let st1 = cl.stage(5);
    let st2 = cl.stage(12);
    set.add(
        "F064 stage cap",
        st1 && !st2 && cl.staged == 5,
        "partial staging rejected as a whole",
    );
    let aborted = cl.abort();
    let st3 = cl.stage(6);
    let committed = cl.commit();
    set.add(
        "F064 abort and commit",
        aborted == 5 && cl.staged == 0 && st3 && committed == 6 && cl.committed == 6,
        "abort clears, commit settles",
    );

    // F065 系统调用回放流
    let mut rp = SyscReplay::new();
    let mut i = 0u64;
    while i < 10 {
        rp.push(i as u32, i * 10);
        i += 1;
    }
    set.add(
        "F065 replay wrap",
        rp.pushed == 10 && rp.dropped() == 2,
        "ring holds 8, rest dropped",
    );
    set.add(
        "F065 replay content",
        rp.nr_at(8) == 8 && rp.nr_at(2) == 2 && rp.arg_at(9) == 90,
        "slot reuse order",
    );

    // F066 errno 语言表
    let all: [SyscErrno; 6] = [
        SyscErrno::Ok,
        SyscErrno::NoEntry,
        SyscErrno::BadFd,
        SyscErrno::NoMem,
        SyscErrno::AccessDenied,
        SyscErrno::NotSupported,
    ];
    let mut roundtrip = true;
    let mut j = 0usize;
    while j < all.len() {
        if errno_from_code(errno_code(all[j])) != Some(all[j]) {
            roundtrip = false;
        }
        j += 1;
    }
    set.add("F066 errno roundtrip", roundtrip, "bidirectional map");
    set.add(
        "F066 errno unknown",
        errno_from_code(7).is_none() && errno_code(SyscErrno::Ok) == 0,
        "unmapped code rejected",
    );

    // F067 零拷贝调用通道
    set.add(
        "F067 zc happy path",
        zero_copy_ok(0x2000, SYSC_PAGE_BYTES),
        "page aligned full page",
    );
    set.add(
        "F067 zc rejects",
        !zero_copy_ok(0x2001, SYSC_PAGE_BYTES)
            && !zero_copy_ok(0, SYSC_PAGE_BYTES)
            && !zero_copy_ok(0x2000, 100)
            && !zero_copy_ok(0x2000, 65 * SYSC_PAGE_BYTES),
        "unaligned, null, partial, over-cap",
    );

    // F068 调用限流阀
    let mut lim = SyscLimiter::new(5);
    let r1 = lim.refill(3);
    let t1 = lim.take();
    let t2 = lim.take();
    let t3 = lim.take();
    let t4 = lim.take();
    set.add(
        "F068 limiter drain",
        r1 == 3 && t1 && t2 && t3 && !t4,
        "three tokens then dry",
    );
    let r2 = lim.refill(10);
    let t5 = lim.take();
    set.add(
        "F068 limiter clamp",
        r2 == 5 && t5 && lim.tokens == 4,
        "refill capped at bucket size",
    );

    // F069 号段版本协商
    set.add(
        "F069 version ok",
        version_compatible(3, 3) && version_compatible(3, 4) && version_compatible(4, 3),
        "one step apart fine",
    );
    set.add("F069 version gap", !version_compatible(3, 5), "two steps incompatible");

    // F070 系统调用金样本
    set.add(
        "F070 golden hits",
        golden(1, 41) == Some(42) && golden(2, 21) == Some(42),
        "scripted outputs",
    );
    set.add(
        "F070 golden miss",
        golden(3, 0).is_none()
            && golden_matches(2, 21, Some(42))
            && !golden_matches(2, 21, Some(43)),
        "drift caught",
    );

    // F071 内嵌断言网
    set.add(
        "F071 pre net",
        pre_ok(0) && pre_ok((1u64 << 48) - 1) && !pre_ok(1u64 << 48),
        "arg < 2^48",
    );
    set.add(
        "F071 post net",
        post_ok(0) && post_ok(42) && post_ok(-1) && post_ok(-4095) && !post_ok(-4096),
        "errno range [-4095,-1]",
    );

    // F072 调用树视图
    let mut tree = SyscTree::new();
    let n1 = tree.enter();
    let n2 = tree.enter();
    let n3 = tree.enter();
    let n4 = tree.enter();
    let n5 = tree.enter();
    set.add(
        "F072 tree depth cap",
        n1 && n2 && n3 && n4 && !n5 && tree.max_seen == 4 && tree.depth == 4,
        "max depth recorded",
    );
    let lv = tree.leave();
    set.add(
        "F072 tree unwind",
        lv && tree.depth == 3 && tree.max_seen == 4,
        "peak preserved on unwind",
    );

    // F073 长调用看门狗
    set.add(
        "F073 wdt tiers",
        watchdog_verdict(SYSC_WDT_HEALTHY_TICKS) == SyscWatchdog::Healthy
            && watchdog_verdict(101) == SyscWatchdog::Warn
            && watchdog_verdict(SYSC_WDT_KILL_TICKS) == SyscWatchdog::Warn,
        "healthy and warn bands",
    );
    set.add(
        "F073 wdt kill",
        watchdog_verdict(1001) == SyscWatchdog::Kill
            && watchdog_verdict(99999) == SyscWatchdog::Kill,
        "over budget dies",
    );

    // F074 调用隐私边界
    set.add(
        "F074 privacy user ptr",
        ret_pointer_safe(0x1000),
        "user pointer fine",
    );
    set.add(
        "F074 privacy kernel ptr",
        !ret_pointer_safe(0xFFFF_8000_0000_0000) && !ret_pointer_safe(SYSC_USER_TOP),
        "kernel range leaked",
    );

    // F075 系统调用年报
    set.add(
        "F075 sysc report",
        SYSC_REPORT_SECTIONS.len() == 5 && sysc_report_complete(5) && !sysc_report_complete(4),
        "sections complete",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f051_dispatch_registry() {
        let mut t = SyscTable::new();
        assert!(t.register(0, sysc_double));
        assert!(!t.register(0, sysc_inc));
        assert_eq!(t.dispatch(0, 5), Some(10));
        assert_eq!(t.dispatch(63, 1), None);
        assert!(t.register(63, sysc_inc));
        assert_eq!(t.dispatch(63, 1), Some(2));
        assert_eq!(t.registered(), 2);
    }

    #[test]
    fn f056_slow_book_math() {
        let mut b = SyscSlowBook::default();
        assert!(b.record(10));
        assert!(b.record(20));
        assert!(b.record(30));
        assert_eq!(b.avg_cycles(), 20);
        assert_eq!(b.calls, 3);
        assert_eq!(SyscSlowBook::default().avg_cycles(), 0);
    }

    #[test]
    fn f062_reentry_gate() {
        let mut r = SyscReentry::new(2);
        assert!(r.enter());
        assert!(r.enter());
        assert!(!r.enter());
        assert!(r.leave());
        assert!(r.enter());
        assert_eq!(r.depth, 2);
        assert!(r.leave());
        assert!(r.leave());
        assert!(!r.leave());
    }

    #[test]
    fn f063_histogram_buckets() {
        let mut h = SyscHistogram::new();
        h.record(1);
        h.record(4);
        h.record(16);
        h.record(64);
        h.record(65);
        h.record(1);
        assert_eq!(h.buckets, [2, 1, 1, 1, 1]);
        assert_eq!(h.total(), 6);
    }

    #[test]
    fn f065_replay_ring_wrap() {
        let mut r = SyscReplay::new();
        let mut i = 0u64;
        while i < 20 {
            r.push((i * 2) as u32, i);
            i += 1;
        }
        assert_eq!(r.pushed, 20);
        assert_eq!(r.dropped(), 12);
        // 环内每个槽位都已被第 12~19 次推送覆盖一遍。
        assert_eq!(r.nr_at(19), 38);
        assert_eq!(r.nr_at(12), 24);
        assert_eq!(r.arg_at(19), 19);
    }

    #[test]
    fn m700sysc_selfcheck_all_pass() {
        let set = run_m700sysc_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
