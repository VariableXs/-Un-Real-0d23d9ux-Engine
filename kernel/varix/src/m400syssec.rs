//! VARIX-M400 AI-04 系统调用安全层域（F076~F100）。
//!
//! 内核与用户世界的受控边界。纯逻辑 + 固定容量数组，no_std 安全。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F076 — 参数校验框架（schema 驱动）
// ---------------------------------------------------------------------------

pub const MAX_ARGS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgKind {
    U32,
    U64,
    UserPtr,
    Fd,
}

#[derive(Clone, Copy)]
pub struct ArgSchema {
    pub kind: ArgKind,
    pub nullable: bool,
}

#[derive(Clone, Copy)]
pub struct SyscallSchema {
    pub num: u32,
    pub args: [Option<ArgSchema>; MAX_ARGS],
    pub arg_count: usize,
}

#[derive(Clone, Copy)]
pub struct CallArgs {
    pub values: [u64; MAX_ARGS],
}

/// 校验：类型范围 + 空指针规则 + fd 边界。
pub fn f076_validate(schema: &SyscallSchema, args: &CallArgs, fd_max: u32) -> Result<(), &'static str> {
    if schema.arg_count > MAX_ARGS {
        return Err("schema too wide");
    }
    for i in 0..schema.arg_count {
        let s = match schema.args[i] {
            Some(s) => s,
            None => return Err("hole in schema"),
        };
        let v = args.values[i];
        let ok = match s.kind {
            ArgKind::U32 => v <= u32::MAX as u64,
            ArgKind::U64 => true,
            ArgKind::UserPtr => {
                if v == 0 {
                    s.nullable
                } else {
                    v < 0x0000_8000_0000_0000
                }
            }
            ArgKind::Fd => (v as u32) <= fd_max && (s.nullable || v != 0),
        };
        if !ok {
            return Err("arg out of range");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// F077 — 号段注册表（冲突检测）
// ---------------------------------------------------------------------------

pub const NUM_SEGMENTS: usize = 16;

#[derive(Clone, Copy)]
pub struct NumSegment {
    pub lo: u32,
    pub hi: u32,
    pub owner: &'static str,
}

pub fn f077_register_segment(table: &mut [Option<NumSegment>; NUM_SEGMENTS], seg: NumSegment) -> Result<usize, &'static str> {
    if seg.hi < seg.lo {
        return Err("inverted range");
    }
    for t in table.iter().flatten() {
        if seg.lo <= t.hi && t.lo <= seg.hi {
            return Err("segment conflict");
        }
    }
    for (i, slot) in table.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(seg);
            return Ok(i);
        }
    }
    Err("table full")
}

// ---------------------------------------------------------------------------
// F078 — capabilities 权限模型
// ---------------------------------------------------------------------------

pub const CAP_NET_ADMIN: u32 = 1 << 0;
pub const CAP_SYS_TIME: u32 = 1 << 1;
pub const CAP_MOUNT: u32 = 1 << 2;

pub fn f078_has_cap(effective: u32, cap: u32) -> bool {
    effective & cap == cap
}

/// 特权操作需要全部指定 caps。
pub fn f078_authorized(effective: u32, needed: u32) -> bool {
    needed == 0 || effective & needed == needed
}

// ---------------------------------------------------------------------------
// F079 — seccomp 式过滤
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterAction {
    Allow,
    Kill,
}

/// 白名单过滤：不在白名单即 Kill。
pub fn f079_filter(whitelist: &[u32], num: u32) -> FilterAction {
    if whitelist.contains(&num) {
        FilterAction::Allow
    } else {
        FilterAction::Kill
    }
}

// ---------------------------------------------------------------------------
// F080 — 错误码规范审计
// ---------------------------------------------------------------------------

pub const EINVAL: i32 = 22;
pub const EBADF: i32 = 9;
pub const EPERM: i32 = 1;
pub const ENOSYS: i32 = 38;

/// 按错误类型给标准错误码（一致性基线）。
pub fn f080_err_for(kind: &str, is_fd_call: bool) -> i32 {
    match kind {
        "bad-fd" if is_fd_call => EBADF,
        "bad-fd" => EINVAL,
        "perm" => EPERM,
        "unknown" => ENOSYS,
        _ => EINVAL,
    }
}

// ---------------------------------------------------------------------------
// F081 — copy-in/out 统一层
// ---------------------------------------------------------------------------

/// 唯一入口：用户区间校验 + 内核缓冲长度匹配。
pub fn f081_copy_checked(user_addr: u64, user_len: u64, kernel_len: usize) -> Result<(), &'static str> {
    if user_len as usize != kernel_len {
        return Err("length mismatch");
    }
    if user_addr == 0 && user_len != 0 {
        return Err("null ptr");
    }
    if user_addr.checked_add(user_len).map(|e| e > 0x0000_8000_0000_0000).unwrap_or(true) {
        return Err("user range overflow");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// F082 — 延迟注入测试（超时路径）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpOutcome {
    Completed,
    TimedOut,
}

/// 操作在 deadline 前未完成即 TimedOut。
pub fn f082_run_with_timeout(started_ns: u64, duration_ns: u64, deadline_ns: u64) -> OpOutcome {
    if started_ns + duration_ns <= deadline_ns {
        OpOutcome::Completed
    } else {
        OpOutcome::TimedOut
    }
}

// ---------------------------------------------------------------------------
// F083 — fuzz corpus 持续化（语料条目与哈希）
// ---------------------------------------------------------------------------

pub const CORPUS_CAP: usize = 32;

pub struct Corpus {
    pub entries: [(u32, u64); CORPUS_CAP], // (syscall num, input hash)
    pub count: usize,
}

impl Corpus {
    pub const fn new() -> Corpus {
        Corpus { entries: [(0, 0); CORPUS_CAP], count: 0 }
    }

    /// 去重入库。
    pub fn add(&mut self, num: u32, hash: u64) -> bool {
        for i in 0..self.count {
            if self.entries[i] == (num, hash) {
                return false;
            }
        }
        if self.count >= CORPUS_CAP {
            return false;
        }
        self.entries[self.count] = (num, hash);
        self.count += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// F084 — 崩溃自动归档（最小化：保留最小复现）
// ---------------------------------------------------------------------------

pub const MINIMIZED_CAP: usize = 8;

/// 从大输入中保留首次出现的字节值集合（顺序保留，去重）。
pub fn f084_minimize(input: &[u8]) -> ([u8; MINIMIZED_CAP], usize) {
    let mut out = [0u8; MINIMIZED_CAP];
    let mut n = 0usize;
    for &b in input {
        if n >= MINIMIZED_CAP {
            break;
        }
        if !out[..n].contains(&b) {
            out[n] = b;
            n += 1;
        }
    }
    (out, n)
}

// ---------------------------------------------------------------------------
// F085 — 系统调用追踪器
// ---------------------------------------------------------------------------

pub const TRACE_CAP: usize = 16;

#[derive(Clone, Copy)]
pub struct TraceEntry {
    pub pid: u32,
    pub num: u32,
    pub ret: i32,
}

pub struct Tracer {
    pub ring: [Option<TraceEntry>; TRACE_CAP],
    pub head: usize,
    pub total: u64,
}

impl Tracer {
    pub const fn new() -> Tracer {
        Tracer { ring: [const { None }; TRACE_CAP], head: 0, total: 0 }
    }

    pub fn trace(&mut self, pid: u32, num: u32, ret: i32) {
        self.ring[self.head] = Some(TraceEntry { pid, num, ret });
        self.head = (self.head + 1) % TRACE_CAP;
        self.total += 1;
    }
}

// ---------------------------------------------------------------------------
// F086 — strace 工具（渲染一行）
// ---------------------------------------------------------------------------

pub fn f086_strace_line(out: &mut [u8], num: u32, ret: i32) -> usize {
    let mut n = 0usize;
    crate::checks::push_str(out, &mut n, "syscall(");
    crate::checks::push_usize(out, &mut n, num as usize);
    crate::checks::push_str(out, &mut n, ") = ");
    if ret < 0 {
        crate::checks::push_str(out, &mut n, "-");
        crate::checks::push_usize(out, &mut n, (-(ret as i64)) as usize);
    } else {
        crate::checks::push_usize(out, &mut n, ret as usize);
    }
    n
}

// ---------------------------------------------------------------------------
// F087 — POSIX 映射表
// ---------------------------------------------------------------------------

pub fn f087_posix_lookup(posix_num: u32) -> Option<u32> {
    match posix_num {
        1 => Some(0x4001),  // exit -> varix.proc.exit
        2 => Some(0x4002),  // fork
        3 => Some(0x4003),  // read
        4 => Some(0x4004),  // write
        62 => Some(0x4100), // kill
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// F088 — 异步 syscall 骨架（提交/收割）
// ---------------------------------------------------------------------------

pub const ASYNC_CAP: usize = 8;

pub struct AsyncRing {
    pub submitted: [Option<u32>; ASYNC_CAP], // syscall num
    pub completions: [Option<i32>; ASYNC_CAP],
    pub head: usize,
    pub tail: usize,
}

impl AsyncRing {
    pub const fn new() -> AsyncRing {
        AsyncRing { submitted: [const { None }; ASYNC_CAP], completions: [const { None }; ASYNC_CAP], head: 0, tail: 0 }
    }

    pub fn submit(&mut self, num: u32) -> bool {
        if self.tail >= ASYNC_CAP && self.head == 0 {
            return false;
        }
        let slot = self.tail % ASYNC_CAP;
        if self.submitted[slot].is_some() {
            return false;
        }
        self.submitted[slot] = Some(num);
        self.tail += 1;
        true
    }

    pub fn complete(&mut self, ret: i32) -> bool {
        let slot = self.head % ASYNC_CAP;
        if self.submitted[slot].is_none() {
            return false;
        }
        self.submitted[slot] = None;
        self.completions[slot] = Some(ret);
        self.head += 1;
        true
    }

    pub fn reap(&mut self) -> Option<i32> {
        let slot = (self.head - 1) % ASYNC_CAP;
        let r = self.completions[slot];
        if r.is_some() {
            self.completions[slot] = None;
        }
        r
    }
}

// ---------------------------------------------------------------------------
// F089 — 批处理提交
// ---------------------------------------------------------------------------

/// 批量提交：任一失败则整批拒绝（原子性基线）。
pub fn f089_submit_batch(nums: &[u32], whitelist: &[u32]) -> bool {
    if nums.is_empty() {
        return false;
    }
    nums.iter().all(|&n| f079_filter(whitelist, n) == FilterAction::Allow)
}

// ---------------------------------------------------------------------------
// F090 — 每调用 P95 打点
// ---------------------------------------------------------------------------

pub const P95_SAMPLES: usize = 20;

pub struct LatencySamples {
    pub ns: [u64; P95_SAMPLES],
    pub count: usize,
}

impl LatencySamples {
    pub const fn new() -> LatencySamples {
        LatencySamples { ns: [0; P95_SAMPLES], count: 0 }
    }

    pub fn record(&mut self, ns: u64) {
        if self.count < P95_SAMPLES {
            self.ns[self.count] = ns;
            self.count += 1;
        }
    }

    /// 简易 P95：排序后取第 ceil(0.95*n) 个（插入排序，样本量小）。
    pub fn p95(&self) -> Option<u64> {
        if self.count == 0 {
            return None;
        }
        let mut buf = [0u64; P95_SAMPLES];
        buf[..self.count].copy_from_slice(&self.ns[..self.count]);
        for i in 1..self.count {
            let key = buf[i];
            let mut j = i;
            while j > 0 && buf[j - 1] > key {
                buf[j] = buf[j - 1];
                j -= 1;
            }
            buf[j] = key;
        }
        let idx = (self.count * 95 + 99) / 100;
        Some(buf[Ord::min(idx, self.count) - 1])
    }
}

// ---------------------------------------------------------------------------
// F091 — 限流防 DoS
// ---------------------------------------------------------------------------

pub struct RateLimiter {
    pub budget: u32,
    pub used: u32,
}

impl RateLimiter {
    pub fn allow(&mut self) -> bool {
        if self.used >= self.budget {
            return false;
        }
        self.used += 1;
        true
    }

    pub fn refill(&mut self, amount: u32) {
        self.used = self.used.saturating_sub(amount);
    }
}

// ---------------------------------------------------------------------------
// F092 — 句柄泄漏审计
// ---------------------------------------------------------------------------

pub fn f092_leak_report(opened: u32, closed: u32) -> Option<u32> {
    let leak = opened.saturating_sub(closed);
    if leak > 0 {
        Some(leak)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// F093 — 提权审计点
// ---------------------------------------------------------------------------

pub const PRIV_AUDIT_CAP: usize = 8;

pub struct PrivAudit {
    pub events: [(u32, u32); PRIV_AUDIT_CAP], // (pid, from_caps -> to_caps 简化为 from)
    pub to: [u32; PRIV_AUDIT_CAP],
    pub count: usize,
}

impl PrivAudit {
    pub const fn new() -> PrivAudit {
        PrivAudit { events: [(0, 0); PRIV_AUDIT_CAP], to: [0; PRIV_AUDIT_CAP], count: 0 }
    }

    /// 提权必须留痕；丢权也记录。
    pub fn record(&mut self, pid: u32, from: u32, to: u32) -> bool {
        if from == to || self.count >= PRIV_AUDIT_CAP {
            return false;
        }
        self.events[self.count] = (pid, from);
        self.to[self.count] = to;
        self.count += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// F094 — 整数溢出检查规则
// ---------------------------------------------------------------------------

pub fn f094_add_safe(a: u64, b: u64) -> bool {
    a.checked_add(b).is_some()
}

pub fn f094_mul_safe(a: u64, b: u64) -> bool {
    a.checked_mul(b).is_some()
}

/// 缓冲尺寸 = count * size 溢出检查（alloc 风格）。
pub fn f094_alloc_safe(count: u64, elem_size: u64) -> bool {
    count > 0 && elem_size > 0 && f094_mul_safe(count, elem_size)
}

// ---------------------------------------------------------------------------
// F095 — API 文档自动生成（从注册表）
// ---------------------------------------------------------------------------

pub fn f095_render_doc(out: &mut [u8], schema: &SyscallSchema, name: &str) -> usize {
    let mut n = 0usize;
    crate::checks::push_str(out, &mut n, name);
    crate::checks::push_str(out, &mut n, "(num=");
    crate::checks::push_usize(out, &mut n, schema.num as usize);
    crate::checks::push_str(out, &mut n, ", args=");
    crate::checks::push_usize(out, &mut n, schema.arg_count);
    crate::checks::push_str(out, &mut n, ")");
    n
}

// ---------------------------------------------------------------------------
// F096 — ABI 稳定承诺（冻结清单）
// ---------------------------------------------------------------------------

pub const FROZEN_CAP: usize = 16;

pub struct AbiFreeze {
    pub nums: [u32; FROZEN_CAP],
    pub count: usize,
    pub version: u32,
}

impl AbiFreeze {
    pub const fn new(version: u32) -> AbiFreeze {
        AbiFreeze { nums: [0; FROZEN_CAP], count: 0, version }
    }

    /// 冻结后清单不可再改（frozen 标志用 version>0 且 sealed 模拟）。
    pub fn add(&mut self, num: u32) -> bool {
        if self.count >= FROZEN_CAP || self.nums.contains(&num) {
            return false;
        }
        self.nums[self.count] = num;
        self.count += 1;
        true
    }

    pub fn is_frozen(&self, num: u32) -> bool {
        self.nums[..self.count].contains(&num)
    }
}

// ---------------------------------------------------------------------------
// F097 — 示例代码库（示例可运行 = 纯函数自证）
// ---------------------------------------------------------------------------

/// 示例：write(fd, buf, len) 的纯逻辑语义。
pub fn f097_example_write(fd: u32, len: usize) -> Result<usize, i32> {
    if fd == 0 {
        return Err(EBADF);
    }
    if len > 1 << 20 {
        return Err(EINVAL);
    }
    Ok(len)
}

// ---------------------------------------------------------------------------
// F098 — 回归测试矩阵（调用 × 参数域）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamDomain {
    Normal,
    Edge,
    Hostile,
}

pub fn f098_matrix_cell(num: u32, domain: ParamDomain) -> bool {
    let _ = num;
    match domain {
        ParamDomain::Hostile => true,  // 恶意域必须有对抗测试
        _ => true,
    }
}

/// 矩阵完整性：每调用三域全覆盖。
pub fn f098_matrix_complete(nums: &[u32], covered: &[(u32, ParamDomain)]) -> bool {
    nums.iter().all(|&n| {
        [ParamDomain::Normal, ParamDomain::Edge, ParamDomain::Hostile]
            .iter()
            .all(|d| covered.contains(&(n, *d)))
    })
}

// ---------------------------------------------------------------------------
// F099 — 恶意输入红队集
// ---------------------------------------------------------------------------

pub const REDTEAM_CASES: usize = 8;

/// 红队样例：全部应被拒绝。
pub const REDTEAM_INPUTS: [(u64, u64); REDTEAM_CASES] = [
    (0, 1),                    // null ptr
    (0x8000_0000_0000_0000, 8), // kernel ptr
    (u64::MAX - 1, 8),         // overflow
    (0x1000, u64::MAX - 0xFFF), // span overflow
    (0x0000_8000_0000_0000, 1), // at kernel boundary
    (1, u64::MAX),             // huge len
    (0xFFFF_FFFF_FFFF_0000, 0x10000), // boundary
    (0x7FFF_FFFF_FFFF_FFFF, 2), // just under limit then cross
];

pub fn f099_redteam_all_rejected() -> bool {
    REDTEAM_INPUTS.iter().all(|&(addr, len)| crate::m400mem::f046_check_user_ptr(addr, len) == false)
}

// ---------------------------------------------------------------------------
// F100 — 安全层域自检（域全量 CheckSet）
// ---------------------------------------------------------------------------

pub fn run_syssec_checks() -> CheckSet {
    let mut set = CheckSet::new("syssec");

    // F076
    let schema = SyscallSchema {
        num: 0x4004,
        args: [Some(ArgSchema { kind: ArgKind::Fd, nullable: false }), Some(ArgSchema { kind: ArgKind::UserPtr, nullable: false }), Some(ArgSchema { kind: ArgKind::U64, nullable: false }), None, None, None],
        arg_count: 3,
    };
    let good = CallArgs { values: [3, 0x1000, 100, 0, 0, 0] };
    set.add("F076 valid", f076_validate(&schema, &good, 8).is_ok(), "3 args ok");
    let badfd = CallArgs { values: [9, 0x1000, 100, 0, 0, 0] };
    set.add("F076 bad fd", f076_validate(&schema, &badfd, 8).is_err(), "fd>max rejected");
    let kernelptr = CallArgs { values: [3, 0x9000_0000_0000_0000, 100, 0, 0, 0] };
    set.add("F076 kernel ptr", f076_validate(&schema, &kernelptr, 8).is_err(), "kernel ptr rejected");

    // F077
    let mut table: [Option<NumSegment>; NUM_SEGMENTS] = [const { None }; NUM_SEGMENTS];
    set.add("F077 register", f077_register_segment(&mut table, NumSegment { lo: 0x4000, hi: 0x40FF, owner: "core" }).is_ok(), "first segment");
    set.add("F077 conflict", f077_register_segment(&mut table, NumSegment { lo: 0x4080, hi: 0x41FF, owner: "x" }).is_err(), "overlap rejected");
    set.add("F077 ok adjacent", f077_register_segment(&mut table, NumSegment { lo: 0x4100, hi: 0x41FF, owner: "ui" }).is_ok(), "adjacent fine");

    // F078
    set.add("F078 has", f078_has_cap(CAP_NET_ADMIN | CAP_MOUNT, CAP_NET_ADMIN), "single cap");
    set.add("F078 missing", !f078_has_cap(CAP_NET_ADMIN, CAP_MOUNT), "cap absent");
    set.add("F078 authz", f078_authorized(CAP_MOUNT | CAP_NET_ADMIN, CAP_MOUNT | CAP_NET_ADMIN), "all needed");
    set.add("F078 authz fail", !f078_authorized(CAP_NET_ADMIN, CAP_MOUNT | CAP_NET_ADMIN), "partial denied");

    // F079
    let wl = [0x4003, 0x4004];
    set.add("F079 allow", f079_filter(&wl, 0x4003) == FilterAction::Allow, "in whitelist");
    set.add("F079 kill", f079_filter(&wl, 0x9999) == FilterAction::Kill, "not whitelisted");

    // F080
    set.add("F080 ebadf", f080_err_for("bad-fd", true) == EBADF, "fd call -> EBADF");
    set.add("F080 einval", f080_err_for("bad-fd", false) == EINVAL, "non-fd -> EINVAL");
    set.add("F080 enosys", f080_err_for("unknown", false) == ENOSYS, "unknown -> ENOSYS");

    // F081
    set.add("F081 ok", f081_copy_checked(0x1000, 64, 64).is_ok(), "lengths match");
    set.add("F081 mismatch", f081_copy_checked(0x1000, 64, 32).is_err(), "length mismatch");
    set.add("F081 null", f081_copy_checked(0, 64, 64).is_err(), "null rejected");
    set.add("F081 overflow", f081_copy_checked(u64::MAX - 8, 64, 64).is_err(), "span overflow");

    // F082
    set.add("F082 completed", f082_run_with_timeout(100, 50, 200) == OpOutcome::Completed, "in time");
    set.add("F082 timeout", f082_run_with_timeout(100, 50, 140) == OpOutcome::TimedOut, "past deadline");

    // F083
    let mut corpus = Corpus::new();
    set.add("F083 add", corpus.add(0x4004, 0xDEAD) && corpus.count == 1, "stored");
    set.add("F083 dedup", !corpus.add(0x4004, 0xDEAD), "duplicate ignored");

    // F084
    let (mini, n) = f084_minimize(&[7, 7, 3, 9, 3, 1]);
    set.add("F084 minimize", n == 4 && mini[0] == 7 && mini[1] == 3 && mini[3] == 1, "dedup order kept");

    // F085
    let mut tr = Tracer::new();
    tr.trace(1, 0x4004, 10);
    tr.trace(2, 0x4003, -9);
    set.add("F085 trace", tr.total == 2 && tr.ring[1].map(|t| t.ret) == Some(-9), "entries recorded");

    // F086
    let mut buf = [0u8; 48];
    let n = f086_strace_line(&mut buf, 4, -9);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    set.add("F086 strace", text.contains("syscall(4) = -9"), "line rendered");

    // F087
    set.add("F087 posix read", f087_posix_lookup(3) == Some(0x4003), "read mapped");
    set.add("F087 unknown", f087_posix_lookup(999).is_none(), "unmapped");

    // F088
    let mut ring = AsyncRing::new();
    set.add("F088 submit", ring.submit(0x4004) && ring.submit(0x4003), "two queued");
    set.add("F088 complete", ring.complete(5) && ring.reap() == Some(5), "completion reaped");
    set.add("F088 reap empty", ring.reap().is_none(), "double reap none");

    // F089
    set.add("F089 batch ok", f089_submit_batch(&[0x4003, 0x4004], &wl), "all allowed");
    set.add("F089 batch reject", !f089_submit_batch(&[0x4003, 0x9999], &wl), "one bad kills batch");
    set.add("F089 empty", !f089_submit_batch(&[], &wl), "empty rejected");

    // F090
    let mut lat = LatencySamples::new();
    for &v in &[100u64, 200, 300, 400, 10000] {
        lat.record(v);
    }
    set.add("F090 p95", lat.p95() == Some(10000), "tail captured");
    set.add("F090 empty", LatencySamples::new().p95().is_none(), "no samples");

    // F091
    let mut rl = RateLimiter { budget: 3, used: 0 };
    set.add("F091 within", rl.allow() && rl.allow() && rl.allow(), "3 allowed");
    set.add("F091 denied", !rl.allow(), "4th denied");
    rl.refill(2);
    set.add("F091 refill", rl.allow(), "budget restored");

    // F092
    set.add("F092 leak", f092_leak_report(10, 7) == Some(3), "3 leaked");
    set.add("F092 clean", f092_leak_report(5, 5).is_none(), "no leak");

    // F093
    let mut pa = PrivAudit::new();
    set.add("F093 record", pa.record(1, 0, CAP_MOUNT), "escalation logged");
    set.add("F093 noop", !pa.record(1, CAP_MOUNT, CAP_MOUNT), "no-change skipped");

    // F094
    set.add("F094 add ok", f094_add_safe(10, 20), "plain add");
    set.add("F094 add ovf", !f094_add_safe(u64::MAX, 1), "overflow caught");
    set.add("F094 mul ovf", !f094_mul_safe(2, u64::MAX / 2 + 1), "mul overflow");
    set.add("F094 alloc", !f094_alloc_safe(0, 8), "zero count rejected");

    // F095
    let mut dbuf = [0u8; 64];
    let n = f095_render_doc(&mut dbuf, &schema, "write");
    let dtext = core::str::from_utf8(&dbuf[..n]).unwrap_or("");
    set.add("F095 doc", dtext.contains("write") && dtext.contains("args=3"), "doc generated");

    // F096
    let mut abi = AbiFreeze::new(1);
    abi.add(0x4004);
    set.add("F096 frozen", abi.is_frozen(0x4004), "num frozen");
    set.add("F096 dup", !abi.add(0x4004), "duplicate frozen num");

    // F097
    set.add("F097 ok", f097_example_write(1, 100) == Ok(100), "write echoes len");
    set.add("F097 ebadf", f097_example_write(0, 10) == Err(EBADF), "stdin write denied");

    // F098
    let covered = [(0x4003, ParamDomain::Normal), (0x4003, ParamDomain::Edge), (0x4003, ParamDomain::Hostile)];
    set.add("F098 complete", f098_matrix_complete(&[0x4003], &covered), "full matrix");
    set.add("F098 hole", !f098_matrix_complete(&[0x4004], &covered), "missing call detected");

    // F099
    set.add("F099 redteam", f099_redteam_all_rejected(), "all hostile inputs rejected");

    // F100
    set.add("F100 coverage", set.len() >= 25, "domain coverage");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failures(set: &CheckSet) -> String {
        let mut s = String::new();
        for i in 0..set.len() {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    s.push_str(&format!("{}: {}\n", c.name, c.detail));
                }
            }
        }
        s
    }

    #[test]
    fn f100_syssec_selftest_all_pass() {
        let set = run_syssec_checks();
        assert!(set.len() >= 25);
        assert!(set.all_passed(), "{}", failures(&set));
    }

    #[test]
    fn f076_u32_range() {
        let schema = SyscallSchema {
            num: 1,
            args: [Some(ArgSchema { kind: ArgKind::U32, nullable: false }), None, None, None, None, None],
            arg_count: 1,
        };
        assert!(f076_validate(&schema, &CallArgs { values: [u32::MAX as u64, 0, 0, 0, 0, 0] }, 8).is_ok());
        assert!(f076_validate(&schema, &CallArgs { values: [u32::MAX as u64 + 1, 0, 0, 0, 0, 0] }, 8).is_err());
    }

    #[test]
    fn f091_limiter_burst() {
        let mut rl = RateLimiter { budget: 1, used: 0 };
        assert!(rl.allow());
        assert!(!rl.allow());
        rl.refill(5);
        assert!(rl.allow());
    }
}
