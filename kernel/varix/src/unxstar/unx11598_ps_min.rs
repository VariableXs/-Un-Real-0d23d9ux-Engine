//! UNX-F11593 ps/进程面 applet 简化输出判据
//!
//! 本模块把 busybox `ps` 的最小可用面切成三个可判定的层次：**进程记录**、
//! **有序进程表**、**定宽渲染**。模型不依赖内核真实进程，而是用固定容量的
//! 数组把一条进程记录（pid / ppid / 状态字 / tty / CPU 时间 / comm）装进
//! `Proc`，再用按 pid 严格递增的 `Table` 模拟「读 /proc 后排序」的过程，
//! 最后由 `render` 把整张表铺进调用者给的字节缓冲。
//!
//! # 一、为什么是定宽
//!
//! busybox 的 `ps` 为了在 80 列终端里稳定对齐，列宽不是写死的，而是
//! **先扫一遍全表求每列最大值，再和表头长度取较大者**。本模块把这个
//! 过程显式化：`Widths` 保存六列（PID / PPID / STAT / TTY / TIME / COMM）
//! 的最终宽度，`Widths::total()` 加上五处分隔符得到整行宽度。若整行宽度
//! 超过 `LINE_MAX`，说明这次输出已经不适合简化面，`render` 直接以
//! `PS_EWIDE` 拒绝，而不是悄悄截断——截断会让列错位，比不输出更糟。
//!
//! # 二、对齐规则
//!
//! - 数字列（PID / PPID / TTY / TIME）**右对齐**：右侧永远贴着分隔符，
//!   这样个位、十位、百位的末位在同一列上，肉眼可以竖着比较。
//! - 文本列（STAT / COMM）**左对齐**：状态字只有一个字符，靠左才与
//!   `STAT` 表头的首字母对齐；comm 同理。
//! - 每一行（含表头）都补齐到 `Widths::total()`，行尾统一 `\n`。
//!   因此 `render_len() == (total + 1) * (len + 1)`，这个恒等式本身
//!   就是渲染层的自洽判据之一。
//!
//! # 三、状态字
//!
//! 只接受五个字母：`R`（运行）、`S`（可中断睡眠）、`T`（停止）、
//! `Z`（僵尸）、`I`（空闲）。`state_valid` 用来挡住 `X` 之类的脏值，
//! `state_code` 给出稳定的 1..=5 编号供账本记录——编号一旦发布就不能
//! 随字母顺序漂移，否则历史账本会读错。
//!
//! # 四、时间列
//!
//! 内核用时钟滴答计账，本模型固定 `HZ = 100`（即 1 tick = 10ms），
//! `time_text` 把滴答折成 `分:秒`（秒固定两位，分不补零）。分钟数上限
//! 999，避免 `[u8; 6]` 缓冲越界；超过 999 分钟的值按 999 处理——这是
//! 展示层的饱和，不是时间语义的改变。
//!
//! # 五、父子关系
//!
//! 允许 `ppid == 0` 的只有 pid 1（init/根进程）；其余进程必须挂在一个
//! 已登记的父进程上。**插入时不检查父进程是否存在**，因为读 /proc 的
//! 顺序不可控，父进程可能稍后才读到；`validate` 在整表装配完成后统一
//! 检查悬空 ppid（`PS_EPPID`）。这样插入是局部操作，整表校验是全局
//! 操作，两层职责不混。
//!
//! 闸门次序：pid 域 → 父子关系 → 状态字 → tty 上限 → comm 字节 →
//! 表内重复 → 表容量 → 整表悬空校验 → 列宽预算 → 输出缓冲。
//!
//! # 六、账本
//!
//! `build_ledger` 固定登记 14 格：前 8 格成功（依次验证表长、行宽、
//! 渲染长度、秒数换算、状态编号、tty 文本长度、剩余预算、睡眠进程数），
//! 后 6 格失败（重复 pid、pid 0、非法状态、comm 控制字节、悬空 ppid、
//! 表满）。`audit` 把 8 个成功值与 `VALS`、6 个错误码与 `CODES` 逐格
//! 对齐，同时检查槽序、成功格 code 必须为 0、以及 8/6 的计数。任何一格
//! 漂移都会得到 `PS_EVALUE` 而不是静默通过。

use crate::alloc::string::String;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 段基址与错误码
// ---------------------------------------------------------------------------

/// 本卡在 UNX 错误段的基址。
pub const SEG_BASE: u32 = 0xfab0;

/// pid 非法：为 0 或超过 `PID_MAX`。
pub const PS_EPID: u32 = SEG_BASE + 0x01;
/// 父子关系非法：自指、非 1 号进程却无父、或父进程不存在。
pub const PS_EPPID: u32 = SEG_BASE + 0x02;
/// 状态字非法（不在 R/S/T/Z/I 之内）。
pub const PS_ESTATE: u32 = SEG_BASE + 0x03;
/// comm 非法：空、超长或含不可打印字节。
pub const PS_ECOMM: u32 = SEG_BASE + 0x04;
/// pid 重复登记。
pub const PS_EDUP: u32 = SEG_BASE + 0x05;
/// 进程表已满。
pub const PS_EFULL: u32 = SEG_BASE + 0x06;
/// tty 编号越界。
pub const PS_ETTY: u32 = SEG_BASE + 0x07;
/// 列宽超预算或输出缓冲不足。
pub const PS_EWIDE: u32 = SEG_BASE + 0x08;
/// 账本失衡（格数 / 成功失败计数 / 槽序之外的账本级错误）。
pub const PS_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错序或冻结后登记。
pub const PS_ESLOT: u32 = SEG_BASE + 0x0a;
/// 账本观测值与期望不符。
pub const PS_EVALUE: u32 = SEG_BASE + 0x0b;
/// 重放指纹与冻结根不一致。
pub const PS_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺口：内部构造失败但无法归入上面任何一档。
pub const PS_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 规模常量
// ---------------------------------------------------------------------------

/// 账本格数。
pub const SAMPLES: usize = 14;
/// 进程表容量。
pub const TABLE_CAP: usize = 12;
/// comm 最长字节数（对齐内核 TASK_COMM_LEN - 1）。
pub const COMM_CAP: usize = 15;
/// 允许的最大 pid（内核常见 pid_max 的一半，留出展示余量）。
pub const PID_MAX: u16 = 32_768;
/// 允许的最大 tty 编号。
pub const TTY_MAX: u8 = 63;
/// 时钟频率：每秒 100 个滴答。
pub const HZ: u32 = 100;
/// 整行宽度预算（含分隔符，不含换行）。
pub const LINE_MAX: usize = 48;
/// 渲染输出缓冲建议容量。
pub const OUT_CAP: usize = 512;
/// STAT 列固定宽度，等于表头 "STAT" 的长度。
pub const STAT_W: usize = 4;
/// 六列之间的分隔符个数。
pub const SEP_COUNT: usize = 5;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 插入 / 表长观测。
pub const OP_INSERT: u8 = 1;
/// 列宽观测。
pub const OP_WIDTH: u8 = 2;
/// 渲染长度观测。
pub const OP_RENDER: u8 = 3;
/// 时间换算观测。
pub const OP_TIME: u8 = 4;
/// 状态编号观测。
pub const OP_STATE: u8 = 5;
/// tty 文本观测。
pub const OP_TTY: u8 = 6;
/// 剩余列宽预算观测。
pub const OP_SLACK: u8 = 7;
/// 状态计数观测。
pub const OP_COUNT: u8 = 8;

// ---------------------------------------------------------------------------
// FNV-1a
// ---------------------------------------------------------------------------

/// 64 位 FNV-1a 初始偏移。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// 64 位 FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 吸收一个字节：异或后连乘 8 次。
///
/// 之所以不用「每字节一次乘法」的教科书写法，是因为本仓库的指纹约定
/// 固定为 8 次乘法；一旦改动，历史账本的根全部失效。
pub fn fold_byte(h: u64, b: u8) -> u64 {
    let mut x = h ^ (b as u64);
    let mut i = 0usize;
    while i < 8 {
        x = x.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    x
}

/// 吸收一个 u64（小端逐字节）。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0usize;
    while i < 8 {
        x = fold_byte(x, (n & 0xff) as u8);
        n >>= 8;
        i += 1;
    }
    x
}

/// 吸收一段字节。
pub fn fold_bytes(h: u64, bs: &[u8]) -> u64 {
    let mut x = h;
    let mut i = 0usize;
    while i < bs.len() {
        if let Some(b) = bs.get(i) {
            x = fold_byte(x, *b);
        }
        i += 1;
    }
    x
}

// ---------------------------------------------------------------------------
// 字节小工具
// ---------------------------------------------------------------------------

/// 逐字节相等（不看长度之外的任何内容）。
pub fn eq_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 十进制位数（0 算 1 位）。
pub fn digits(v: u32) -> usize {
    let mut n = 1usize;
    let mut x = v;
    while x >= 10 {
        x /= 10;
        n += 1;
    }
    n
}

/// 10 的 n 次幂（饱和，n 只会在 0..=9 内使用）。
fn pow10(n: usize) -> u32 {
    let mut p = 1u32;
    let mut i = 0usize;
    while i < n {
        p = p.saturating_mul(10);
        i += 1;
    }
    p
}

/// 追加十进制数字到 `String`（不依赖格式化设施）。
fn push_dec(s: &mut String, mut v: u32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    while v > 0 {
        if let Some(dst) = buf.get_mut(n) {
            *dst = b'0' + (v % 10) as u8;
        }
        n += 1;
        v /= 10;
    }
    while n > 0 {
        n -= 1;
        if let Some(c) = buf.get(n) {
            s.push(*c as char);
        }
    }
}

// ---------------------------------------------------------------------------
// 状态字
// ---------------------------------------------------------------------------

/// 运行。
pub const ST_RUN: u8 = b'R';
/// 可中断睡眠。
pub const ST_SLEEP: u8 = b'S';
/// 停止。
pub const ST_STOP: u8 = b'T';
/// 僵尸。
pub const ST_ZOMBIE: u8 = b'Z';
/// 空闲。
pub const ST_IDLE: u8 = b'I';

/// 状态字是否合法。
pub fn state_valid(c: u8) -> bool {
    match c {
        ST_RUN => true,
        ST_SLEEP => true,
        ST_STOP => true,
        ST_ZOMBIE => true,
        ST_IDLE => true,
        _ => false,
    }
}

/// 状态编号：1..=5，非法状态为 0。
///
/// 编号与字母顺序无关，是账本对外承诺的稳定值。
pub fn state_code(c: u8) -> u32 {
    match c {
        ST_RUN => 1,
        ST_SLEEP => 2,
        ST_STOP => 3,
        ST_ZOMBIE => 4,
        ST_IDLE => 5,
        _ => 0,
    }
}

/// 状态中文名（只用于文档与判据展示）。
pub fn state_name(c: u8) -> &'static str {
    match c {
        ST_RUN => "运行",
        ST_SLEEP => "睡眠",
        ST_STOP => "停止",
        ST_ZOMBIE => "僵尸",
        ST_IDLE => "空闲",
        _ => "未知",
    }
}

// ---------------------------------------------------------------------------
// comm
// ---------------------------------------------------------------------------

/// 定容进程名（最长 15 字节）。
///
/// 相等性只看有效前缀：`[u8; COMM_CAP]` 的尾部是历史残留，若用 derive
/// 会拿垃圾字节参与比较，得到「同名前缀却不等」的假象。
#[derive(Clone, Copy, Debug)]
pub struct Comm {
    buf: [u8; COMM_CAP],
    len: u8,
}

impl PartialEq for Comm {
    fn eq(&self, other: &Comm) -> bool {
        eq_bytes(self.as_bytes(), other.as_bytes())
    }
}

impl Eq for Comm {}

impl Comm {
    /// 空白名。
    pub const fn blank() -> Comm {
        Comm { buf: [0u8; COMM_CAP], len: 0 }
    }

    /// 从字节构造，挡住空 / 超长 / 非可打印。
    ///
    /// 可打印范围取 `0x21..=0x7e`：空格会把定宽输出切断，控制字节会
    /// 污染终端，两者都拒绝。
    pub fn new(raw: &[u8]) -> Result<Comm, u32> {
        if raw.is_empty() {
            return Err(PS_ECOMM);
        }
        if raw.len() > COMM_CAP {
            return Err(PS_ECOMM);
        }
        let mut c = Comm::blank();
        let mut i = 0usize;
        while i < raw.len() {
            let b = match raw.get(i) {
                Some(v) => *v,
                None => return Err(PS_ECOMM),
            };
            if b < 0x21 || b > 0x7e {
                return Err(PS_ECOMM);
            }
            if let Some(dst) = c.buf.get_mut(i) {
                *dst = b;
            }
            c.len = c.len.saturating_add(1);
            i += 1;
        }
        Ok(c)
    }

    /// 有效字节。
    pub fn as_bytes(&self) -> &[u8] {
        self.buf.get(..self.len as usize).unwrap_or(&[])
    }

    /// 有效长度。
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否空名（只可能来自 `blank`）。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 转成 `String`。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(b) = self.buf.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }

    /// 与一段字节同名。
    pub fn eq_name(&self, other: &[u8]) -> bool {
        eq_bytes(self.as_bytes(), other)
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        fold_bytes(FNV_OFF, self.as_bytes())
    }
}

// ---------------------------------------------------------------------------
// 进程记录
// ---------------------------------------------------------------------------

/// 一条进程记录。字段全部是定宽标量，可以按值拷贝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Proc {
    /// 进程号。
    pub pid: u16,
    /// 父进程号；0 只允许出现在 pid 1 上。
    pub ppid: u16,
    /// 状态字。
    pub state: u8,
    /// 控制终端编号；0 表示无终端。
    pub tty: u8,
    /// CPU 时间（滴答，HZ=100）。
    pub ticks: u32,
    /// 进程名。
    comm: Comm,
}

impl Proc {
    /// 空白记录（只用于定容数组占位）。
    pub const fn blank() -> Proc {
        Proc { pid: 0, ppid: 0, state: 0, tty: 0, ticks: 0, comm: Comm::blank() }
    }

    /// 构造一条记录，闸门顺序固定：
    /// pid → 父子关系 → 状态 → tty → comm。
    ///
    /// 顺序固定是为了让错误码可判定：同一份脏数据无论从哪条路径进来，
    /// 都得到同一个错误码，账本才有意义。
    pub fn new(
        pid: u16,
        ppid: u16,
        state: u8,
        tty: u8,
        ticks: u32,
        name: &[u8],
    ) -> Result<Proc, u32> {
        if pid == 0 || pid > PID_MAX {
            return Err(PS_EPID);
        }
        if ppid == pid {
            return Err(PS_EPPID);
        }
        if ppid == 0 && pid != 1 {
            return Err(PS_EPPID);
        }
        if !state_valid(state) {
            return Err(PS_ESTATE);
        }
        if tty > TTY_MAX {
            return Err(PS_ETTY);
        }
        let comm = Comm::new(name)?;
        Ok(Proc { pid, ppid, state, tty, ticks, comm })
    }

    /// 进程名（拷贝）。
    pub fn comm(&self) -> Comm {
        self.comm
    }

    /// 进程名字节。
    pub fn comm_bytes(&self) -> &[u8] {
        self.comm.as_bytes()
    }

    /// 进程名的有效长度。
    pub fn comm_len(&self) -> usize {
        self.comm.len()
    }

    /// 一行诊断文本：`pid=7 ppid=1 S sh`。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str("pid=");
        push_dec(&mut s, self.pid as u32);
        s.push_str(" ppid=");
        push_dec(&mut s, self.ppid as u32);
        s.push(' ');
        s.push(self.state as char);
        s.push(' ');
        let c = self.comm.text();
        s.push_str(c.as_str());
        s
    }

    /// 指纹：六元组逐字段吸收。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.pid as u64);
        h = fold(h, self.ppid as u64);
        h = fold(h, self.state as u64);
        h = fold(h, self.tty as u64);
        h = fold(h, self.ticks as u64);
        fold(h, self.comm.seal())
    }
}

// ---------------------------------------------------------------------------
// 时间与 tty 文本
// ---------------------------------------------------------------------------

/// 滴答折算成整秒。
pub fn time_secs(ticks: u32) -> u32 {
    ticks / HZ
}

/// 把滴答折成 `分:秒`，返回 (缓冲, 有效长度)。
///
/// 分钟最多 3 位（999），秒固定 2 位，所以最长 6 字节；超过 999 分钟
/// 按 999 饱和，保证不会写出缓冲。
pub fn time_text(ticks: u32) -> ([u8; 6], usize) {
    let secs = time_secs(ticks);
    let raw_min = secs / 60;
    let mins = if raw_min > 999 { 999 } else { raw_min };
    let rem = secs % 60;
    let mut buf = [0u8; 6];
    let mut pos = 0usize;
    let mut d = digits(mins);
    while d > 0 {
        let p = pow10(d - 1);
        let digit = (mins / p) % 10;
        if let Some(dst) = buf.get_mut(pos) {
            *dst = b'0' + digit as u8;
        }
        pos += 1;
        d -= 1;
    }
    if let Some(dst) = buf.get_mut(pos) {
        *dst = b':';
    }
    pos += 1;
    if let Some(dst) = buf.get_mut(pos) {
        *dst = b'0' + (rem / 10) as u8;
    }
    pos += 1;
    if let Some(dst) = buf.get_mut(pos) {
        *dst = b'0' + (rem % 10) as u8;
    }
    pos += 1;
    (buf, pos)
}

/// tty 文本：0 显示 `?`，其余显示 `ttyN`。
///
/// `TTY_MAX == 63` 保证编号最多两位，`[u8; 5]` 刚好装下 `tty63`。
pub fn tty_text(tty: u8) -> ([u8; 5], usize) {
    let mut buf = [0u8; 5];
    if tty == 0 || tty > TTY_MAX {
        if let Some(dst) = buf.get_mut(0) {
            *dst = b'?';
        }
        return (buf, 1);
    }
    let prefix = b"tty";
    let mut pos = 0usize;
    let mut i = 0usize;
    while i < prefix.len() {
        match (buf.get_mut(pos), prefix.get(i)) {
            (Some(dst), Some(src)) => *dst = *src,
            _ => break,
        }
        pos += 1;
        i += 1;
    }
    let mut d = digits(tty as u32);
    while d > 0 {
        let p = pow10(d - 1);
        let digit = ((tty as u32) / p) % 10;
        if let Some(dst) = buf.get_mut(pos) {
            *dst = b'0' + digit as u8;
        }
        pos += 1;
        d -= 1;
    }
    (buf, pos)
}

// ---------------------------------------------------------------------------
// 列宽
// ---------------------------------------------------------------------------

/// 六列的最终宽度（不含分隔符）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Widths {
    /// PID 列。
    pub pid: usize,
    /// PPID 列。
    pub ppid: usize,
    /// TTY 列。
    pub tty: usize,
    /// TIME 列。
    pub time: usize,
    /// COMM 列。
    pub comm: usize,
}

impl Widths {
    /// 空表时的列宽（全部取表头长度）。
    pub const fn blank() -> Widths {
        Widths { pid: 3, ppid: 4, tty: 3, time: 4, comm: 4 }
    }

    /// 整行宽度：六列 + 五处分隔符。
    pub fn total(&self) -> usize {
        self.pid
            .saturating_add(self.ppid)
            .saturating_add(STAT_W)
            .saturating_add(self.tty)
            .saturating_add(self.time)
            .saturating_add(self.comm)
            .saturating_add(SEP_COUNT)
    }
}

// ---------------------------------------------------------------------------
// 进程表
// ---------------------------------------------------------------------------

/// 按 pid 严格递增的定容进程表。
#[derive(Clone, Copy, Debug)]
pub struct Table {
    rows: [Proc; TABLE_CAP],
    len: usize,
}

impl PartialEq for Table {
    fn eq(&self, other: &Table) -> bool {
        if self.len != other.len {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            match (self.rows.get(i), other.rows.get(i)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

impl Eq for Table {}

/// 在有序行上求第一个 `pid >= key` 的位置。
fn lower_bound_pid(rows: &[Proc; TABLE_CAP], len: usize, pid: u16) -> usize {
    let mut lo = 0usize;
    let mut hi = len.min(TABLE_CAP);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        match rows.get(mid) {
            Some(p) => {
                if p.pid < pid {
                    lo = mid.saturating_add(1);
                } else {
                    hi = mid;
                }
            }
            None => return lo,
        }
    }
    lo
}

impl Table {
    /// 空表。
    pub const fn new() -> Table {
        Table { rows: [Proc::blank(); TABLE_CAP], len: 0 }
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取第 i 条。
    pub fn at(&self, i: usize) -> Option<Proc> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 二分查找 pid 的位置。
    pub fn find(&self, pid: u16) -> Option<usize> {
        let p = lower_bound_pid(&self.rows, self.len, pid);
        if p >= self.len {
            return None;
        }
        match self.rows.get(p) {
            Some(x) => {
                if x.pid == pid {
                    Some(p)
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// 按 pid 取记录。
    pub fn get(&self, pid: u16) -> Option<Proc> {
        match self.find(pid) {
            Some(p) => self.rows.get(p).copied(),
            None => None,
        }
    }

    /// 插入：满表 → 重复 → 二分定位 → 整体后移。
    ///
    /// 返回落点下标，便于调用方验证排序语义。
    pub fn insert(&mut self, p: Proc) -> Result<usize, u32> {
        if self.len >= TABLE_CAP {
            return Err(PS_EFULL);
        }
        if self.find(p.pid).is_some() {
            return Err(PS_EDUP);
        }
        let pos = lower_bound_pid(&self.rows, self.len, p.pid);
        let mut i = self.len;
        while i > pos {
            match (self.rows.get(i - 1).copied(), self.rows.get_mut(i)) {
                (Some(src), Some(dst)) => *dst = src,
                _ => break,
            }
            i -= 1;
        }
        if let Some(dst) = self.rows.get_mut(pos) {
            *dst = p;
        }
        self.len = self.len.saturating_add(1);
        Ok(pos)
    }

    /// 是否按 pid 严格递增。
    pub fn is_sorted(&self) -> bool {
        let mut i = 1usize;
        while i < self.len {
            match (self.rows.get(i - 1), self.rows.get(i)) {
                (Some(a), Some(b)) => {
                    if a.pid >= b.pid {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 整表校验：字段域 + 悬空父进程。
    pub fn validate(&self) -> Result<(), u32> {
        if self.len > TABLE_CAP {
            return Err(PS_EFULL);
        }
        if !self.is_sorted() {
            return Err(PS_EDUP);
        }
        let mut i = 0usize;
        while i < self.len {
            let p = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(PS_ENOBS),
            };
            if p.pid == 0 || p.pid > PID_MAX {
                return Err(PS_EPID);
            }
            if p.ppid == p.pid {
                return Err(PS_EPPID);
            }
            if p.ppid == 0 && p.pid != 1 {
                return Err(PS_EPPID);
            }
            if !state_valid(p.state) {
                return Err(PS_ESTATE);
            }
            if p.tty > TTY_MAX {
                return Err(PS_ETTY);
            }
            if p.comm.is_empty() {
                return Err(PS_ECOMM);
            }
            if p.ppid != 0 && self.find(p.ppid).is_none() {
                return Err(PS_EPPID);
            }
            i += 1;
        }
        Ok(())
    }

    /// 处于某状态的记录数。
    pub fn count_in_state(&self, state: u8) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(p) => {
                    if p.state == state {
                        n += 1;
                    }
                }
                None => {}
            }
            i += 1;
        }
        n
    }

    /// 僵尸数。
    pub fn zombies(&self) -> usize {
        self.count_in_state(ST_ZOMBIE)
    }

    /// 沿 ppid 链上溯，判断 pid 是否是 root 的后代（root 自身不算）。
    fn reaches(&self, pid: u16, root: u16) -> bool {
        let mut cur = pid;
        let mut hops = 0usize;
        while hops < TABLE_CAP {
            match self.get(cur) {
                Some(p) => {
                    if p.ppid == root {
                        return true;
                    }
                    if p.ppid == 0 || p.ppid == p.pid {
                        return false;
                    }
                    cur = p.ppid;
                }
                None => return false,
            }
            hops += 1;
        }
        false
    }

    /// root 的后代总数。
    pub fn descendants(&self, root: u16) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(p) => {
                    if p.pid != root && self.reaches(p.pid, root) {
                        n += 1;
                    }
                }
                None => {}
            }
            i += 1;
        }
        n
    }

    /// 最长的 comm 长度。
    pub fn max_comm(&self) -> usize {
        let mut m = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(p) = self.rows.get(i) {
                if p.comm_len() > m {
                    m = p.comm_len();
                }
            }
            i += 1;
        }
        m
    }

    /// 计算六列宽度：全表最大值与表头长度取较大者。
    pub fn widths(&self) -> Widths {
        let mut pid = Widths::blank().pid;
        let mut ppid = Widths::blank().ppid;
        let mut tty = Widths::blank().tty;
        let mut time = Widths::blank().time;
        let mut comm = Widths::blank().comm;
        let mut i = 0usize;
        while i < self.len {
            if let Some(p) = self.rows.get(i) {
                let d = digits(p.pid as u32);
                if d > pid {
                    pid = d;
                }
                let d = digits(p.ppid as u32);
                if d > ppid {
                    ppid = d;
                }
                let d = tty_text(p.tty).1;
                if d > tty {
                    tty = d;
                }
                let d = time_text(p.ticks).1;
                if d > time {
                    time = d;
                }
                let d = p.comm_len();
                if d > comm {
                    comm = d;
                }
            }
            i += 1;
        }
        Widths { pid, ppid, tty, time, comm }
    }

    /// 整表渲染后的字节数（含表头，每行含换行）。
    ///
    /// 恒等式：`(total + 1) * (len + 1)`。列宽超预算时仍返回该值，
    /// 由 `render` 负责拒绝。
    pub fn render_len(&self) -> usize {
        let w = self.widths();
        w.total()
            .saturating_add(1)
            .saturating_mul(self.len.saturating_add(1))
    }

    /// 整表指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(p) = self.rows.get(i) {
                h = fold(h, p.seal());
            }
            i += 1;
        }
        fold(h, self.len as u64)
    }

    /// 渲染整表：表头 + 每行，全部补齐到整行宽度。
    pub fn render(&self, out: &mut [u8]) -> Result<usize, u32> {
        let w = self.widths();
        if w.total() > LINE_MAX {
            return Err(PS_EWIDE);
        }
        let mut pos = 0usize;
        pos = put_field(out, pos, b"PID", w.pid, false)?;
        pos = put_sep(out, pos)?;
        pos = put_field(out, pos, b"PPID", w.ppid, false)?;
        pos = put_sep(out, pos)?;
        pos = put_field(out, pos, b"STAT", STAT_W, true)?;
        pos = put_sep(out, pos)?;
        pos = put_field(out, pos, b"TTY", w.tty, false)?;
        pos = put_sep(out, pos)?;
        pos = put_field(out, pos, b"TIME", w.time, false)?;
        pos = put_sep(out, pos)?;
        pos = put_field(out, pos, b"COMM", w.comm, true)?;
        pos = put_nl(out, pos)?;
        let mut i = 0usize;
        while i < self.len {
            if let Some(p) = self.rows.get(i) {
                let tb = tty_text(p.tty);
                let tt = time_text(p.ticks);
                let stat = [p.state, b' ', b' ', b' '];
                pos = put_num(out, pos, p.pid as u32, w.pid)?;
                pos = put_sep(out, pos)?;
                pos = put_num(out, pos, p.ppid as u32, w.ppid)?;
                pos = put_sep(out, pos)?;
                pos = put_field(out, pos, &stat, STAT_W, true)?;
                pos = put_sep(out, pos)?;
                pos = put_field(out, pos, tb.0.get(..tb.1).unwrap_or(&[]), w.tty, false)?;
                pos = put_sep(out, pos)?;
                pos = put_field(out, pos, tt.0.get(..tt.1).unwrap_or(&[]), w.time, false)?;
                pos = put_sep(out, pos)?;
                pos = put_field(out, pos, p.comm_bytes(), w.comm, true)?;
                pos = put_nl(out, pos)?;
            }
            i += 1;
        }
        Ok(pos)
    }
}

// ---------------------------------------------------------------------------
// 渲染原语
// ---------------------------------------------------------------------------

/// 写一列：`left` 为真时左对齐（右侧补空格），否则右对齐（左侧补空格）。
///
/// 三种拒绝：字段比列宽还长、目标区间越过缓冲尾部。绝不做截断。
fn put_field(
    out: &mut [u8],
    pos: usize,
    bytes: &[u8],
    width: usize,
    left: bool,
) -> Result<usize, u32> {
    if bytes.len() > width {
        return Err(PS_EWIDE);
    }
    let end = pos.saturating_add(width);
    if end > out.len() {
        return Err(PS_EWIDE);
    }
    let pad = width.saturating_sub(bytes.len());
    let mut i = 0usize;
    while i < width {
        let b = if left {
            if i < bytes.len() {
                match bytes.get(i) {
                    Some(v) => *v,
                    None => b' ',
                }
            } else {
                b' '
            }
        } else if i < pad {
            b' '
        } else {
            match bytes.get(i - pad) {
                Some(v) => *v,
                None => b' ',
            }
        };
        if let Some(dst) = out.get_mut(pos.saturating_add(i)) {
            *dst = b;
        }
        i += 1;
    }
    Ok(end)
}

/// 写一个右对齐的十进制数。
fn put_num(out: &mut [u8], pos: usize, v: u32, width: usize) -> Result<usize, u32> {
    let d = digits(v);
    if d > width {
        return Err(PS_EWIDE);
    }
    let end = pos.saturating_add(width);
    if end > out.len() {
        return Err(PS_EWIDE);
    }
    let pad = width.saturating_sub(d);
    let mut i = 0usize;
    while i < pad {
        if let Some(dst) = out.get_mut(pos.saturating_add(i)) {
            *dst = b' ';
        }
        i += 1;
    }
    let mut n = v;
    let mut k = width;
    while k > pad {
        k -= 1;
        let digit = (n % 10) as u8;
        n /= 10;
        if let Some(dst) = out.get_mut(pos.saturating_add(k)) {
            *dst = b'0' + digit;
        }
    }
    Ok(end)
}

/// 写一处分隔符。
fn put_sep(out: &mut [u8], pos: usize) -> Result<usize, u32> {
    if let Some(dst) = out.get_mut(pos) {
        *dst = b' ';
        Ok(pos.saturating_add(1))
    } else {
        Err(PS_EWIDE)
    }
}

/// 写换行。
fn put_nl(out: &mut [u8], pos: usize) -> Result<usize, u32> {
    if let Some(dst) = out.get_mut(pos) {
        *dst = b'\n';
        Ok(pos.saturating_add(1))
    } else {
        Err(PS_EWIDE)
    }
}

// ---------------------------------------------------------------------------
// 样表
// ---------------------------------------------------------------------------

/// 六行样表：一个 init 家族，覆盖五种状态与两种 tty。
pub fn sample_table() -> Table {
    let rows: [(u16, u16, u8, u8, u32, &[u8]); 6] = [
        (1, 0, ST_SLEEP, 0, 0, b"init"),
        (2, 1, ST_SLEEP, 0, 1_234, b"kthreadd"),
        (7, 1, ST_SLEEP, 25, 5_678, b"sh"),
        (20, 7, ST_RUN, 25, 13_579, b"ps"),
        (21, 7, ST_ZOMBIE, 25, 0, b"sleep"),
        (99, 1, ST_IDLE, 0, 99_999, b"worker"),
    ];
    let mut t = Table::new();
    let mut i = 0usize;
    while i < rows.len() {
        if let Some((pid, ppid, st, tty, ticks, name)) = rows.get(i).copied() {
            if let Ok(p) = Proc::new(pid, ppid, st, tty, ticks, name) {
                let _ = t.insert(p);
            }
        }
        i += 1;
    }
    t
}

/// 恰好装满的表：pid 1..=TABLE_CAP，除 1 号外都挂在 1 号下。
fn full_table() -> Table {
    let mut t = Table::new();
    let mut pid: u16 = 1;
    while pid <= TABLE_CAP as u16 {
        let ppid: u16 = if pid == 1 { 0 } else { 1 };
        if let Ok(p) = Proc::new(pid, ppid, ST_SLEEP, 0, pid as u32 * 100, b"fill") {
            let _ = t.insert(p);
        }
        pid += 1;
    }
    t
}

// ---------------------------------------------------------------------------
// 样本与账本
// ---------------------------------------------------------------------------

/// 一格样本。
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

/// 账本。
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

    /// 登记：槽序必须连续，冻结后拒绝。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PS_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len = self.len.saturating_add(1);
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取格。
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
        self.len.saturating_sub(self.ok_count())
    }

    /// 对账：格数 / 8-6 分布 / 槽序 / 成功值 / 错误码。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(PS_EBAL);
        }
        const VALS: [u32; 8] = [6, 34, 245, 135, 4, 5, 14, 3];
        const CODES: [u32; 6] = [PS_EDUP, PS_EPID, PS_ESTATE, PS_ECOMM, PS_EPPID, PS_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(PS_EBAL),
            };
            if s.slot as usize != i {
                return Err(PS_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(PS_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(PS_EVALUE);
                        }
                    }
                    None => return Err(PS_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(PS_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(PS_EVALUE);
                        }
                    }
                    None => return Err(PS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(PS_EBAL);
        }
        Ok(())
    }

    /// 指纹：全格 + 拒绝计数。
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

    /// 冻结：只允许一次。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PS_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：按同一构造路径重建账本并比对指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(PS_ESEAL)
        }
    }

    /// 冻结根。
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

/// `Result<u32, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 账本组装：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let t = sample_table();

    // 0: 样表六条。
    let _ = l.register(cell(0, OP_INSERT, Ok(t.len() as u32)));

    // 1: 样表整行宽度 34。
    let _ = l.register(cell(1, OP_WIDTH, Ok(t.widths().total() as u32)));

    // 2: 样表渲染总字节 245。
    let _ = l.register(cell(
        2,
        OP_RENDER,
        {
            let mut out = [0u8; OUT_CAP];
            match t.render(&mut out) {
                Ok(n) => Ok(n as u32),
                Err(c) => Err(c),
            }
        },
    ));

    // 3: 13579 滴答 = 135 秒。
    let _ = l.register(cell(3, OP_TIME, Ok(time_secs(13_579))));

    // 4: 僵尸状态的编号 4。
    let _ = l.register(cell(4, OP_STATE, Ok(state_code(ST_ZOMBIE))));

    // 5: tty25 的文本长度 5。
    let _ = l.register(cell(5, OP_TTY, Ok(tty_text(25).1 as u32)));

    // 6: 行宽预算余量 48 - 34 = 14。
    let _ = l.register(cell(6, OP_SLACK, Ok((LINE_MAX - t.widths().total()) as u32)));

    // 7: 睡眠状态三条（init / kthreadd / sh）。
    let _ = l.register(cell(7, OP_COUNT, Ok(t.count_in_state(ST_SLEEP) as u32)));

    // 8: 重复登记 pid 20。
    let _ = l.register(cell(
        8,
        OP_INSERT,
        {
            let mut d = sample_table();
            let r: Result<u32, u32>;
            match Proc::new(20, 7, ST_SLEEP, 25, 0, b"again") {
                Ok(p) => match d.insert(p) {
                    Ok(_) => r = Ok(d.len() as u32),
                    Err(c) => r = Err(c),
                },
                Err(c) => r = Err(c),
            }
            r
        },
    ));

    // 9: pid 0。
    let _ = l.register(cell(
        9,
        OP_INSERT,
        match Proc::new(0, 1, ST_SLEEP, 0, 0, b"zero") {
            Ok(_) => Ok(0),
            Err(c) => Err(c),
        },
    ));

    // 10: 非法状态字 X。
    let _ = l.register(cell(
        10,
        OP_STATE,
        match Proc::new(30, 1, b'X', 0, 0, b"bad") {
            Ok(_) => Ok(0),
            Err(c) => Err(c),
        },
    ));

    // 11: comm 含控制字节 0x07。
    let _ = l.register(cell(
        11,
        OP_INSERT,
        match Proc::new(31, 1, ST_SLEEP, 0, 0, &[0x07u8]) {
            Ok(_) => Ok(0),
            Err(c) => Err(c),
        },
    ));

    // 12: 悬空父进程 42。
    let _ = l.register(cell(
        12,
        OP_INSERT,
        {
            let mut d = sample_table();
            let r: Result<u32, u32>;
            match Proc::new(50, 42, ST_SLEEP, 0, 0, b"orphan") {
                Ok(p) => match d.insert(p) {
                    Ok(_) => match d.validate() {
                        Ok(()) => r = Ok(d.len() as u32),
                        Err(c) => r = Err(c),
                    },
                    Err(c) => r = Err(c),
                },
                Err(c) => r = Err(c),
            }
            r
        },
    ));

    // 13: 满表后再插一条。
    let _ = l.register(cell(
        13,
        OP_INSERT,
        {
            let mut d = full_table();
            let r: Result<u32, u32>;
            match Proc::new((TABLE_CAP + 1) as u16, 1, ST_SLEEP, 0, 0, b"over") {
                Ok(p) => match d.insert(p) {
                    Ok(_) => r = Ok(d.len() as u32),
                    Err(c) => r = Err(c),
                },
                Err(c) => r = Err(c),
            }
            r
        },
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据面
// ---------------------------------------------------------------------------

/// 字段闸：pid / 父子 / 状态 / tty / comm 五道闸。
fn gate_ok() -> bool {
    let a = Proc::new(0, 1, ST_SLEEP, 0, 0, b"x").err() == Some(PS_EPID);
    let b = Proc::new(PID_MAX + 1, 1, ST_SLEEP, 0, 0, b"x").err() == Some(PS_EPID);
    let c = Proc::new(2, 0, ST_SLEEP, 0, 0, b"x").err() == Some(PS_EPPID);
    let d = Proc::new(3, 3, ST_SLEEP, 0, 0, b"x").err() == Some(PS_EPPID);
    let e = Proc::new(4, 1, b'X', 0, 0, b"x").err() == Some(PS_ESTATE);
    let f = Proc::new(5, 1, ST_SLEEP, TTY_MAX + 1, 0, b"x").err() == Some(PS_ETTY);
    let g = Proc::new(6, 1, ST_SLEEP, 0, 0, b"").err() == Some(PS_ECOMM);
    let h = Proc::new(1, 0, ST_SLEEP, 0, 0, b"init").is_ok();
    a && b && c && d && e && f && g && h
}

/// 有序插入：二分定位 + 整体后移。
fn order_ok() -> bool {
    let mut t = Table::new();
    let a = t.is_empty() && t.len() == 0;
    let r0 = match Proc::new(7, 1, ST_SLEEP, 0, 0, b"seven") {
        Ok(p) => t.insert(p),
        Err(c) => Err(c),
    };
    let r1 = match Proc::new(2, 1, ST_SLEEP, 0, 0, b"two") {
        Ok(p) => t.insert(p),
        Err(c) => Err(c),
    };
    let r2 = match Proc::new(9, 1, ST_SLEEP, 0, 0, b"nine") {
        Ok(p) => t.insert(p),
        Err(c) => Err(c),
    };
    let b = r0 == Ok(0) && r1 == Ok(0) && r2 == Ok(2);
    let c = t.is_sorted()
        && t.find(7) == Some(1)
        && t.find(9) == Some(2)
        && t.find(3).is_none();
    let d = match t.at(0) {
        Some(p) => p.pid == 2,
        None => false,
    };
    let e = match t.get(9) {
        Some(p) => p.ppid == 1 && p.state == ST_SLEEP,
        None => false,
    };
    a && b && c && d && e
}

/// 重复与满表。
fn full_ok() -> bool {
    let mut t = sample_table();
    let a = match Proc::new(20, 7, ST_SLEEP, 0, 0, b"again") {
        Ok(p) => t.insert(p) == Err(PS_EDUP),
        Err(_) => false,
    };
    let mut f = full_table();
    let b = f.len() == TABLE_CAP && f.is_sorted() && f.validate().is_ok();
    let c = match Proc::new((TABLE_CAP + 1) as u16, 1, ST_SLEEP, 0, 0, b"over") {
        Ok(p) => f.insert(p) == Err(PS_EFULL),
        Err(_) => false,
    };
    let d = f.len() == TABLE_CAP;
    a && b && c && d
}

/// 父子关系与悬空父进程。
fn kin_ok() -> bool {
    let t = sample_table();
    let a = t.validate().is_ok();
    let mut d = sample_table();
    let mut early = true;
    match Proc::new(50, 42, ST_SLEEP, 0, 0, b"orphan") {
        Ok(p) => {
            if d.insert(p).is_err() {
                early = false;
            }
        }
        Err(_) => early = false,
    }
    let b = early && d.validate() == Err(PS_EPPID);
    let c = match d.get(50) {
        Some(p) => p.ppid == 42,
        None => false,
    };
    let e = t.get(42).is_none();
    a && b && c && e
}

/// 时间列格式。
fn time_ok() -> bool {
    let a = time_secs(13_579) == 135;
    let b = time_secs(0) == 0;
    let t0 = time_text(0);
    let c = t0.1 == 4 && eq_bytes(t0.0.get(..t0.1).unwrap_or(&[]), b"0:00");
    let t1 = time_text(13_579);
    let d = t1.1 == 4 && eq_bytes(t1.0.get(..t1.1).unwrap_or(&[]), b"2:15");
    let t2 = time_text(99_999);
    let e = t2.1 == 5 && eq_bytes(t2.0.get(..t2.1).unwrap_or(&[]), b"16:39");
    let t3 = time_text(100);
    let f = eq_bytes(t3.0.get(..t3.1).unwrap_or(&[]), b"0:01");
    let g = digits(0) == 1 && digits(9) == 1 && digits(10) == 2 && digits(13_579) == 5;
    a && b && c && d && e && f && g
}

/// 列宽与行宽预算。
fn width_ok() -> bool {
    let t = sample_table();
    let w = t.widths();
    let a = w.pid == 3 && w.ppid == 4 && w.tty == 5 && w.time == 5 && w.comm == 8;
    let b = w.total() == 34 && w.total() <= LINE_MAX;
    let c = t.max_comm() == 8;
    let e = Table::new().widths().total() == 27;
    let f = Table::new().widths().comm == 4;
    let g = LINE_MAX - w.total() == 14;
    a && b && c && e && f && g
}

/// 定宽渲染布局。
fn render_ok() -> bool {
    let t = sample_table();
    let mut out = [0u8; OUT_CAP];
    let n = match t.render(&mut out) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = n == 245 && t.render_len() == 245;
    let b = eq_bytes(out.get(..3).unwrap_or(&[]), b"PID");
    let c = out.get(34) == Some(&b'\n');
    let d = eq_bytes(out.get(35..38).unwrap_or(&[]), b"  1");
    let e = eq_bytes(out.get(61..65).unwrap_or(&[]), b"init");
    a && b && c && d && e
}

/// 渲染缓冲边界。
fn buffer_ok() -> bool {
    let t = sample_table();
    let mut tiny = [0u8; 100];
    let a = t.render(&mut tiny) == Err(PS_EWIDE);
    let mut exact = [0u8; 245];
    let b = t.render(&mut exact) == Ok(245);
    let mut short = [0u8; 244];
    let c = t.render(&mut short) == Err(PS_EWIDE);
    let mut empty_out = [0u8; 64];
    let d = Table::new().render(&mut empty_out) == Ok(28);
    let e = Table::new().render_len() == 28;
    a && b && c && d && e
}

/// 状态字与状态统计。
fn stat_ok() -> bool {
    let t = sample_table();
    let a = state_valid(ST_RUN)
        && state_valid(ST_SLEEP)
        && state_valid(ST_STOP)
        && state_valid(ST_ZOMBIE)
        && state_valid(ST_IDLE)
        && !state_valid(b'X');
    let b = state_code(ST_RUN) == 1
        && state_code(ST_SLEEP) == 2
        && state_code(ST_STOP) == 3
        && state_code(ST_ZOMBIE) == 4
        && state_code(ST_IDLE) == 5
        && state_code(b'X') == 0;
    let c = t.count_in_state(ST_SLEEP) == 3
        && t.count_in_state(ST_RUN) == 1
        && t.count_in_state(ST_ZOMBIE) == 1
        && t.count_in_state(ST_STOP) == 0
        && t.count_in_state(ST_IDLE) == 1;
    let d = t.zombies() == 1;
    let e = state_name(ST_ZOMBIE) == "僵尸";
    a && b && c && d && e
}

/// 后代遍历（含环防护）。
fn desc_ok() -> bool {
    let t = sample_table();
    let a = t.descendants(1) == 5;
    let b = t.descendants(7) == 2;
    let c = t.descendants(20) == 0;
    let d = t.descendants(42) == 0;
    let mut cyc = Table::new();
    let mut ok = true;
    let rows: [(u16, u16); 3] = [(1, 0), (2, 1), (3, 2)];
    let mut i = 0usize;
    while i < rows.len() {
        if let Some((pid, ppid)) = rows.get(i).copied() {
            match Proc::new(pid, ppid, ST_SLEEP, 0, 0, b"chain") {
                Ok(p) => {
                    if cyc.insert(p).is_err() {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        i += 1;
    }
    let e = ok && cyc.validate().is_ok() && cyc.descendants(1) == 2;
    a && b && c && d && e
}

/// comm 字节闸与文本。
fn comm_ok() -> bool {
    let a = match Comm::new(b"kthreadd") {
        Ok(c) => c.len() == 8 && c.eq_name(b"kthreadd") && !c.eq_name(b"kthread"),
        Err(_) => false,
    };
    let b = Comm::new(b"a b").err() == Some(PS_ECOMM);
    let c = Comm::new(&[b'k'; COMM_CAP + 1]).err() == Some(PS_ECOMM);
    let d = match Comm::new(b"ps") {
        Ok(c) => c.text().as_str() == "ps" && c.seal() != 0,
        Err(_) => false,
    };
    let e = match Proc::new(7, 1, ST_SLEEP, 25, 5_678, b"sh") {
        Ok(p) => p.text().as_str() == "pid=7 ppid=1 S sh" && p.comm().eq_name(b"sh"),
        Err(_) => false,
    };
    let f = Comm::new(b"").err() == Some(PS_ECOMM);
    a && b && c && d && e && f
}

/// 账本对账 + 槽位抽查。
fn ledger_ok() -> bool {
    let l = build_ledger();
    let a = l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6;
    let b = l.audit().is_ok();
    let c = match l.sample(2) {
        Some(s) => s.ok && s.value == 245 && s.code == 0,
        None => false,
    };
    let d = match l.sample(9) {
        Some(s) => !s.ok && s.code == PS_EPID && s.value == 0,
        None => false,
    };
    let e = l.sample(0).map(|s| s.slot == 0).unwrap_or(false);
    a && b && c && d && e
}

/// 冻结 / 重放 / 指纹自洽。
fn seal_ok() -> bool {
    let a = build_ledger();
    let b = build_ledger();
    let c = a.seal() == b.seal();
    let mut d = a;
    let e = d.freeze().is_ok();
    let f = d.replay() == Ok(d.root());
    let g = d.rejects() == 0;
    let h = Table::new().seal() == Table::new().seal();
    let mut m = sample_table();
    let before = m.seal();
    let inserted = match Proc::new(30, 1, ST_SLEEP, 0, 0, b"new") {
        Ok(p) => m.insert(p).is_ok(),
        Err(_) => false,
    };
    let i = inserted && m.seal() != before;
    c && e && f && g && h && i
}

// ---------------------------------------------------------------------------
// 四注入
// ---------------------------------------------------------------------------

/// 把判据压成四束，供第 13 项一次性把关。
pub fn inject_four() -> [bool; 4] {
    let t = sample_table();
    let b0 = gate_ok() && order_ok();
    let b1 = full_ok() && kin_ok();
    let b2 = time_ok() && width_ok() && render_ok() && buffer_ok();
    let b3 = stat_ok() && desc_ok() && comm_ok() && t.validate().is_ok() && t.is_sorted();
    [b0, b1, b2, b3]
}

// ---------------------------------------------------------------------------
// 判据入口
// ---------------------------------------------------------------------------

/// 运行全部 13 项判据。
pub fn run_unx11598_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11598_ps_min");
    s.add("PS01-进程字段闸", gate_ok(), "static");
    s.add("PS02-有序插入", order_ok(), "static");
    s.add("PS03-重复与满表", full_ok(), "static");
    s.add("PS04-父子与悬空", kin_ok(), "static");
    s.add("PS05-时间列格式", time_ok(), "static");
    s.add("PS06-列宽与预算", width_ok(), "static");
    s.add("PS07-定宽渲染", render_ok(), "static");
    s.add("PS08-渲染容量", buffer_ok(), "static");
    s.add("PS09-状态统计", stat_ok(), "static");
    s.add("PS10-后代遍历", desc_ok(), "static");
    s.add("PS11-comm 字节闸", comm_ok(), "static");
    s.add("PS12-账本对账", ledger_ok() && seal_ok(), "static");
    let four = inject_four();
    s.add(
        "PS13-冻结重放四注入",
        four[0] && four[1] && four[2] && four[3],
        "static",
    );
    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), 14);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        match l.sample(2) {
            Some(s) => assert_eq!(s.value, 245),
            None => panic!("missing sample 2"),
        }
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert!(root != 0);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(PS_EBAL));
        assert_eq!(l.rejects(), 1);
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0]);
        assert!(four[1]);
        assert!(four[2]);
        assert!(four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11598_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn comm_gate() {
        assert!(Comm::new(b"sh").is_ok());
        assert_eq!(Comm::new(b"").err(), Some(PS_ECOMM));
        assert_eq!(Comm::new(b"a b").err(), Some(PS_ECOMM));
        assert_eq!(Comm::new(&[b'z'; COMM_CAP + 1]).err(), Some(PS_ECOMM));
        let c = match Comm::new(b"init") {
            Ok(v) => v,
            Err(_) => Comm::blank(),
        };
        assert_eq!(c.len(), 4);
        assert!(c.eq_name(b"init"));
        assert_eq!(c.text().as_str(), "init");
    }

    #[test]
    fn proc_gate_priority() {
        assert_eq!(
            Proc::new(0, 0, b'X', TTY_MAX + 1, 0, b"").err(),
            Some(PS_EPID)
        );
        assert_eq!(
            Proc::new(2, 0, b'X', TTY_MAX + 1, 0, b"").err(),
            Some(PS_EPPID)
        );
        assert_eq!(
            Proc::new(3, 3, b'X', TTY_MAX + 1, 0, b"").err(),
            Some(PS_EPPID)
        );
        assert_eq!(
            Proc::new(4, 1, b'X', TTY_MAX + 1, 0, b"").err(),
            Some(PS_ESTATE)
        );
        assert_eq!(
            Proc::new(5, 1, ST_SLEEP, TTY_MAX + 1, 0, b"").err(),
            Some(PS_ETTY)
        );
        assert_eq!(
            Proc::new(6, 1, ST_SLEEP, 0, 0, b"").err(),
            Some(PS_ECOMM)
        );
    }

    #[test]
    fn sorted_insert_positions() {
        let mut t = Table::new();
        let p7 = Proc::new(7, 1, ST_SLEEP, 0, 0, b"seven");
        assert_eq!(p7.map(|p| t.insert(p)), Ok(Ok(0)));
        let p2 = Proc::new(2, 1, ST_SLEEP, 0, 0, b"two");
        assert_eq!(p2.map(|p| t.insert(p)), Ok(Ok(0)));
        let p9 = Proc::new(9, 1, ST_SLEEP, 0, 0, b"nine");
        assert_eq!(p9.map(|p| t.insert(p)), Ok(Ok(2)));
        assert!(t.is_sorted());
        assert_eq!(t.find(7), Some(1));
        assert_eq!(t.find(9), Some(2));
        assert_eq!(t.find(3), None);
        assert!(t.get(2).is_some());
    }

    #[test]
    fn duplicate_pid_rejected() {
        let mut t = sample_table();
        let p = Proc::new(20, 7, ST_SLEEP, 0, 0, b"again");
        assert_eq!(p.map(|v| t.insert(v)), Ok(Err(PS_EDUP)));
        assert_eq!(t.len(), 6);
    }

    #[test]
    fn table_full_rejected() {
        let mut t = full_table();
        assert_eq!(t.len(), TABLE_CAP);
        assert!(t.validate().is_ok());
        let p = Proc::new((TABLE_CAP + 1) as u16, 1, ST_SLEEP, 0, 0, b"over");
        assert_eq!(p.map(|v| t.insert(v)), Ok(Err(PS_EFULL)));
        assert_eq!(t.len(), TABLE_CAP);
    }

    #[test]
    fn dangling_parent_detected() {
        let mut t = sample_table();
        assert!(t.validate().is_ok());
        let p = Proc::new(50, 42, ST_SLEEP, 0, 0, b"orphan");
        if let Ok(v) = p {
            assert!(t.insert(v).is_ok());
        }
        assert_eq!(t.validate(), Err(PS_EPPID));
        assert!(t.get(42).is_none());
    }

    #[test]
    fn time_format() {
        let (b0, n0) = time_text(0);
        assert_eq!(n0, 4);
        assert_eq!(b0.get(..n0), Some(&b"0:00"[..]));
        let (b1, n1) = time_text(100);
        assert_eq!(b1.get(..n1), Some(&b"0:01"[..]));
        let (b2, n2) = time_text(13_579);
        assert_eq!(n2, 4);
        assert_eq!(b2.get(..n2), Some(&b"2:15"[..]));
        let (b3, n3) = time_text(99_999);
        assert_eq!(n3, 5);
        assert_eq!(b3.get(..n3), Some(&b"16:39"[..]));
        assert_eq!(time_secs(13_579), 135);
    }

    #[test]
    fn widths_sample() {
        let w = sample_table().widths();
        assert_eq!(w.pid, 3);
        assert_eq!(w.ppid, 4);
        assert_eq!(w.tty, 5);
        assert_eq!(w.time, 5);
        assert_eq!(w.comm, 8);
        assert_eq!(w.total(), 34);
        assert_eq!(Table::new().widths().total(), 27);
        assert_eq!(LINE_MAX - w.total(), 14);
    }

    #[test]
    fn render_layout() {
        let t = sample_table();
        let mut out = [0u8; OUT_CAP];
        let n = match t.render(&mut out) {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert_eq!(n, 245);
        assert_eq!(out.get(0..3), Some(&b"PID"[..]));
        assert_eq!(out.get(34), Some(&b'\n'));
        assert_eq!(out.get(35..38), Some(&b"  1"[..]));
        assert_eq!(out.get(61..65), Some(&b"init"[..]));
    }

    #[test]
    fn tight_buffer_rejected() {
        let t = sample_table();
        let mut tiny = [0u8; 100];
        assert_eq!(t.render(&mut tiny), Err(PS_EWIDE));
        let mut exact = [0u8; 245];
        assert_eq!(t.render(&mut exact), Ok(245));
        let mut short = [0u8; 244];
        assert_eq!(t.render(&mut short), Err(PS_EWIDE));
    }

    #[test]
    fn empty_table_render() {
        let t = Table::new();
        let mut out = [0u8; 64];
        assert_eq!(t.render(&mut out), Ok(28));
        assert_eq!(t.render_len(), 28);
        assert_eq!(out.get(0..3), Some(&b"PID"[..]));
    }

    #[test]
    fn state_counts() {
        let t = sample_table();
        assert_eq!(t.count_in_state(ST_SLEEP), 3);
        assert_eq!(t.count_in_state(ST_RUN), 1);
        assert_eq!(t.count_in_state(ST_STOP), 0);
        assert_eq!(t.count_in_state(ST_IDLE), 1);
        assert_eq!(t.zombies(), 1);
        assert_eq!(state_code(ST_ZOMBIE), 4);
        assert_eq!(state_name(ST_ZOMBIE), "僵尸");
    }

    #[test]
    fn descendant_counts() {
        let t = sample_table();
        assert_eq!(t.descendants(1), 5);
        assert_eq!(t.descendants(7), 2);
        assert_eq!(t.descendants(20), 0);
        assert_eq!(t.descendants(42), 0);
    }

    #[test]
    fn table_seal_sensitive() {
        let a = sample_table();
        let b = sample_table();
        assert_eq!(a.seal(), b.seal());
        let mut m = sample_table();
        let before = m.seal();
        if let Ok(p) = Proc::new(30, 1, ST_SLEEP, 0, 0, b"new") {
            assert!(m.insert(p).is_ok());
        }
        assert!(m.seal() != before);
    }

    #[test]
    fn ledger_slot_order() {
        let l = build_ledger();
        let mut i = 0usize;
        while i < SAMPLES {
            match l.sample(i) {
                Some(s) => {
                    assert_eq!(s.slot as usize, i);
                }
                None => panic!("missing slot"),
            }
            i += 1;
        }
        for i in 0..8 {
            match l.sample(i) {
                Some(s) => {
                    assert!(s.ok);
                    assert_eq!(s.code, 0);
                }
                None => panic!("missing ok slot"),
            }
        }
    }
}

