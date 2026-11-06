//! UNX-F11347 /proc/self/status 关键行族（0xebb0 段）
//!
//! `/proc/self/status` 是 `Key:\tValue\n` 的行族。它看起来「只是文本」，
//! 但每种键的值有自己的格式与不变量，守护进程和监控器正是按这些不变量
//! 做热解析的：
//!
//! - `Pid`/`PPid`/`Threads` 是十进制标量，`Threads` 至少为 1；
//! - `State` 是单字母加括号解释，字母只能取 `R/S/D/Z/T/t/I`；
//! - `Uid` 是**四元组**：real / effective / saved / fs，用制表符分隔，
//!   少一列就会让读第 3、4 列的消费者错位；
//! - `VmPeak`/`VmSize` 带 `" kB"` 后缀，且峰值不能小于当前值；
//! - `SigQ` 是 `pending/limit`，limit 为 0 或 pending 超过 limit 不可能出现；
//! - `Cpus_allowed_list` 是**范围表**的规范形式：升序、去重、连续段压缩成
//!   `a-b`，段间逗号分隔。输入 `0,1,2,3,8` 与 `0-3,8` 是同一集合，但呈现
//!   必须是后者，否则按字符串比较的监控器会误判漂移。
//!
//! 本模块把 status 建模成「字段 + 逐行渲染」：先做值域闸门，再按键渲染到
//! 定容缓冲，最后按字节比对真值。渲染要求先设定 Pid（未设定报 `EWARM`），
//! 因为 status 的最小可辨识身份就是 Pid。
//!
//! 闸门次序：Pid 非零 → 状态字母合法 → 线程数非零 → 内存峰值不小于当前 →
//! SigQ 界限 → CPU 范围表合法。CPU 表解析最贵，放最后。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xebb0;

/// Pid 为零。
pub const ST_EPID: u32 = SEG_BASE + 0x01;
/// 状态字母非法。
pub const ST_ESTATE: u32 = SEG_BASE + 0x02;
/// 线程数为零。
pub const ST_ETHREADS: u32 = SEG_BASE + 0x03;
/// CPU 范围表非法。
pub const ST_ELIST: u32 = SEG_BASE + 0x04;
/// SigQ 界限非法。
pub const ST_ESIGQ: u32 = SEG_BASE + 0x05;
/// 内存峰值小于当前值。
pub const ST_EMEM: u32 = SEG_BASE + 0x06;
/// 进程名超长。
pub const ST_ENAME: u32 = SEG_BASE + 0x07;
/// 渲染前缺少 Pid。
pub const ST_EWARM: u32 = SEG_BASE + 0x08;
/// 未知行键。
pub const ST_EBADKEY: u32 = SEG_BASE + 0x09;
/// 字节下标越过已渲染长度。
pub const ST_ENOBS: u32 = SEG_BASE + 0x0a;
/// 账本失衡。
pub const ST_EBAL: u32 = SEG_BASE + 0x0b;
/// 账本槽位乱序。
pub const ST_EORDER: u32 = SEG_BASE + 0x0c;
/// 观测值与复算不符。
pub const ST_EMISMATCH: u32 = SEG_BASE + 0x0d;
/// 冻结后重放不一致。
pub const ST_ESEAL: u32 = SEG_BASE + 0x0e;

/// 进程名容量。
pub const NAME_CAP: usize = 16;
/// CPU id 上限（不含）。
pub const CPU_MAX: u8 = 16;
/// 行缓冲容量。
pub const LINE_CAP: usize = 96;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 行键：进程名。
pub const L_NAME: u8 = 1;
/// 行键：状态。
pub const L_STATE: u8 = 2;
/// 行键：Pid。
pub const L_PID: u8 = 3;
/// 行键：PPid。
pub const L_PPID: u8 = 4;
/// 行键：Uid 四元组。
pub const L_UID: u8 = 5;
/// 行键：VmPeak。
pub const L_VMPEAK: u8 = 6;
/// 行键：Threads。
pub const L_THREADS: u8 = 7;
/// 行键：SigQ。
pub const L_SIGQ: u8 = 8;
/// 行键：Cpus_allowed_list。
pub const L_CPUS: u8 = 9;

/// 操作码：设定 Pid。
pub const OP_PID: u8 = 1;
/// 操作码：设定 PPid。
pub const OP_PPID: u8 = 2;
/// 操作码：设定状态。
pub const OP_STATE: u8 = 3;
/// 操作码：设定线程数。
pub const OP_THREADS: u8 = 4;
/// 操作码：设定内存。
pub const OP_MEM: u8 = 5;
/// 操作码：设定 Uid。
pub const OP_UID: u8 = 6;
/// 操作码：设定 CPU 表。
pub const OP_CPUS: u8 = 7;
/// 操作码：设定 SigQ。
pub const OP_SIGQ: u8 = 8;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 状态字母 → 解释词。
pub fn state_word(c: u8) -> Option<&'static [u8]> {
    match c {
        b'R' => Some(b"running"),
        b'S' => Some(b"sleeping"),
        b'D' => Some(b"disk sleep"),
        b'Z' => Some(b"zombie"),
        b'T' => Some(b"stopped"),
        b't' => Some(b"tracing stop"),
        b'I' => Some(b"idle"),
        _ => None,
    }
}

/// 进程 status 的关键字段。
#[derive(Clone, Copy, Debug)]
pub struct Status {
    name: [u8; NAME_CAP],
    name_len: usize,
    state: u8,
    pid: u32,
    ppid: u32,
    uid: [u32; 4],
    vpeak: u32,
    vsize: u32,
    threads: u32,
    sigq_pending: u32,
    sigq_limit: u32,
    cpus: [u8; CPU_MAX as usize],
    cpu_len: usize,
    pid_set: bool,
    buf: [u8; LINE_CAP],
    buf_len: usize,
}

impl Status {
    /// 初始 status。
    pub const fn new() -> Status {
        Status {
            name: [0u8; NAME_CAP],
            name_len: 0,
            state: b'R',
            pid: 0,
            ppid: 0,
            uid: [0u32; 4],
            vpeak: 0,
            vsize: 0,
            threads: 1,
            sigq_pending: 0,
            sigq_limit: 1,
            cpus: [0u8; CPU_MAX as usize],
            cpu_len: 0,
            pid_set: false,
            buf: [0u8; LINE_CAP],
            buf_len: 0,
        }
    }

    /// 设定进程名。
    pub fn set_name(&mut self, name: &[u8]) -> Result<u32, u32> {
        if name.len() > NAME_CAP {
            return Err(ST_ENAME);
        }
        let mut i = 0usize;
        while i < name.len() {
            if let (Some(src), Some(dst)) = (name.get(i), self.name.get_mut(i)) {
                *dst = *src;
            }
            i += 1;
        }
        self.name_len = name.len();
        Ok(name.len() as u32)
    }

    /// 设定 Pid；零不是合法进程身份。
    pub fn set_pid(&mut self, pid: u32) -> Result<u32, u32> {
        if pid == 0 {
            return Err(ST_EPID);
        }
        self.pid = pid;
        self.pid_set = true;
        Ok(pid)
    }

    /// 设定 PPid。
    pub fn set_ppid(&mut self, ppid: u32) -> Result<u32, u32> {
        self.ppid = ppid;
        Ok(ppid)
    }

    /// 设定状态字母。
    pub fn set_state(&mut self, c: u8) -> Result<u32, u32> {
        if state_word(c).is_none() {
            return Err(ST_ESTATE);
        }
        self.state = c;
        Ok(c as u32)
    }

    /// 设定线程数；至少为 1。
    pub fn set_threads(&mut self, n: u32) -> Result<u32, u32> {
        if n == 0 {
            return Err(ST_ETHREADS);
        }
        self.threads = n;
        Ok(n)
    }

    /// 设定 VmPeak 与 VmSize（单位 kB）；峰值不得小于当前值。
    pub fn set_mem(&mut self, peak: u32, size: u32) -> Result<u32, u32> {
        if peak < size {
            return Err(ST_EMEM);
        }
        self.vpeak = peak;
        self.vsize = size;
        Ok(peak)
    }

    /// 设定 Uid 四元组；返回 fs uid。
    pub fn set_uid(&mut self, real: u32, eff: u32, saved: u32, fs: u32) -> Result<u32, u32> {
        if let Some(slot) = self.uid.get_mut(0) {
            *slot = real;
        }
        if let Some(slot) = self.uid.get_mut(1) {
            *slot = eff;
        }
        if let Some(slot) = self.uid.get_mut(2) {
            *slot = saved;
        }
        if let Some(slot) = self.uid.get_mut(3) {
            *slot = fs;
        }
        Ok(fs)
    }

    /// 设定 SigQ；limit 为零或 pending 超出 limit 都非法。
    pub fn set_sigq(&mut self, pending: u32, limit: u32) -> Result<u32, u32> {
        if limit == 0 || pending > limit {
            return Err(ST_ESIGQ);
        }
        self.sigq_pending = pending;
        self.sigq_limit = limit;
        Ok(limit)
    }

    /// 设定 CPU 允许表。
    ///
    /// 输入是范围表文本，要求升序、无重叠、id 小于 `CPU_MAX`。
    /// 成功返回 CPU 个数。
    pub fn set_cpu_list(&mut self, list: &[u8]) -> Result<u32, u32> {
        if list.is_empty() {
            return Err(ST_ELIST);
        }
        let mut tmp = [0u8; CPU_MAX as usize];
        let mut n = 0usize;
        let mut i = 0usize;
        let mut last: Option<u8> = None;
        while i < list.len() {
            // 解析 lo。
            let mut lo: u32 = 0;
            let mut digits = 0u32;
            while i < list.len() {
                let c = match list.get(i) {
                    Some(v) => *v,
                    None => return Err(ST_ELIST),
                };
                if c.is_ascii_digit() {
                    lo = lo * 10 + (c - b'0') as u32;
                    if lo >= CPU_MAX as u32 {
                        return Err(ST_ELIST);
                    }
                    digits += 1;
                    i += 1;
                } else {
                    break;
                }
            }
            if digits == 0 {
                return Err(ST_ELIST);
            }
            // 可选的高位。
            let mut hi = lo;
            if i < list.len() {
                let dash = match list.get(i) {
                    Some(b) => *b == b'-',
                    None => false,
                };
                if dash {
                    i += 1;
                    let mut h: u32 = 0;
                    let mut hd = 0u32;
                    while i < list.len() {
                        let c = match list.get(i) {
                            Some(v) => *v,
                            None => return Err(ST_ELIST),
                        };
                        if c.is_ascii_digit() {
                            h = h * 10 + (c - b'0') as u32;
                            if h >= CPU_MAX as u32 {
                                return Err(ST_ELIST);
                            }
                            hd += 1;
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    if hd == 0 || h < lo {
                        return Err(ST_ELIST);
                    }
                    hi = h;
                }
            }
            // 逐 id 入表，拒绝回退与重叠。
            let mut v = lo;
            while v <= hi {
                if let Some(prev) = last {
                    if v as u8 <= prev {
                        return Err(ST_ELIST);
                    }
                }
                if let Some(dst) = tmp.get_mut(n) {
                    *dst = v as u8;
                } else {
                    return Err(ST_ELIST);
                }
                n += 1;
                last = Some(v as u8);
                v += 1;
            }
            if i < list.len() {
                match list.get(i) {
                    Some(b',') => {
                        i += 1;
                        if i >= list.len() {
                            return Err(ST_ELIST);
                        }
                    }
                    _ => return Err(ST_ELIST),
                }
            }
        }
        // 全部合法才落盘。
        let mut k = 0usize;
        while k < n {
            if let (Some(src), Some(dst)) = (tmp.get(k), self.cpus.get_mut(k)) {
                *dst = *src;
            }
            k += 1;
        }
        self.cpu_len = n;
        Ok(n as u32)
    }

    /// 当前 Pid。
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// 当前线程数。
    pub fn threads(&self) -> u32 {
        self.threads
    }

    /// 当前状态字母。
    pub fn state(&self) -> u8 {
        self.state
    }

    /// 当前 CPU 个数。
    pub fn cpu_count(&self) -> usize {
        self.cpu_len
    }

    /// 是否已设定 Pid。
    pub fn has_pid(&self) -> bool {
        self.pid_set
    }

    /// 渲染缓冲中第 i 个字节。
    pub fn byte_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.buf_len {
            return Err(ST_ENOBS);
        }
        match self.buf.get(i) {
            Some(b) => Ok(*b as u32),
            None => Err(ST_ENOBS),
        }
    }

    /// 上次渲染的字节数。
    pub fn line_len(&self) -> usize {
        self.buf_len
    }

    /// 向缓冲追加一个字节。
    fn put(&mut self, b: u8) {
        if self.buf_len < LINE_CAP {
            if let Some(dst) = self.buf.get_mut(self.buf_len) {
                *dst = b;
                self.buf_len += 1;
            }
        }
    }

    /// 追加一段字节。
    fn put_bytes(&mut self, s: &[u8]) {
        let mut i = 0usize;
        while i < s.len() {
            if let Some(b) = s.get(i) {
                let c = *b;
                self.put(c);
            }
            i += 1;
        }
    }

    /// 追加十进制无符号数。
    fn put_dec(&mut self, v: u32) {
        let mut tmp = [0u8; 10];
        let mut n = 0usize;
        let mut x = v;
        if x == 0 {
            self.put(b'0');
            return;
        }
        while x > 0 && n < tmp.len() {
            if let Some(dst) = tmp.get_mut(n) {
                *dst = b'0' + (x % 10) as u8;
            }
            n += 1;
            x /= 10;
        }
        while n > 0 {
            n -= 1;
            if let Some(b) = tmp.get(n) {
                let c = *b;
                self.put(c);
            }
        }
    }

    /// 按行键渲染，返回行字节数。
    ///
    /// 渲染前必须设定 Pid，否则 `EWARM`；未知键报 `EBADKEY`。
    pub fn render(&mut self, key: u8) -> Result<usize, u32> {
        if !self.pid_set {
            return Err(ST_EWARM);
        }
        self.buf_len = 0;
        match key {
            L_NAME => {
                self.put_bytes(b"Name:\t");
                let l = self.name_len;
                let mut i = 0usize;
                while i < l {
                    let b = match self.name.get(i) {
                        Some(v) => *v,
                        None => 0,
                    };
                    self.put(b);
                    i += 1;
                }
                self.put(b'\n');
            }
            L_STATE => {
                self.put_bytes(b"State:\t");
                self.put(self.state);
                self.put(b' ');
                self.put(b'(');
                if let Some(w) = state_word(self.state) {
                    self.put_bytes(w);
                }
                self.put(b')');
                self.put(b'\n');
            }
            L_PID => {
                self.put_bytes(b"Pid:\t");
                let v = self.pid;
                self.put_dec(v);
                self.put(b'\n');
            }
            L_PPID => {
                self.put_bytes(b"PPid:\t");
                let v = self.ppid;
                self.put_dec(v);
                self.put(b'\n');
            }
            L_UID => {
                self.put_bytes(b"Uid:\t");
                let mut i = 0usize;
                while i < 4 {
                    if i > 0 {
                        self.put(b'\t');
                    }
                    let v = match self.uid.get(i) {
                        Some(x) => *x,
                        None => 0,
                    };
                    self.put_dec(v);
                    i += 1;
                }
                self.put(b'\n');
            }
            L_VMPEAK => {
                self.put_bytes(b"VmPeak:\t");
                let v = self.vpeak;
                self.put_dec(v);
                self.put_bytes(b" kB\n");
            }
            L_THREADS => {
                self.put_bytes(b"Threads:\t");
                let v = self.threads;
                self.put_dec(v);
                self.put(b'\n');
            }
            L_SIGQ => {
                self.put_bytes(b"SigQ:\t");
                let p = self.sigq_pending;
                let l = self.sigq_limit;
                self.put_dec(p);
                self.put(b'/');
                self.put_dec(l);
                self.put(b'\n');
            }
            L_CPUS => {
                self.put_bytes(b"Cpus_allowed_list:\t");
                let n = self.cpu_len;
                let mut i = 0usize;
                while i < n {
                    let lo = match self.cpus.get(i) {
                        Some(v) => *v,
                        None => 0,
                    };
                    let mut hi = lo;
                    let mut j = i + 1;
                    while j < n {
                        let prev = match self.cpus.get(j - 1) {
                            Some(v) => *v,
                            None => lo,
                        };
                        let cur = match self.cpus.get(j) {
                            Some(v) => *v,
                            None => lo,
                        };
                        if cur == prev + 1 {
                            hi = cur;
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    if i > 0 {
                        self.put(b',');
                    }
                    self.put_dec(lo as u32);
                    if hi > lo {
                        self.put(b'-');
                        self.put_dec(hi as u32);
                    }
                    i = j;
                }
                self.put(b'\n');
            }
            _ => return Err(ST_EBADKEY),
        }
        if self.buf_len >= LINE_CAP {
            return Err(ST_EWARM);
        }
        Ok(self.buf_len)
    }
}

/// 渲染结果是否与期望字节串相等。
fn matches(s: &Status, expect: &[u8]) -> bool {
    if s.line_len() != expect.len() {
        return false;
    }
    let mut i = 0usize;
    while i < expect.len() {
        let want = match expect.get(i) {
            Some(b) => *b as u32,
            None => return false,
        };
        if s.byte_at(i) != Ok(want) {
            return false;
        }
        i += 1;
    }
    true
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记一格；槽位必须严格递增。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(ST_EORDER);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(ST_EBAL);
        }
        const VALS: [u32; 8] = [1234, 1, 82, 3, 4096, 1000, 5, 127];
        const CODES: [u32; 6] = [
            ST_EPID,
            ST_ESTATE,
            ST_ETHREADS,
            ST_ELIST,
            ST_ESIGQ,
            ST_EMEM,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(ST_EBAL),
            };
            if s.slot as usize != i {
                return Err(ST_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(ST_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(ST_EMISMATCH);
                        }
                    }
                    None => return Err(ST_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(ST_EMISMATCH);
                        }
                    }
                    None => return Err(ST_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(ST_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(ST_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(ST_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 十四格账本：身份 → 状态 → 资源 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut s = Status::new();

    let _ = l.register(word_cell(0, OP_PID, s.set_pid(1234)));
    let _ = l.register(word_cell(1, OP_PPID, s.set_ppid(1)));
    let _ = l.register(word_cell(2, OP_STATE, s.set_state(b'R')));
    let _ = l.register(word_cell(3, OP_THREADS, s.set_threads(3)));
    let _ = l.register(word_cell(4, OP_MEM, s.set_mem(4096, 2048)));
    let _ = l.register(word_cell(5, OP_UID, s.set_uid(1000, 1000, 1000, 1000)));
    let _ = l.register(word_cell(6, OP_CPUS, s.set_cpu_list(b"0-3,8")));
    let _ = l.register(word_cell(7, OP_SIGQ, s.set_sigq(0, 127)));

    let _ = l.register(word_cell(8, OP_PID, s.set_pid(0)));
    let _ = l.register(word_cell(9, OP_STATE, s.set_state(b'Q')));
    let _ = l.register(word_cell(10, OP_THREADS, s.set_threads(0)));
    let _ = l.register(word_cell(11, OP_CPUS, s.set_cpu_list(b"3-1")));
    let _ = l.register(word_cell(12, OP_SIGQ, s.set_sigq(200, 127)));
    let _ = l.register(word_cell(13, OP_MEM, s.set_mem(2048, 4096)));

    l
}

/// 构造一份完整的 status 样本。
fn full_status() -> Status {
    let mut s = Status::new();
    let _ = s.set_name(b"vxbox");
    let _ = s.set_pid(1234);
    let _ = s.set_ppid(1);
    let _ = s.set_state(b'R');
    let _ = s.set_threads(3);
    let _ = s.set_mem(4096, 2048);
    let _ = s.set_uid(1000, 1000, 1000, 1000);
    let _ = s.set_cpu_list(b"0-3,8");
    let _ = s.set_sigq(0, 127);
    s
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：身份、状态、资源行的逐字节真值。
    const P_PID: &[u8] = b"Pid:\t1234\n";
    const P_THREADS: &[u8] = b"Threads:\t3\n";
    const P_VMPEAK: &[u8] = b"VmPeak:\t4096 kB\n";
    const P_STATE: &[u8] = b"State:\tR (running)\n";
    let mut s = full_status();
    let pid_line = s.render(L_PID) == Ok(10) && matches(&s, P_PID);
    let thr_line = s.render(L_THREADS) == Ok(11) && matches(&s, P_THREADS);
    let mem_line = s.render(L_VMPEAK) == Ok(16) && matches(&s, P_VMPEAK);
    let state_line = s.render(L_STATE) == Ok(19) && matches(&s, P_STATE);
    let name_line = {
        s.render(L_NAME) == Ok(12)
            && matches(&s, b"Name:\tvxbox\n")
            && s.render(L_PPID) == Ok(8)
            && matches(&s, b"PPid:\t1\n")
    };
    out[1] = pid_line && thr_line && mem_line && state_line && name_line;

    // 二路：Uid 四元组、SigQ、CPU 范围表及其规范化。
    const P_UID: &[u8] = b"Uid:\t1000\t1000\t1000\t1000\n";
    const P_SIGQ: &[u8] = b"SigQ:\t0/127\n";
    const P_CPUS: &[u8] = b"Cpus_allowed_list:\t0-3,8\n";
    let mut u = full_status();
    let uid_line = u.render(L_UID) == Ok(25) && matches(&u, P_UID);
    let sigq_line = u.render(L_SIGQ) == Ok(12) && matches(&u, P_SIGQ);
    let cpus_line = u.render(L_CPUS) == Ok(25) && matches(&u, P_CPUS);
    let canonical = u.set_cpu_list(b"0,1,2,3,8") == Ok(5)
        && u.render(L_CPUS) == Ok(25)
        && matches(&u, P_CPUS)
        && u.set_cpu_list(b"0-0") == Ok(1)
        && u.render(L_CPUS) == Ok(21)
        && matches(&u, b"Cpus_allowed_list:\t0\n");
    out[2] = uid_line && sigq_line && cpus_line && canonical;

    // 三路：闸门次序、失败不改状态、渲染前置条件。
    let mut g = Status::new();
    let warm = g.render(L_PID) == Err(ST_EWARM)
        && g.render(L_CPUS) == Err(ST_EWARM);
    let _ = g.set_pid(7);
    let ok_pid = g.render(L_PID) == Ok(7) && matches(&g, b"Pid:\t7\n");

    let gates = g.set_pid(0) == Err(ST_EPID)
        && g.set_state(b'Q') == Err(ST_ESTATE)
        && g.set_threads(0) == Err(ST_ETHREADS)
        && g.set_cpu_list(b"3-1") == Err(ST_ELIST)
        && g.set_cpu_list(b"0-3,3") == Err(ST_ELIST)
        && g.set_cpu_list(b"16") == Err(ST_ELIST)
        && g.set_cpu_list(b"0-") == Err(ST_ELIST)
        && g.set_cpu_list(b"") == Err(ST_ELIST)
        && g.set_sigq(200, 127) == Err(ST_ESIGQ)
        && g.set_sigq(0, 0) == Err(ST_ESIGQ)
        && g.set_mem(2048, 4096) == Err(ST_EMEM)
        && g.set_mem(2048, 2048) == Ok(2048)
        && g.set_name(b"0123456789abcdefg") == Err(ST_ENAME)
        && g.render(99) == Err(ST_EBADKEY);

    let preserved = g.has_pid()
        && g.pid() == 7
        && g.threads() == 1
        && g.state() == b'R'
        && g.cpu_count() == 0
        && g.render(L_PID) == Ok(7)
        && matches(&g, b"Pid:\t7\n");
    out[3] = warm && ok_pid && gates && preserved;

    out
}

/// 十三项判据。
pub fn run_unx11347_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11347_status_lines");
    let l = build_ledger();

    s.add_dyn(
        "ST1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "ST2-Pid设定",
        l.sample(0).map(|x| x.value) == Some(1234),
        "static",
    );

    s.add(
        "ST3-PPid设定",
        l.sample(1).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "ST4-状态字母",
        l.sample(2).map(|x| x.value) == Some(b'R' as u32),
        "static",
    );

    s.add(
        "ST5-线程数",
        l.sample(3).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "ST6-峰值内存",
        l.sample(4).map(|x| x.value) == Some(4096),
        "static",
    );

    s.add(
        "ST7-Uid四元组尾项",
        l.sample(5).map(|x| x.value) == Some(1000),
        "static",
    );

    s.add(
        "ST8-CPU个数",
        l.sample(6).map(|x| x.value) == Some(5),
        "static",
    );

    s.add(
        "ST9-SigQ上限",
        l.sample(7).map(|x| x.value) == Some(127),
        "static",
    );

    s.add(
        "ST10-零Pid与非法状态",
        l.sample(8).map(|x| x.code) == Some(ST_EPID)
            && l.sample(9).map(|x| x.code) == Some(ST_ESTATE),
        "static",
    );

    s.add(
        "ST11-零线程与倒序区间",
        l.sample(10).map(|x| x.code) == Some(ST_ETHREADS)
            && l.sample(11).map(|x| x.code) == Some(ST_ELIST),
        "static",
    );

    s.add(
        "ST12-SigQ与内存闸门",
        l.sample(12).map(|x| x.code) == Some(ST_ESIGQ)
            && l.sample(13).map(|x| x.code) == Some(ST_EMEM),
        "static",
    );

    s.add(
        "ST13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.root() != 0
                && f.rejects() == 1
                && inj == [true; 4]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11347_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn identity_lines() {
        let mut s = full_status();
        assert_eq!(s.render(L_PID), Ok(10));
        assert!(matches(&s, b"Pid:\t1234\n"));
        assert_eq!(s.render(L_PPID), Ok(8));
        assert!(matches(&s, b"PPid:\t1\n"));
        assert_eq!(s.render(L_NAME), Ok(12));
        assert!(matches(&s, b"Name:\tvxbox\n"));
        assert_eq!(s.render(L_STATE), Ok(19));
        assert!(matches(&s, b"State:\tR (running)\n"));
    }

    #[test]
    fn resource_lines() {
        let mut s = full_status();
        assert_eq!(s.render(L_THREADS), Ok(11));
        assert!(matches(&s, b"Threads:\t3\n"));
        assert_eq!(s.render(L_VMPEAK), Ok(16));
        assert!(matches(&s, b"VmPeak:\t4096 kB\n"));
        assert_eq!(s.render(L_SIGQ), Ok(12));
        assert!(matches(&s, b"SigQ:\t0/127\n"));
        assert_eq!(s.render(L_UID), Ok(25));
        assert!(matches(&s, b"Uid:\t1000\t1000\t1000\t1000\n"));
    }

    #[test]
    fn cpu_list_canonical() {
        let mut s = full_status();
        assert_eq!(s.render(L_CPUS), Ok(25));
        assert!(matches(&s, b"Cpus_allowed_list:\t0-3,8\n"));
        assert_eq!(s.set_cpu_list(b"0,1,2,3,8"), Ok(5));
        assert_eq!(s.render(L_CPUS), Ok(25));
        assert!(matches(&s, b"Cpus_allowed_list:\t0-3,8\n"));
        assert_eq!(s.set_cpu_list(b"0-0"), Ok(1));
        assert_eq!(s.render(L_CPUS), Ok(21));
        assert!(matches(&s, b"Cpus_allowed_list:\t0\n"));
        assert_eq!(s.set_cpu_list(b"0-15"), Ok(16));
        assert_eq!(s.cpu_count(), 16);
    }

    #[test]
    fn render_requires_pid() {
        let mut s = Status::new();
        assert_eq!(s.render(L_PID), Err(ST_EWARM));
        assert_eq!(s.set_pid(7), Ok(7));
        assert_eq!(s.render(L_PID), Ok(7));
        assert!(matches(&s, b"Pid:\t7\n"));
        assert_eq!(s.render(42), Err(ST_EBADKEY));
    }

    #[test]
    fn rejects_bad_lists() {
        let mut s = Status::new();
        let _ = s.set_pid(1);
        assert_eq!(s.set_cpu_list(b""), Err(ST_ELIST));
        assert_eq!(s.set_cpu_list(b"3-1"), Err(ST_ELIST));
        assert_eq!(s.set_cpu_list(b"0-3,3"), Err(ST_ELIST));
        assert_eq!(s.set_cpu_list(b"1,0"), Err(ST_ELIST));
        assert_eq!(s.set_cpu_list(b"16"), Err(ST_ELIST));
        assert_eq!(s.set_cpu_list(b"0-"), Err(ST_ELIST));
        assert_eq!(s.set_cpu_list(b",1"), Err(ST_ELIST));
        assert_eq!(s.set_cpu_list(b"0,"), Err(ST_ELIST));
        assert_eq!(s.cpu_count(), 0);
    }

    #[test]
    fn value_gates_and_atomicity() {
        let mut s = full_status();
        assert_eq!(s.set_pid(0), Err(ST_EPID));
        assert_eq!(s.pid(), 1234);
        assert_eq!(s.set_state(b'Q'), Err(ST_ESTATE));
        assert_eq!(s.state(), b'R');
        assert_eq!(s.set_threads(0), Err(ST_ETHREADS));
        assert_eq!(s.threads(), 3);
        assert_eq!(s.set_mem(1, 2), Err(ST_EMEM));
        assert_eq!(s.set_sigq(128, 127), Err(ST_ESIGQ));
        assert_eq!(s.set_name(b"0123456789abcdefg"), Err(ST_ENAME));
        assert_eq!(s.set_name(b"0123456789abcdef"), Ok(16));
    }

    #[test]
    fn state_words_cover_set() {
        let letters = [b'R', b'S', b'D', b'Z', b'T', b't', b'I'];
        let mut i = 0usize;
        while i < letters.len() {
            let c = match letters.get(i) {
                Some(v) => *v,
                None => b'R',
            };
            assert!(state_word(c).is_some());
            i += 1;
        }
        assert!(state_word(b'Q').is_none());
        assert!(state_word(b'r').is_none());
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn inject_paths_agree() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
