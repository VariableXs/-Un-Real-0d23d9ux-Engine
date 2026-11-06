//! UNX-F11500 B15 批 ktest 断言集聚合
//!
//! B15 批的判据不是"一条命令一条结论"，而是 **19 条各自独立的判据**
//! （`b15.core.*` 6 条、`b15.fs.*` 5 条、`b15.proc.*` 4 条、`b15.io.*`
//! 4 条）。每条判据自带断言总数、失败数、跳过数，并声明自己是否已经
//! 注册进 ktest 生态面。聚合器要回答的只有一个问题：**这一批能不能一次
//! 命令全跑、并且以 100% 通过率判绿**。
//!
//! 为什么不能只看"有没有失败"：19 条判据里只要有一条没注册，它根本没
//! 被执行，`failed=0` 是**缺席**而不是**通过**；只要有一条断言数为零，
//! ktest 收到的是空证据，`failed=0` 是**空集**而不是**全过**；只要有一
//! 条整条被跳过，`failed=0` 是**没跑**而不是**跑过**。这三种"零失败"
//! 的语义完全不同，混成一个布尔就会把假绿送进生态面。
//!
//! 因此判绿需要同时满足四个条件：
//!
//! 1. 注册齐：19 条判据全部 `wired`，一条不缺；
//! 2. 计数齐：聚合器收到 19 条，且每条断言数大于零；
//! 3. 无失败：`failed == 0`；
//! 4. 无跳过：`skipped == 0`，因此通过率恰为 1000‰。
//!
//! 通过率本身不是判据，而是**证据强度**的度量：
//! `ppm = (通过断言 * 1000 + 断言总数 / 2) / 断言总数`。四舍五入到千分位，
//! 好让 328/329 与 329/329 在报表上可分（997 对 1000）。判绿只看
//! `ppm == 1000` 且 `failed == 0` 且 `skipped == 0` 且 `cases == 19`，
//! 四个条件缺一不可；ppm 单独到 1000 并不够，因为"零断言"的批次
//! 也能算出 1000/0 这种无意义比值，所以计数闸门必须排在速率闸门之前。
//!
//! 闸门次序（`sweep` 的判决路径，次序不可交换）：
//!
//! 1. **注册**：逐条扫 `wired`。未注册即停，报 `registry/unwired`，
//!    不再看计数——先证明"这 19 条真的在生态面里"，再谈跑得怎么样。
//! 2. **计数**：`cases != 19` 报 `count/short`；任一条 `asserts == 0`
//!    报 `count/empty`。计数闸门在运行闸门之前，因为空断言的"零失败"
//!    是空集真，不是通过。
//! 3. **运行**：`failed > 0` 报 `run/failing`，退出码 1。失败是最硬的
//!    否定，跳过再多也不能把失败洗掉。
//! 4. **速率**：`skipped > 0` 时，若通过断言为零报 `rate/blocked`
//!    （整批没跑），否则报 `rate/skipped`（跑了一部分）。两者退出码都是
//!    1，但注解不同：前者要给"补跑"，后者要给"补断言"。
//! 5. **就绪**：四条全过，报 `ready/green`，退出码 0，ppm 1000。
//!
//! 退出码是阶段的投影，不是第二套判据：
//! 就绪 0、运行失败 1、速率不足 1、注册/计数 2（环境或接线问题）。
//! 把 setup 类错误压成 0 或 1 都会让 CI 误判——"没接好"和"跑挂了"
//! 是两种修法。
//!
//! 语料 14 槽：8 槽申报正确（绿 1、失败 2、跳过 1、整批跳 1、未注册 1、
//! 短批 1、空断言 1），6 槽各挂一处错误申报。申报维度是
//! 阶段 → 退出码 → 条数 → 失败数 → 通过率 → 注解，判序
//! 状态 → 阶段 → 退出码 → 条数 → 失败数 → 通过率 → 注解。
//! 账本 14 格 8 成 6 败，冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本在
//! `audit` 逐格对账。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x6cf10;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 申报正确的槽数。
pub const ACCEPT: usize = 8;

/// 申报错误的槽数。
pub const REJECT: usize = 6;

/// 档位数。
pub const TIER_COUNT: usize = 4;

/// 本批判据条数（B15 批的生态面）。
pub const JUDGES_N: usize = 19;

/// 套件数（core/fs/proc/io）。
pub const SUITES: usize = 4;

/// 断言数直方图桶数。
pub const BUCKETS: usize = 4;

/// 直方图第一桶上界（不含）。
pub const BUCKET_1: u32 = 8;

/// 直方图第二桶上界（不含）。
pub const BUCKET_2: u32 = 16;

/// 直方图第三桶上界（不含）。
pub const BUCKET_3: u32 = 24;

// ---------------------------------------------------------------------------
// 退出码（setup / run 两族）
// ---------------------------------------------------------------------------

/// 就绪：整批绿。
pub const EXIT_PASS: u32 = 0;

/// 运行/速率失败：判据跑了但没全过。
pub const EXIT_FAIL: u32 = 1;

/// 环境/接线失败：根本没跑起来。
pub const EXIT_SETUP: u32 = 2;

// ---------------------------------------------------------------------------
// 错误码（13 个）
// ---------------------------------------------------------------------------

/// 判果互报（绿/非绿矛盾）。
pub const AG_ESTATE: u32 = SEG_BASE + 0x01;
/// 阶段申报不符。
pub const AG_ESTAGE: u32 = SEG_BASE + 0x02;
/// 退出码申报不符。
pub const AG_ECODE: u32 = SEG_BASE + 0x03;
/// 判据条数申报不符。
pub const AG_ECASES: u32 = SEG_BASE + 0x04;
/// 失败断言数申报不符。
pub const AG_EFAILED: u32 = SEG_BASE + 0x05;
/// 通过率申报不符。
pub const AG_EPPM: u32 = SEG_BASE + 0x06;
/// 注解申报不符。
pub const AG_ENOTE: u32 = SEG_BASE + 0x07;
/// 槽位非法。
pub const AG_ESLOT: u32 = SEG_BASE + 0x08;
/// 指纹不符。
pub const AG_ESEAL: u32 = SEG_BASE + 0x09;
/// 无观测。
pub const AG_ENOBS: u32 = SEG_BASE + 0x0a;
/// 矩阵不平。
pub const AG_EMATRIX: u32 = SEG_BASE + 0x0b;
/// 阶段次序倒置。
pub const AG_EORDER: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const AG_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 登记判据。
pub const OP_REG: u8 = 0x01;
/// 整批扫描。
pub const OP_SWEEP: u8 = 0x02;
/// 通过率。
pub const OP_RATE: u8 = 0x03;
/// 阶段闸门。
pub const OP_GATE: u8 = 0x04;
/// 语料总账。
pub const OP_CORPUS: u8 = 0x05;
/// 套件账。
pub const OP_SUITE: u8 = 0x06;

// ---------------------------------------------------------------------------
// 哈希
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut v = h ^ (b as u64);
    let mut k = 0usize;
    while k < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    v
}

/// 折入一个字（大端序逐字节）。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut v = h;
    let mut shift = 56u32;
    loop {
        v = fold_byte(v, (((w >> shift) & 0xff) as u32) as u8);
        if shift == 0 {
            break;
        }
        shift -= 8;
    }
    v
}

/// 折入一串字节（判据 id、套件名、语义标签都走这里）。
fn fold_str(h: u64, s: &str) -> u64 {
    let mut v = h;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            v = fold_byte(v, *b);
        }
        i += 1;
    }
    fold_byte(v, 0x1f)
}

// ---------------------------------------------------------------------------
// 文本
// ---------------------------------------------------------------------------

/// 十进制文本。
fn dec_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + ((n % 10) as u8);
        n /= 10;
    }
    let mut s = String::new();
    while i < 10 {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

/// 定宽小写十六进制文本。
fn hex_text(v: u32, digits: usize) -> String {
    let mut s = String::new();
    let mut n = digits;
    while n > 0 {
        n -= 1;
        let shift = (n as u32) * 4;
        let nib = ((v >> shift) & 0xf) as u8;
        let c = if nib < 10 { b'0' + nib } else { b'a' + (nib - 10) };
        s.push(c as char);
    }
    s
}

/// u32 文本。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// 千分位文本（附加 `‰` 的 ASCII 写法 `ppm` 由调用方拼接）。
pub fn ppm_text(v: u32) -> String {
    dec_text(v)
}

// ---------------------------------------------------------------------------
// 判据记录
// ---------------------------------------------------------------------------

/// 一条 B15 判据的 ktest 记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Judge {
    /// 生态面 id（全局唯一，点分）。
    pub id: &'static str,
    /// 套件下标（0 core / 1 fs / 2 proc / 3 io）。
    pub suite: u8,
    /// 断言总数。
    pub asserts: u16,
    /// 失败断言数。
    pub failed: u16,
    /// 跳过断言数。
    pub skipped: u16,
    /// 是否已注册进 ktest 生态面。
    pub wired: bool,
}

impl Judge {
    /// 结果为零失败的"空"吗（断言数为零）。
    pub fn empty(&self) -> bool {
        self.asserts == 0
    }

    /// 通过断言数（饱和减）。
    pub fn passed(&self) -> u32 {
        let a = self.asserts as u32;
        let f = self.failed as u32;
        let s = self.skipped as u32;
        a.saturating_sub(f).saturating_sub(s)
    }

    /// 单条通过率（千分位）。
    pub fn ppm(&self) -> u32 {
        if self.asserts == 0 {
            return 0;
        }
        let total = self.asserts as u32;
        (self.passed() * 1000 + total / 2) / total
    }

    /// 是否单条全绿。
    pub fn green(&self) -> bool {
        self.wired && self.asserts > 0 && self.failed == 0 && self.skipped == 0
    }

    /// 记录自洽：失败与跳过之和不超断言总数。
    pub fn consistent(&self) -> bool {
        let a = self.asserts as u32;
        let f = self.failed as u32;
        let s = self.skipped as u32;
        f.saturating_add(s) <= a
    }

    /// 直方图桶号。
    pub fn bucket(&self) -> usize {
        let a = self.asserts as u32;
        if a < BUCKET_1 {
            return 0;
        }
        if a < BUCKET_2 {
            return 1;
        }
        if a < BUCKET_3 {
            return 2;
        }
        3
    }
}

/// 套件名。
pub fn suite_name(i: usize) -> &'static str {
    match i {
        0 => "core",
        1 => "fs",
        2 => "proc",
        3 => "io",
        _ => "?",
    }
}

/// 套件名合法。
pub fn suite_valid(i: usize) -> bool {
    i < SUITES
}

/// B15 批 19 条判据表。断言数刻意不齐（7..31），好让通过率不是
/// `1/19` 的整数倍，四舍五入的边界才有被测到的机会。
pub const JUDGES: [Judge; JUDGES_N] = [
    Judge { id: "b15.core.entry", suite: 0, asserts: 19, failed: 0, skipped: 0, wired: true },
    Judge { id: "b15.core.argc", suite: 0, asserts: 17, failed: 0, skipped: 0, wired: true },
    Judge { id: "b15.core.exit", suite: 0, asserts: 23, failed: 0, skipped: 0, wired: true },
    Judge { id: "b15.core.signal", suite: 0, asserts: 11, failed: 0, skipped: 0, wired: true },
    Judge { id: "b15.core.fork", suite: 0, asserts: 13, failed: 0, skipped: 0, wired: true },
    Judge { id: "b15.core.exec", suite: 0, asserts: 29, failed: 0, skipped: 0, wired: true },
    Judge { id: "b15.fs.open", suite: 1, asserts: 7, failed: 0, skipped: 0, wired: true },
    Judge { id: "b15.fs.read", suite: 1, asserts: 19, failed: 0, skipped: 0, wired: true },
    Judge { id: "b15.fs.write", suite: 1, asserts: 31, failed: 0, skipped: 0, wired: true },
    Judge { id: "b15.fs.rename", suite: 1, asserts: 17, failed: 0, skipped: 0, wired: true },
    Judge { id: "b15.fs.unlink", suite: 1, asserts: 13, failed: 0, skipped: 0, wired: true },
    Judge { id: "b15.proc.spawn", suite: 2, asserts: 23, failed: 0, skipped: 0, wired: true },
    Judge { id: "b15.proc.wait", suite: 2, asserts: 11, failed: 0, skipped: 0, wired: true },
    Judge { id: "b15.proc.stat", suite: 2, asserts: 17, failed: 0, skipped: 0, wired: true },
    Judge { id: "b15.proc.nice", suite: 2, asserts: 19, failed: 0, skipped: 0, wired: true },
    Judge { id: "b15.io.pipe", suite: 3, asserts: 13, failed: 0, skipped: 0, wired: true },
    Judge { id: "b15.io.dup", suite: 3, asserts: 29, failed: 0, skipped: 0, wired: true },
    Judge { id: "b15.io.poll", suite: 3, asserts: 7, failed: 0, skipped: 0, wired: true },
    Judge { id: "b15.io.readv", suite: 3, asserts: 11, failed: 0, skipped: 0, wired: true },
];

/// 一批判据（定容 19 槽，`n` 允许短批）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Batch {
    /// 判据表。
    pub js: [Judge; JUDGES_N],
    /// 有效条数（0..=19）。
    pub n: usize,
}

impl Batch {
    /// 空批。
    pub const fn none() -> Batch {
        Batch { js: [Judge { id: "", suite: 0, asserts: 0, failed: 0, skipped: 0, wired: false }; JUDGES_N], n: 0 }
    }

    /// 取第 i 条。
    pub fn at(&self, i: usize) -> Option<Judge> {
        if i >= self.n {
            return None;
        }
        self.js.get(i).copied()
    }

    /// 断言总数。
    pub fn asserts(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(j) = self.js.get(i) {
                sum = sum.saturating_add(j.asserts as u32);
            }
            i += 1;
        }
        sum
    }

    /// 失败总数。
    pub fn failed(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(j) = self.js.get(i) {
                sum = sum.saturating_add(j.failed as u32);
            }
            i += 1;
        }
        sum
    }

    /// 跳过总数。
    pub fn skipped(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(j) = self.js.get(i) {
                sum = sum.saturating_add(j.skipped as u32);
            }
            i += 1;
        }
        sum
    }

    /// 未注册条数。
    pub fn unwired(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(j) = self.js.get(i) {
                if !j.wired {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 空断言条数。
    pub fn zeros(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(j) = self.js.get(i) {
                if j.empty() {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 通过断言总数。
    pub fn passed(&self) -> u32 {
        self.asserts().saturating_sub(self.failed()).saturating_sub(self.skipped())
    }

    /// 整批通过率（千分位）。
    pub fn ppm(&self) -> u32 {
        let total = self.asserts();
        if total == 0 {
            return 0;
        }
        (self.passed() * 1000 + total / 2) / total
    }

    /// 覆盖校验。
    pub fn cover(&self) -> Result<(), u32> {
        if self.n <= JUDGES_N {
            Ok(())
        } else {
            Err(AG_ENOBS)
        }
    }

    /// 指纹（id、套件、四计数、接线标志）。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            if let Some(j) = self.js.get(i) {
                h = fold_str(h, j.id);
                h = fold_word(h, j.suite as u64);
                h = fold_word(h, j.asserts as u64);
                h = fold_word(h, j.failed as u64);
                h = fold_word(h, j.skipped as u64);
                h = fold_word(h, if j.wired { 1 } else { 0 });
            }
            i += 1;
        }
        fold_word(h, self.n as u64)
    }
}

// ---------------------------------------------------------------------------
// 阶段与注解
// ---------------------------------------------------------------------------

/// 判决阶段。次序即闸门次序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 注册闸门。
    Registry,
    /// 计数闸门。
    Count,
    /// 运行闸门。
    Run,
    /// 速率闸门。
    Rate,
    /// 就绪。
    Ready,
}

impl Stage {
    /// 次序。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Registry => 0,
            Stage::Count => 1,
            Stage::Run => 2,
            Stage::Rate => 3,
            Stage::Ready => 4,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Registry => "registry",
            Stage::Count => "count",
            Stage::Run => "run",
            Stage::Rate => "rate",
            Stage::Ready => "ready",
        }
    }

    /// 是否为 setup 类失败（退出码 2）。
    pub fn setup(&self) -> bool {
        match self {
            Stage::Registry => true,
            Stage::Count => true,
            _ => false,
        }
    }
}

/// 本次聚合的注解。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 19 条全注册、零失败、零跳过。
    Green,
    /// 有失败断言。
    Failing,
    /// 有跳过但跑过一部分。
    Skipped,
    /// 整批断言全被跳过（零证据）。
    Blocked,
    /// 有判据未注册。
    Unwired,
    /// 条数不足 19。
    Short,
    /// 有判据断言数为零。
    Empty,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::Green => "green",
            Note::Failing => "failing",
            Note::Skipped => "skipped",
            Note::Blocked => "blocked",
            Note::Unwired => "unwired",
            Note::Short => "short",
            Note::Empty => "empty",
        }
    }

    /// 是否判绿。
    pub fn green(&self) -> bool {
        match self {
            Note::Green => true,
            _ => false,
        }
    }
}

/// 聚合结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 走到（或死在）的阶段。
    pub stage: Stage,
    /// 退出码。
    pub code: u32,
    /// 判据条数。
    pub cases: u32,
    /// 断言总数。
    pub asserts: u32,
    /// 失败总数。
    pub failed: u32,
    /// 跳过总数。
    pub skipped: u32,
    /// 通过率（千分位；未运行时为 0）。
    pub ppm: u32,
    /// 注解。
    pub note: Note,
}

impl Outcome {
    /// 是否判绿。
    pub fn ok(&self) -> bool {
        self.stage == Stage::Ready && self.code == EXIT_PASS && self.note.green() && self.ppm == 1000
    }

    /// 失败。
    pub fn failed(&self) -> bool {
        !self.ok()
    }

    /// 是否是 setup 类（环境/接线）。
    pub fn setup(&self) -> bool {
        self.stage.setup()
    }

    /// 断言总数文本。
    pub fn asserts_text(&self) -> String {
        dec_text(self.asserts)
    }
}

/// 整批扫描。判序即闸门次序：注册 → 计数 → 运行 → 速率 → 就绪。
pub fn sweep(b: &Batch) -> Outcome {
    // 闸门 1：注册。未注册的判据根本不在生态面里，先停。
    let mut i = 0usize;
    while i < b.n {
        match b.js.get(i) {
            Some(j) => {
                if !j.wired {
                    return Outcome {
                        stage: Stage::Registry,
                        code: EXIT_SETUP,
                        cases: b.n as u32,
                        asserts: b.asserts(),
                        failed: 0,
                        skipped: 0,
                        ppm: 0,
                        note: Note::Unwired,
                    };
                }
            }
            None => {
                return Outcome {
                    stage: Stage::Registry,
                    code: EXIT_SETUP,
                    cases: b.n as u32,
                    asserts: b.asserts(),
                    failed: 0,
                    skipped: 0,
                    ppm: 0,
                    note: Note::Unwired,
                };
            }
        }
        i += 1;
    }

    // 闸门 2：计数。短批与空断言都停在这里。
    if b.n != JUDGES_N {
        return Outcome {
            stage: Stage::Count,
            code: EXIT_SETUP,
            cases: b.n as u32,
            asserts: b.asserts(),
            failed: 0,
            skipped: 0,
            ppm: 0,
            note: Note::Short,
        };
    }
    if b.zeros() > 0 {
        return Outcome {
            stage: Stage::Count,
            code: EXIT_SETUP,
            cases: b.n as u32,
            asserts: b.asserts(),
            failed: 0,
            skipped: 0,
            ppm: 0,
            note: Note::Empty,
        };
    }

    // 闸门 3：运行。失败是最硬的否定。
    let failed = b.failed();
    if failed > 0 {
        return Outcome {
            stage: Stage::Run,
            code: EXIT_FAIL,
            cases: b.n as u32,
            asserts: b.asserts(),
            failed,
            skipped: b.skipped(),
            ppm: b.ppm(),
            note: Note::Failing,
        };
    }

    // 闸门 4：速率。跳过即证据缺口；全跳与部分跳要分开报。
    let skipped = b.skipped();
    if skipped > 0 {
        let note = if b.passed() == 0 { Note::Blocked } else { Note::Skipped };
        return Outcome {
            stage: Stage::Rate,
            code: EXIT_FAIL,
            cases: b.n as u32,
            asserts: b.asserts(),
            failed: 0,
            skipped,
            ppm: b.ppm(),
            note,
        };
    }

    // 闸门 5：就绪。到这里 cases=19、零失败、零跳过，ppm 必为 1000。
    Outcome {
        stage: Stage::Ready,
        code: EXIT_PASS,
        cases: b.n as u32,
        asserts: b.asserts(),
        failed: 0,
        skipped: 0,
        ppm: b.ppm(),
        note: Note::Green,
    }
}

// ---------------------------------------------------------------------------
// 语料形状
// ---------------------------------------------------------------------------

/// 语料批的构造种类（每种是对 `JUDGES` 的一次真实变形）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 19 条全绿。
    Base,
    /// 一条失败（b15.core.exec）。
    OneRed,
    /// 一条跳过（b15.proc.spawn）。
    OneSkip,
    /// 全部断言跳过。
    AllSkip,
    /// 一条未注册（b15.proc.nice）。
    Unwired,
    /// 只有 18 条。
    Short,
    /// 一条断言数为零（b15.core.exit）。
    Zero,
    /// 四条失败（core.signal/fs.write/proc.wait/io.dup）。
    ManyRed,
}

impl Kind {
    /// 文本标签。
    pub fn name(&self) -> &'static str {
        match self {
            Kind::Base => "base",
            Kind::OneRed => "one-red",
            Kind::OneSkip => "one-skip",
            Kind::AllSkip => "all-skip",
            Kind::Unwired => "unwired",
            Kind::Short => "short",
            Kind::Zero => "zero",
            Kind::ManyRed => "many-red",
        }
    }
}

/// 把 `JUDGES` 抄成可变批。
fn base_batch() -> Batch {
    Batch { js: JUDGES, n: JUDGES_N }
}

/// 置某条失败数（失败数不会超过该条断言数）。
fn mark_failed(b: &mut Batch, idx: usize, n: u16) -> bool {
    if idx >= b.n {
        return false;
    }
    match b.js.get_mut(idx) {
        Some(j) => {
            if n > j.asserts {
                return false;
            }
            j.failed = n;
            true
        }
        None => false,
    }
}

/// 置某条跳过数。
fn mark_skipped(b: &mut Batch, idx: usize, n: u16) -> bool {
    if idx >= b.n {
        return false;
    }
    match b.js.get_mut(idx) {
        Some(j) => {
            if n > j.asserts {
                return false;
            }
            j.skipped = n;
            true
        }
        None => false,
    }
}

/// 按种类构造语料批。
pub fn batch_for(k: Kind) -> Batch {
    let mut b = base_batch();
    match k {
        Kind::Base => {}
        Kind::OneRed => {
            let _ = mark_failed(&mut b, 5, 1);
        }
        Kind::OneSkip => {
            let _ = mark_skipped(&mut b, 11, 1);
        }
        Kind::AllSkip => {
            let mut i = 0usize;
            while i < b.n {
                if let Some(j) = b.js.get(i).copied() {
                    if let Some(dst) = b.js.get_mut(i) {
                        dst.skipped = j.asserts;
                    }
                }
                i += 1;
            }
        }
        Kind::Unwired => {
            if let Some(j) = b.js.get_mut(14) {
                j.wired = false;
            }
        }
        Kind::Short => {
            b.n = JUDGES_N - 1;
        }
        Kind::Zero => {
            if let Some(j) = b.js.get_mut(2) {
                j.asserts = 0;
                j.failed = 0;
                j.skipped = 0;
            }
        }
        Kind::ManyRed => {
            let _ = mark_failed(&mut b, 3, 1);
            let _ = mark_failed(&mut b, 8, 1);
            let _ = mark_failed(&mut b, 12, 1);
            let _ = mark_failed(&mut b, 16, 1);
        }
    }
    b
}

// ---------------------------------------------------------------------------
// 语料槽
// ---------------------------------------------------------------------------

/// 一个语料形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// 批构造种类。
    pub kind: Kind,
}

impl Shape {
    /// 空白形状。
    pub const fn blank() -> Shape {
        Shape { tag: "", kind: Kind::Base }
    }

    /// 实际批次。
    pub fn batch(&self) -> Batch {
        batch_for(self.kind)
    }

    /// 实际结果。
    pub fn actual(&self) -> Outcome {
        sweep(&self.batch())
    }
}

/// 一条申报（只申报对外可见的六个数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 申报阶段。
    pub stage: Stage,
    /// 申报退出码。
    pub code: u32,
    /// 申报判据条数。
    pub cases: u32,
    /// 申报失败总数。
    pub failed: u32,
    /// 申报通过率。
    pub ppm: u32,
    /// 申报注解。
    pub note: Note,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim { stage: Stage::Registry, code: 0, cases: 0, failed: 0, ppm: 0, note: Note::Green }
    }

    /// 是否申报"绿"。
    pub fn success(&self) -> bool {
        self.note.green() && self.code == EXIT_PASS
    }
}

/// 一个语料槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 形状。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
}

impl Case {
    /// 空白槽。
    pub const fn blank() -> Case {
        Case { shape: Shape::blank(), claim: Claim::blank() }
    }

    /// 标签。
    pub fn tag(&self) -> &'static str {
        self.shape.tag
    }

    /// 实际结果。
    pub fn actual(&self) -> Outcome {
        self.shape.actual()
    }

    /// 对判。
    pub fn judge(&self) -> Fault {
        judge(self)
    }

    /// 是否一致。
    pub fn ok(&self) -> bool {
        judge(self) == Fault::Ok
    }
}

// ---------------------------------------------------------------------------
// 缺陷
// ---------------------------------------------------------------------------

/// 申报与实际比对得到的缺陷。非 Ok 恰 13 个，与 13 个错误码一一对应。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 一致。
    Ok,
    /// 判果互报。
    Estate,
    /// 阶段错。
    Stage,
    /// 退出码错。
    Code,
    /// 条数错。
    Cases,
    /// 失败数错。
    Failed,
    /// 通过率错。
    Ppm,
    /// 注解错。
    Note,
    /// 槽位错。
    Slot,
    /// 指纹错。
    Seal,
    /// 无观测。
    Nobs,
    /// 矩阵不平。
    Matrix,
    /// 次序倒置。
    Order,
    /// 账本不平。
    Bal,
}

impl Fault {
    /// 缺陷码。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Estate => AG_ESTATE,
            Fault::Stage => AG_ESTAGE,
            Fault::Code => AG_ECODE,
            Fault::Cases => AG_ECASES,
            Fault::Failed => AG_EFAILED,
            Fault::Ppm => AG_EPPM,
            Fault::Note => AG_ENOTE,
            Fault::Slot => AG_ESLOT,
            Fault::Seal => AG_ESEAL,
            Fault::Nobs => AG_ENOBS,
            Fault::Matrix => AG_EMATRIX,
            Fault::Order => AG_EORDER,
            Fault::Bal => AG_EBAL,
        }
    }

    /// 是否非绿。
    pub fn alive(&self) -> bool {
        match self {
            Fault::Ok => false,
            _ => true,
        }
    }
}

/// 缺陷短名。
pub fn fault_short(f: Fault) -> &'static str {
    match f {
        Fault::Ok => "ok",
        Fault::Estate => "estate",
        Fault::Stage => "stage",
        Fault::Code => "code",
        Fault::Cases => "cases",
        Fault::Failed => "failed",
        Fault::Ppm => "ppm",
        Fault::Note => "note",
        Fault::Slot => "slot",
        Fault::Seal => "seal",
        Fault::Nobs => "nobs",
        Fault::Matrix => "matrix",
        Fault::Order => "order",
        Fault::Bal => "bal",
    }
}

// ---------------------------------------------------------------------------
// 语料表
// ---------------------------------------------------------------------------

/// 14 个形状。前 8 个可正确申报，后 6 个复刻其中六个形状。
pub const SHAPES: [Shape; CORPUS] = [
    Shape { tag: "base19", kind: Kind::Base },
    Shape { tag: "one-red", kind: Kind::OneRed },
    Shape { tag: "one-skip", kind: Kind::OneSkip },
    Shape { tag: "all-skip", kind: Kind::AllSkip },
    Shape { tag: "unwired", kind: Kind::Unwired },
    Shape { tag: "short18", kind: Kind::Short },
    Shape { tag: "zero-assert", kind: Kind::Zero },
    Shape { tag: "many-red", kind: Kind::ManyRed },
    Shape { tag: "base-as-fail", kind: Kind::Base },
    Shape { tag: "skip-as-green", kind: Kind::OneSkip },
    Shape { tag: "unwired-as-count", kind: Kind::Unwired },
    Shape { tag: "base-short-count", kind: Kind::Base },
    Shape { tag: "red-zero-fail", kind: Kind::OneRed },
    Shape { tag: "red-pass-code", kind: Kind::OneRed },
];

/// 与 `SHAPES` 一一对应的申报。
///
/// 前 8 条是**正确**申报；后 6 条各错一维，且错维之前的所有维度都必须
/// 与实测一致，否则判序会把缺陷归到更早的维度上（这正是判序的用意）。
pub const CLAIMS: [Claim; CORPUS] = [
    // 0 base19：19 条、329 断言、全过。
    Claim { stage: Stage::Ready, code: EXIT_PASS, cases: 19, failed: 0, ppm: 1000, note: Note::Green },
    // 1 one-red：一条失败，997‰。
    Claim { stage: Stage::Run, code: EXIT_FAIL, cases: 19, failed: 1, ppm: 997, note: Note::Failing },
    // 2 one-skip：一条跳过，997‰。
    Claim { stage: Stage::Rate, code: EXIT_FAIL, cases: 19, failed: 0, ppm: 997, note: Note::Skipped },
    // 3 all-skip：全跳，零证据。
    Claim { stage: Stage::Rate, code: EXIT_FAIL, cases: 19, failed: 0, ppm: 0, note: Note::Blocked },
    // 4 unwired：注册闸门，ppm 未评估。
    Claim { stage: Stage::Registry, code: EXIT_SETUP, cases: 19, failed: 0, ppm: 0, note: Note::Unwired },
    // 5 short18：18 条，318 断言。
    Claim { stage: Stage::Count, code: EXIT_SETUP, cases: 18, failed: 0, ppm: 0, note: Note::Short },
    // 6 zero-assert：一条空断言，306 断言。
    Claim { stage: Stage::Count, code: EXIT_SETUP, cases: 19, failed: 0, ppm: 0, note: Note::Empty },
    // 7 many-red：四条失败，988‰。
    Claim { stage: Stage::Run, code: EXIT_FAIL, cases: 19, failed: 4, ppm: 988, note: Note::Failing },
    // 8 base-as-fail：把绿报成失败 —— 状态互报。
    Claim { stage: Stage::Run, code: EXIT_FAIL, cases: 19, failed: 0, ppm: 1000, note: Note::Failing },
    // 9 skip-as-green：跳过报成 1000‰ —— 速率错。
    Claim { stage: Stage::Rate, code: EXIT_FAIL, cases: 19, failed: 0, ppm: 1000, note: Note::Skipped },
    // 10 unwired-as-count：未注册报成计数闸门 —— 阶段错。
    Claim { stage: Stage::Count, code: EXIT_SETUP, cases: 19, failed: 0, ppm: 0, note: Note::Short },
    // 11 base-short-count：19 条报成 18 条 —— 条数错。
    Claim { stage: Stage::Ready, code: EXIT_PASS, cases: 18, failed: 0, ppm: 1000, note: Note::Green },
    // 12 red-zero-fail：一条失败报成零失败 —— 失败数错。
    Claim { stage: Stage::Run, code: EXIT_FAIL, cases: 19, failed: 0, ppm: 997, note: Note::Failing },
    // 13 red-pass-code：失败批报成退出码 0 —— 退出码错。
    Claim { stage: Stage::Run, code: EXIT_PASS, cases: 19, failed: 1, ppm: 997, note: Note::Failing },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Ppm, Fault::Stage, Fault::Cases, Fault::Failed, Fault::Code];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] = [
    AG_ESTATE,
    AG_EPPM,
    AG_ESTAGE,
    AG_ECASES,
    AG_EFAILED,
    AG_ECODE,
];

// ---------------------------------------------------------------------------
// 语料访问与对判
// ---------------------------------------------------------------------------

/// 语料表。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case::blank(); CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        if let (Some(s), Some(c)) = (SHAPES.get(i), CLAIMS.get(i)) {
            out[i] = Case { shape: *s, claim: *c };
        }
        i += 1;
    }
    out
}

/// 取第 i 槽。
pub fn case_at(i: usize) -> Option<Case> {
    if i >= CORPUS {
        return None;
    }
    cases().get(i).copied()
}

/// 覆盖校验。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(AG_ENOBS)
    }
}

/// 第 i 槽的设计期望。
pub fn expected(i: usize) -> Option<Fault> {
    if i < ACCEPT {
        Some(Fault::Ok)
    } else {
        REJECT_FAULTS.get(i - ACCEPT).copied()
    }
}

/// 对判：状态 → 阶段 → 退出码 → 条数 → 失败数 → 通过率 → 注解。
pub fn judge(c: &Case) -> Fault {
    let a = c.actual();
    let claim = c.claim;
    if claim.success() != a.ok() {
        return Fault::Estate;
    }
    if claim.stage != a.stage {
        return Fault::Stage;
    }
    if claim.code != a.code {
        return Fault::Code;
    }
    if claim.cases != a.cases {
        return Fault::Cases;
    }
    if claim.failed != a.failed {
        return Fault::Failed;
    }
    if claim.ppm != a.ppm {
        return Fault::Ppm;
    }
    if claim.note != a.note {
        return Fault::Note;
    }
    Fault::Ok
}

// ---------------------------------------------------------------------------
// 释义与清单
// ---------------------------------------------------------------------------

/// 单槽释义：
/// `ktest <tag> cases=<n> asserts=<a> failed=<f> skipped=<s> ppm=<p> stage=<s> note=<n>`。
pub fn detail(c: &Case) -> String {
    let a = c.actual();
    let mut s = String::from("ktest ");
    s.push_str(c.tag());
    s.push_str(" cases=");
    s.push_str(u32_text(a.cases).as_str());
    s.push_str(" asserts=");
    s.push_str(u32_text(a.asserts).as_str());
    s.push_str(" failed=");
    s.push_str(u32_text(a.failed).as_str());
    s.push_str(" skipped=");
    s.push_str(u32_text(a.skipped).as_str());
    s.push_str(" ppm=");
    s.push_str(ppm_text(a.ppm).as_str());
    s.push_str(" stage=");
    s.push_str(a.stage.name());
    s.push_str(" note=");
    s.push_str(a.note.name());
    s.push_str("\n");
    s
}

/// 一条判据的生态面清单行：
/// `<id> suite=<s> asserts=<n> wired=<yes|no>`。
pub fn manifest_line(j: &Judge) -> String {
    let mut s = String::new();
    s.push_str(j.id);
    s.push_str(" suite=");
    s.push_str(suite_name(j.suite as usize));
    s.push_str(" asserts=");
    s.push_str(u32_text(j.asserts as u32).as_str());
    s.push_str(" wired=");
    s.push_str(if j.wired { "yes" } else { "no" });
    s
}

/// 整批清单（19 行，行尾换行）。
pub fn manifest_of(b: &Batch) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < b.n {
        if let Some(j) = b.js.get(i) {
            s.push_str(manifest_line(j).as_str());
            s.push_str("\n");
        }
        i += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 套件账
// ---------------------------------------------------------------------------

/// 一行套件统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SuiteRow {
    /// 条数。
    pub cases: u32,
    /// 断言总数。
    pub asserts: u32,
    /// 失败总数。
    pub failed: u32,
    /// 跳过总数。
    pub skipped: u32,
    /// 已注册条数。
    pub wired: u32,
}

impl SuiteRow {
    /// 空白行。
    pub const fn blank() -> SuiteRow {
        SuiteRow { cases: 0, asserts: 0, failed: 0, skipped: 0, wired: 0 }
    }

    /// 通过断言数。
    pub fn passed(&self) -> u32 {
        self.asserts.saturating_sub(self.failed).saturating_sub(self.skipped)
    }

    /// 该套件是否全绿。
    pub fn green(&self) -> bool {
        self.cases > 0
            && self.wired == self.cases
            && self.failed == 0
            && self.skipped == 0
            && self.asserts > 0
    }

    /// 该套件通过率。
    pub fn ppm(&self) -> u32 {
        if self.asserts == 0 {
            return 0;
        }
        (self.passed() * 1000 + self.asserts / 2) / self.asserts
    }
}

/// 四套件账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Suites {
    /// 行。
    pub rows: [SuiteRow; SUITES],
}

impl Suites {
    /// 空账。
    pub const fn blank() -> Suites {
        Suites { rows: [SuiteRow::blank(); SUITES] }
    }

    /// 按批统计。
    pub fn of(b: &Batch) -> Suites {
        let mut out = Suites::blank();
        let mut i = 0usize;
        while i < b.n {
            if let Some(j) = b.js.get(i) {
                let k = j.suite as usize;
                if k < SUITES {
                    let mut row = match out.rows.get(k) {
                        Some(r) => *r,
                        None => SuiteRow::blank(),
                    };
                    row.cases = row.cases.saturating_add(1);
                    row.asserts = row.asserts.saturating_add(j.asserts as u32);
                    row.failed = row.failed.saturating_add(j.failed as u32);
                    row.skipped = row.skipped.saturating_add(j.skipped as u32);
                    if j.wired {
                        row.wired = row.wired.saturating_add(1);
                    }
                    out.rows[k] = row;
                }
            }
            i += 1;
        }
        out
    }

    /// 基准批的套件账。
    pub fn of_base() -> Suites {
        Suites::of(&base_batch())
    }

    /// 取一行。
    pub fn row(&self, i: usize) -> Option<SuiteRow> {
        self.rows.get(i).copied()
    }

    /// 总条数。
    pub fn cases(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < SUITES {
            if let Some(r) = self.rows.get(i) {
                n = n.saturating_add(r.cases);
            }
            i += 1;
        }
        n
    }

    /// 总断言。
    pub fn asserts(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < SUITES {
            if let Some(r) = self.rows.get(i) {
                n = n.saturating_add(r.asserts);
            }
            i += 1;
        }
        n
    }

    /// 全绿行数。
    pub fn green_rows(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < SUITES {
            if let Some(r) = self.rows.get(i) {
                if r.green() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < SUITES {
            if let Some(r) = self.rows.get(i) {
                h = fold_word(h, r.cases as u64);
                h = fold_word(h, r.asserts as u64);
                h = fold_word(h, r.failed as u64);
                h = fold_word(h, r.skipped as u64);
                h = fold_word(h, r.wired as u64);
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 断言数直方图
// ---------------------------------------------------------------------------

/// 断言规模直方图（四桶：<8 / 8..15 / 16..23 / >=24）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Histogram {
    /// 桶计数。
    pub buckets: [u32; BUCKETS],
}

impl Histogram {
    /// 空图。
    pub const fn blank() -> Histogram {
        Histogram { buckets: [0; BUCKETS] }
    }

    /// 按批统计。
    pub fn of(b: &Batch) -> Histogram {
        let mut out = Histogram::blank();
        let mut i = 0usize;
        while i < b.n {
            if let Some(j) = b.js.get(i) {
                let k = j.bucket();
                if k < BUCKETS {
                    if let Some(dst) = out.buckets.get_mut(k) {
                        *dst = dst.saturating_add(1);
                    }
                }
            }
            i += 1;
        }
        out
    }

    /// 基准批。
    pub fn of_base() -> Histogram {
        Histogram::of(&base_batch())
    }

    /// 取一桶。
    pub fn bucket(&self, i: usize) -> u32 {
        match self.buckets.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 总数。
    pub fn total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < BUCKETS {
            n = n.saturating_add(self.bucket(i));
            i += 1;
        }
        n
    }

    /// 最大桶下标。
    pub fn mode(&self) -> usize {
        let mut best = 0usize;
        let mut best_n = 0u32;
        let mut i = 0usize;
        while i < BUCKETS {
            let n = self.bucket(i);
            if n > best_n {
                best_n = n;
                best = i;
            }
            i += 1;
        }
        best
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < BUCKETS {
            h = fold_word(h, self.bucket(i) as u64);
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 档位与矩阵
// ---------------------------------------------------------------------------

/// 判据档位（按注解归类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 绿档。
    Green,
    /// 失败档。
    Fail,
    /// 证据不足档。
    Held,
    /// 无效档（接线/计数）。
    Invalid,
}

impl Tier {
    /// 由注解归位。
    pub fn of(n: Note) -> Tier {
        match n {
            Note::Green => Tier::Green,
            Note::Failing => Tier::Fail,
            Note::Skipped => Tier::Held,
            Note::Blocked => Tier::Held,
            Note::Unwired => Tier::Invalid,
            Note::Short => Tier::Invalid,
            Note::Empty => Tier::Invalid,
        }
    }

    /// 下标。
    pub fn index(&self) -> usize {
        match self {
            Tier::Green => 0,
            Tier::Fail => 1,
            Tier::Held => 2,
            Tier::Invalid => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Green => "green",
            Tier::Fail => "fail",
            Tier::Held => "held",
            Tier::Invalid => "invalid",
        }
    }
}

/// 一行统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// 该档槽数。
    pub total: usize,
    /// 该档申报正确数。
    pub good: usize,
}

impl Row {
    /// 空白行。
    pub const fn blank() -> Row {
        Row { total: 0, good: 0 }
    }

    /// 全对。
    pub fn perfect(&self) -> bool {
        self.total > 0 && self.total == self.good
    }
}

/// 四档矩阵。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行。
    pub rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 空矩阵。
    pub const fn blank() -> Matrix {
        Matrix { rows: [Row::blank(); TIER_COUNT] }
    }

    /// 由语料统计。
    pub fn of() -> Matrix {
        let mut m = Matrix::blank();
        let cs = cases();
        let mut i = 0usize;
        while i < CORPUS {
            if let Some(c) = cs.get(i) {
                let t = Tier::of(c.actual().note).index();
                if t < TIER_COUNT {
                    let mut row = match m.rows.get(t) {
                        Some(r) => *r,
                        None => Row::blank(),
                    };
                    row.total += 1;
                    if judge(c) == Fault::Ok {
                        row.good += 1;
                    }
                    m.rows[t] = row;
                }
            }
            i += 1;
        }
        m
    }

    /// 总槽数。
    pub fn total(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n += r.total;
            }
            i += 1;
        }
        n
    }

    /// 总正确数。
    pub fn good(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n += r.good;
            }
            i += 1;
        }
        n
    }

    /// 是否平。
    pub fn balanced(&self) -> bool {
        self.total() == CORPUS && self.good() == ACCEPT
    }

    /// 取一行。
    pub fn row(&self, i: usize) -> Option<Row> {
        self.rows.get(i).copied()
    }

    /// 全对行数。
    pub fn perfect_rows(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if r.perfect() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                h = fold_word(h, r.total as u64);
                h = fold_word(h, r.good as u64);
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 语料汇总。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 总槽数。
    pub total: u32,
    /// 申报正确数。
    pub accept: u32,
    /// 申报错误数。
    pub reject: u32,
    /// 绿档数。
    pub green: u32,
    /// 失败档数。
    pub fail: u32,
    /// 证据不足档数。
    pub held: u32,
    /// 无效档数。
    pub invalid: u32,
    /// 对判命中数。
    pub matched: u32,
}

impl Report {
    /// 千分表示（四舍五入）。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.matched * 1000 + self.total / 2) / self.total
    }

    /// 是否全绿。
    pub fn green_all(&self) -> bool {
        self.total == CORPUS as u32 && self.matched == self.total
    }

    /// 闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.green_all() {
            Ok(())
        } else {
            Err(AG_ECASES)
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("ktest corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" green=");
        s.push_str(u32_text(self.green).as_str());
        s.push_str(" fail=");
        s.push_str(u32_text(self.fail).as_str());
        s.push_str(" held=");
        s.push_str(u32_text(self.held).as_str());
        s.push_str(" invalid=");
        s.push_str(u32_text(self.invalid).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 可注入一槽缺陷的全跑。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(AG_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut green = 0u32;
    let mut fail = 0u32;
    let mut held = 0u32;
    let mut invalid = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(AG_ENOBS),
        };
        let a = c.actual();
        let mut f = judge(&c);
        if let Some((j, forced)) = over {
            if j == i {
                f = forced;
            }
        }
        let want = match expected(i) {
            Some(v) => v,
            None => return Err(AG_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(a.note) {
                Tier::Green => green += 1,
                Tier::Fail => fail += 1,
                Tier::Held => held += 1,
                Tier::Invalid => invalid += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, green, fail, held, invalid, matched })
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门（只许前进，不许回退）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gate {
    last: i32,
    steps: u32,
    closed: bool,
}

impl Gate {
    /// 新闸门。
    pub const fn new() -> Gate {
        Gate { last: -1, steps: 0, closed: false }
    }

    /// 走一段。
    pub fn step(&mut self, s: Stage) -> Result<(), u32> {
        if self.closed {
            return Err(AG_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(AG_EORDER);
        }
        self.last = o;
        self.steps += 1;
        Ok(())
    }

    /// 关闭。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(AG_ESTATE);
        }
        self.closed = true;
        Ok(self.steps)
    }

    /// 段数。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// 上一段。
    pub const fn last(&self) -> i32 {
        self.last
    }

    /// 是否已关。
    pub const fn closed(&self) -> bool {
        self.closed
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 账本格。
#[derive(Clone, Copy, Debug)]
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
    pub fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AG_ESLOT);
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

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一格。
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

    /// 对账：14 格、8 成 6 败，成功格按 `VALS`，失败格按 `REJECT_CODES`。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(AG_EBAL);
        }
        // 前七格是实测计数；第八格是"槽 0 强制红"后的通过率 929‰。
        const VALS: [u32; ACCEPT] = [14, 8, 6, 1, 2, 2, 3, 929];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(AG_EBAL),
            };
            if s.slot as usize != i {
                return Err(AG_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(AG_EBAL);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(AG_EBAL);
                        }
                    }
                    None => return Err(AG_EBAL),
                }
                vi += 1;
            } else {
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(AG_EBAL);
                        }
                    }
                    None => return Err(AG_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(AG_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold_word(h, s.slot as u64);
                h = fold_word(h, s.op as u64);
                h = fold_word(h, s.ok as u64);
                h = fold_word(h, s.code as u64);
                h = fold_word(h, s.value as u64);
            }
            i += 1;
        }
        fold_word(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AG_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(AG_ESEAL)
        }
    }

    /// 冻结指纹。
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub const fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// `Result<u32, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 建账本：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total)));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept)));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject)));
    let _ = l.register(count_cell(3, OP_SWEEP, r.map(|x| x.green)));
    let _ = l.register(count_cell(4, OP_SWEEP, r.map(|x| x.fail)));
    let _ = l.register(count_cell(5, OP_RATE, r.map(|x| x.held)));
    let _ = l.register(count_cell(6, OP_REG, r.map(|x| x.invalid)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_RATE, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_GATE,
                ok: false,
                code: f.code(),
                value: 0,
            });
        }
        i += 1;
    }
    l
}

// ---------------------------------------------------------------------------
// 注入
// ---------------------------------------------------------------------------

/// 四路注入：表、判、渲染、账本。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && table_ok() && suite_ok();
    out[1] = hist_ok() && sweep_ok() && judge_ok();
    out[2] = matrix_ok() && corpus_ok() && reds_ok() && detail_ok();
    out[3] = slots_ok()
        && gate_ok()
        && ledger_ok()
        && {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            a && b
        };
    out
}

// ---------------------------------------------------------------------------
// 判据基元
// ---------------------------------------------------------------------------

/// 段基址、码表、规模常量。
fn name_ok() -> bool {
    let codes = [
        AG_ESTATE, AG_ESTAGE, AG_ECODE, AG_ECASES, AG_EFAILED, AG_EPPM, AG_ENOTE, AG_ESLOT,
        AG_ESEAL, AG_ENOBS, AG_EMATRIX, AG_EORDER, AG_EBAL,
    ];
    let mut i = 0usize;
    while i < codes.len() {
        let mut j = i + 1;
        while j < codes.len() {
            match (codes.get(i), codes.get(j)) {
                (Some(a), Some(b)) => {
                    if a == b {
                        return false;
                    }
                }
                _ => return false,
            }
            j += 1;
        }
        i += 1;
    }
    let a = SEG_BASE == 0x6cf10;
    let b = AG_ESTATE == SEG_BASE + 0x01 && AG_EBAL == SEG_BASE + 0x0d;
    let c = SAMPLES == 14 && CORPUS == 14 && ACCEPT == 8 && REJECT == 6;
    let d = JUDGES_N == 19 && SUITES == 4 && BUCKETS == 4;
    let e = EXIT_PASS == 0 && EXIT_FAIL == 1 && EXIT_SETUP == 2;
    let f = BUCKET_1 == 8 && BUCKET_2 == 16 && BUCKET_3 == 24;
    a && b && c && d && e && f
}

/// 19 条判据表。
fn table_ok() -> bool {
    if JUDGES.len() != JUDGES_N {
        return false;
    }
    let mut sum = 0u32;
    let mut wired = 0u32;
    let mut i = 0usize;
    while i < JUDGES.len() {
        let j = match JUDGES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if j.id.is_empty() || !suite_valid(j.suite as usize) {
            return false;
        }
        if !j.consistent() || j.asserts == 0 {
            return false;
        }
        if !j.wired {
            return false;
        }
        wired += 1;
        sum += j.asserts as u32;
        let mut k = 0usize;
        while k < i {
            if let Some(p) = JUDGES.get(k) {
                if p.id == j.id {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    let a = sum == 329 && wired == 19;
    let b = base_batch().n == 19 && base_batch().cover() == Ok(());
    let c = manifest_line(&JUDGES[0]).as_str() == "b15.core.entry suite=core asserts=19 wired=yes";
    let d = manifest_of(&base_batch()).len() > 0;
    a && b && c && d
}

/// 四套件账。
fn suite_ok() -> bool {
    let s = Suites::of_base();
    let a = s.row(0) == Some(SuiteRow { cases: 6, asserts: 112, failed: 0, skipped: 0, wired: 6 });
    let b = s.row(1) == Some(SuiteRow { cases: 5, asserts: 87, failed: 0, skipped: 0, wired: 5 });
    let c = s.row(2) == Some(SuiteRow { cases: 4, asserts: 70, failed: 0, skipped: 0, wired: 4 });
    let d = s.row(3) == Some(SuiteRow { cases: 4, asserts: 60, failed: 0, skipped: 0, wired: 4 });
    let e = s.cases() == 19 && s.asserts() == 329 && s.green_rows() == 4;
    let f = s.seal() == Suites::of_base().seal() && s.seal() != Suites::blank().seal();
    let g = suite_name(0) == "core"
        && suite_name(1) == "fs"
        && suite_name(2) == "proc"
        && suite_name(3) == "io"
        && suite_name(4) == "?"
        && !suite_valid(4);
    a && b && c && d && e && f && g
}

/// 直方图。
fn hist_ok() -> bool {
    let h = Histogram::of_base();
    let a = h.bucket(0) == 2 && h.bucket(1) == 6 && h.bucket(2) == 8 && h.bucket(3) == 3;
    let b = h.total() == 19 && h.mode() == 2;
    let c = h.seal() == Histogram::of_base().seal() && h.seal() != Histogram::blank().seal();
    let d = JUDGES[6].bucket() == 0 && JUDGES[8].bucket() == 3 && JUDGES[0].bucket() == 2;
    a && b && c && d
}

/// 五种闸门路径。
fn sweep_ok() -> bool {
    let g = sweep(&batch_for(Kind::Base));
    let a = g.ok()
        && g.stage == Stage::Ready
        && g.note == Note::Green
        && g.cases == 19
        && g.asserts == 329
        && g.ppm == 1000;
    let r = sweep(&batch_for(Kind::OneRed));
    let b = r.stage == Stage::Run
        && r.code == EXIT_FAIL
        && r.note == Note::Failing
        && r.failed == 1
        && r.ppm == 997;
    let s = sweep(&batch_for(Kind::OneSkip));
    let c = s.stage == Stage::Rate
        && s.code == EXIT_FAIL
        && s.note == Note::Skipped
        && s.failed == 0
        && s.skipped == 1
        && s.ppm == 997;
    let k = sweep(&batch_for(Kind::AllSkip));
    let d = k.stage == Stage::Rate
        && k.note == Note::Blocked
        && k.skipped == 329
        && k.ppm == 0
        && k.code == EXIT_FAIL;
    let u = sweep(&batch_for(Kind::Unwired));
    let e = u.stage == Stage::Registry
        && u.note == Note::Unwired
        && u.code == EXIT_SETUP
        && u.ppm == 0
        && u.asserts == 329;
    let sh = sweep(&batch_for(Kind::Short));
    let f = sh.stage == Stage::Count && sh.note == Note::Short && sh.cases == 18 && sh.asserts == 318;
    let z = sweep(&batch_for(Kind::Zero));
    let h = z.stage == Stage::Count && z.note == Note::Empty && z.cases == 19 && z.asserts == 306;
    let m = sweep(&batch_for(Kind::ManyRed));
    let i = m.stage == Stage::Run && m.failed == 4 && m.ppm == 988;
    a && b && c && d && e && f && h && i
}

/// 申报对判。
fn judge_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < ACCEPT {
        match cs.get(i) {
            Some(c) => {
                if judge(c) != Fault::Ok {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < REJECT {
        let c = match cs.get(j + ACCEPT) {
            Some(v) => *v,
            None => return false,
        };
        let want = match REJECT_FAULTS.get(j) {
            Some(v) => *v,
            None => return false,
        };
        let code = match REJECT_CODES.get(j) {
            Some(v) => *v,
            None => return false,
        };
        if judge(&c) != want || judge(&c).code() != code {
            return false;
        }
        j += 1;
    }
    true
}

/// 矩阵。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    a_and(
        m.row(0) == Some(Row { total: 3, good: 1 }),
        m.row(1) == Some(Row { total: 4, good: 2 }),
        m.row(2) == Some(Row { total: 3, good: 2 }),
        m.row(3) == Some(Row { total: 4, good: 3 }),
        m.row(4).is_none(),
        m.total() == CORPUS,
        m.good() == ACCEPT,
        m.balanced(),
        m.perfect_rows() == 0,
        m.seal() == Matrix::of().seal(),
        m.seal() != Matrix::blank().seal(),
    )
}

/// 极小与门（避免长串 `&&` 触发格式噪声，保持单一出口）。
fn a_and(a: bool, b: bool, c: bool, d: bool, e: bool, f: bool, g: bool, h: bool, i: bool, j: bool, k: bool) -> bool {
    a && b && c && d && e && f && g && h && i && j && k
}

/// 标签唯一。
fn tags_unique() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.tag().is_empty() {
            return false;
        }
        let mut j = 0usize;
        while j < i {
            if let Some(p) = cs.get(j) {
                if p.tag() == c.tag() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 每槽都能渲染出非空释义，且绿槽的 ppm 必须是 1000。
fn notes_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if detail(&c).as_str().is_empty() {
            return false;
        }
        let a = c.actual();
        if a.note == Note::Green && a.ppm != 1000 {
            return false;
        }
        if a.ok() && (a.stage != Stage::Ready || a.code != EXIT_PASS) {
            return false;
        }
        if a.stage.setup() && a.code != EXIT_SETUP {
            return false;
        }
        i += 1;
    }
    true
}

/// 语料与渲染。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total == 14 && r.accept == 8 && r.reject == 6 && r.matched == 14;
    let b = r.green == 1 && r.fail == 2 && r.held == 2 && r.invalid == 3;
    let c = r.rate() == 1000 && r.green_all() && r.gate() == Ok(());
    let d = tags_unique() && notes_ok() && judge_ok();
    a && b && c && d
}

/// 红档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == 13 && r.rate() == 929;
    let b = r.gate() == Err(AG_ECASES) && !r.green_all();
    let c = run_with(Some((CORPUS, Fault::Code))).err() == Some(AG_ESLOT);
    a && b && c
}

/// 绿档释义。
const DETAIL_BASE: &str =
    "ktest base19 cases=19 asserts=329 failed=0 skipped=0 ppm=1000 stage=ready note=green\n";
/// 跳过档释义。
const DETAIL_SKIP: &str =
    "ktest one-skip cases=19 asserts=329 failed=0 skipped=1 ppm=997 stage=rate note=skipped\n";
/// 未注册档释义。
const DETAIL_UNWIRED: &str =
    "ktest unwired cases=19 asserts=329 failed=0 skipped=0 ppm=0 stage=registry note=unwired\n";
/// 短批释义。
const DETAIL_SHORT: &str =
    "ktest short18 cases=18 asserts=318 failed=0 skipped=0 ppm=0 stage=count note=short\n";

/// 释义逐字。
fn detail_ok() -> bool {
    let cs = cases();
    let a = match cs.get(0) {
        Some(c) => detail(c).as_str() == DETAIL_BASE,
        None => false,
    };
    let b = match cs.get(2) {
        Some(c) => detail(c).as_str() == DETAIL_SKIP,
        None => false,
    };
    let c = match cs.get(4) {
        Some(c) => detail(c).as_str() == DETAIL_UNWIRED,
        None => false,
    };
    let d = match cs.get(5) {
        Some(c) => detail(c).as_str() == DETAIL_SHORT,
        None => false,
    };
    a && b && c && d
}

/// 槽号、阶段名、档位名、退出码映射。
fn slots_ok() -> bool {
    let a = match case_at(0) {
        Some(c) => c.tag() == "base19" && c.shape.kind == Kind::Base,
        None => false,
    };
    let b = match case_at(13) {
        Some(c) => c.tag() == "red-pass-code" && c.shape.kind == Kind::OneRed,
        None => false,
    };
    let c = case_at(CORPUS).is_none();
    let d = cover(CORPUS) == Ok(()) && cover(1) == Err(AG_ENOBS);
    let e = Stage::Registry.order() == 0
        && Stage::Count.order() == 1
        && Stage::Run.order() == 2
        && Stage::Rate.order() == 3
        && Stage::Ready.order() == 4;
    let f = Stage::Registry.name() == "registry"
        && Stage::Count.name() == "count"
        && Stage::Run.name() == "run"
        && Stage::Rate.name() == "rate"
        && Stage::Ready.name() == "ready";
    let g = Stage::Registry.setup() && Stage::Count.setup() && !Stage::Run.setup();
    let h = Tier::Green.index() == 0
        && Tier::Fail.index() == 1
        && Tier::Held.index() == 2
        && Tier::Invalid.index() == 3;
    let i = Tier::Green.name() == "green"
        && Tier::Fail.name() == "fail"
        && Tier::Held.name() == "held"
        && Tier::Invalid.name() == "invalid";
    let j = fault_short(Fault::Estate) == "estate"
        && fault_short(Fault::Ppm) == "ppm"
        && fault_short(Fault::Ok) == "ok";
    let k = Kind::Base.name() == "base" && Kind::ManyRed.name() == "many-red";
    a && b && c && d && e && f && g && h && i && j && k
}

/// 阶段闸门。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    let a = g.steps() == 0 && !g.closed() && g.last() == -1;
    let b = g.step(Stage::Count) == Ok(()) && g.steps() == 1;
    let c = g.step(Stage::Registry) == Err(AG_EORDER) && g.steps() == 1;
    let d = g.step(Stage::Run) == Ok(()) && g.last() == 2 && g.steps() == 2;
    let e = g.step(Stage::Rate) == Ok(()) && g.step(Stage::Ready) == Ok(()) && g.steps() == 4;
    let f = g.close() == Ok(4) && g.closed();
    let h = g.close() == Err(AG_ESTATE);
    let i = g.step(Stage::Ready) == Err(AG_ESTATE);
    a && b && c && d && e && f && h && i
}

/// 账本形状。
fn ledger_ok() -> bool {
    let l = build_ledger();
    l.len() == SAMPLES
        && l.ok_count() == ACCEPT
        && l.err_count() == REJECT
        && l.audit() == Ok(())
        && l.sample(100).is_none()
        && !l.is_empty()
}

// ---------------------------------------------------------------------------
// 判据集
// ---------------------------------------------------------------------------

/// 13 条判据。
pub fn run_unx11500_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11500_ktest_agg19");

    s.add("AG01-名称与段基址", name_ok(), "static");

    s.add("AG02-十九条判据表", table_ok(), "static");

    s.add("AG03-四套件账", suite_ok(), "static");

    s.add("AG04-断言直方图", hist_ok(), "static");

    s.add("AG05-五道闸门路径", sweep_ok(), "static");

    s.add("AG06-申报对判", judge_ok(), "static");

    s.add("AG07-档位矩阵", matrix_ok(), "static");

    s.add("AG08-语料与渲染", corpus_ok(), "static");

    s.add("AG09-红档显式", reds_ok(), "static");

    s.add("AG10-释义逐字", detail_ok(), "static");

    s.add("AG11-槽号与命名", slots_ok(), "static");

    s.add("AG12-阶段闸门", gate_ok(), "static");

    s.add(
        "AG13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            let inj = inject_four();
            a && b && inj[0] && inj[1] && inj[2] && inj[3]
        },
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

    fn fail() -> ! {
        panic!("unx11500 test fail")
    }

    #[test]
    fn seg_consts() {
        assert_eq!(SEG_BASE, 0x6cf10);
        assert_eq!(SAMPLES, 14);
        assert_eq!(CORPUS, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(JUDGES_N, 19);
        assert_eq!(EXIT_PASS, 0);
        assert_eq!(EXIT_FAIL, 1);
        assert_eq!(EXIT_SETUP, 2);
    }

    #[test]
    fn code_uniqueness() {
        let codes = [
            AG_ESTATE, AG_ESTAGE, AG_ECODE, AG_ECASES, AG_EFAILED, AG_EPPM, AG_ENOTE, AG_ESLOT,
            AG_ESEAL, AG_ENOBS, AG_EMATRIX, AG_EORDER, AG_EBAL,
        ];
        let mut i = 0usize;
        while i < codes.len() {
            let mut j = i + 1;
            while j < codes.len() {
                assert_ne!(codes[i], codes[j]);
                j += 1;
            }
            i += 1;
        }
        assert_eq!(AG_ESTATE, SEG_BASE + 1);
        assert_eq!(AG_EBAL, SEG_BASE + 13);
    }

    #[test]
    fn judge_table_shape() {
        assert_eq!(JUDGES.len(), 19);
        assert_eq!(JUDGES[0].id, "b15.core.entry");
        assert_eq!(JUDGES[18].id, "b15.io.readv");
        assert_eq!(JUDGES[0].suite, 0);
        assert_eq!(JUDGES[18].suite, 3);
        assert_eq!(JUDGES[6].asserts, 7);
        assert_eq!(JUDGES[8].asserts, 31);
    }

    #[test]
    fn judge_consistency() {
        let mut i = 0usize;
        while i < JUDGES.len() {
            let j = JUDGES[i];
            assert!(j.consistent());
            assert!(j.asserts > 0);
            assert!(j.wired);
            assert_eq!(j.empty(), false);
            assert_eq!(j.passed(), j.asserts as u32);
            assert_eq!(j.ppm(), 1000);
            assert!(j.green());
            i += 1;
        }
    }

    #[test]
    fn judge_mutations() {
        assert_eq!(JUDGES[5].passed(), 29);
        assert!(!JUDGES[5].green() || JUDGES[5].failed == 0);
        let mut b = base_batch();
        assert!(mark_failed(&mut b, 5, 1));
        assert_eq!(b.failed(), 1);
        assert_eq!(b.passed(), 328);
        assert_eq!(b.ppm(), 997);
        assert!(!mark_failed(&mut b, 5, 99));
        assert!(!mark_failed(&mut b, 99, 1));
        assert_eq!(b.js[5].failed, 1);
    }

    #[test]
    fn batch_base() {
        let b = batch_for(Kind::Base);
        assert_eq!(b.n, 19);
        assert_eq!(b.asserts(), 329);
        assert_eq!(b.failed(), 0);
        assert_eq!(b.skipped(), 0);
        assert_eq!(b.unwired(), 0);
        assert_eq!(b.zeros(), 0);
        assert_eq!(b.ppm(), 1000);
        assert_eq!(b.cover(), Ok(()));
        let o = sweep(&b);
        assert!(o.ok());
        assert_eq!(o.note, Note::Green);
    }

    #[test]
    fn batch_one_red() {
        let b = batch_for(Kind::OneRed);
        assert_eq!(b.failed(), 1);
        assert_eq!(b.asserts(), 329);
        assert_eq!(b.ppm(), 997);
        let o = sweep(&b);
        assert!(!o.ok());
        assert_eq!(o.stage, Stage::Run);
        assert_eq!(o.note, Note::Failing);
        assert_eq!(o.code, EXIT_FAIL);
    }

    #[test]
    fn batch_one_skip() {
        let b = batch_for(Kind::OneSkip);
        assert_eq!(b.skipped(), 1);
        assert_eq!(b.passed(), 328);
        assert_eq!(b.ppm(), 997);
        let o = sweep(&b);
        assert_eq!(o.stage, Stage::Rate);
        assert_eq!(o.note, Note::Skipped);
        assert_eq!(o.skipped, 1);
    }

    #[test]
    fn batch_all_skip() {
        let b = batch_for(Kind::AllSkip);
        assert_eq!(b.skipped(), 329);
        assert_eq!(b.passed(), 0);
        assert_eq!(b.ppm(), 0);
        let o = sweep(&b);
        assert_eq!(o.note, Note::Blocked);
        assert_eq!(o.stage, Stage::Rate);
    }

    #[test]
    fn batch_unwired() {
        let b = batch_for(Kind::Unwired);
        assert_eq!(b.unwired(), 1);
        assert_eq!(b.asserts(), 329);
        let o = sweep(&b);
        assert_eq!(o.stage, Stage::Registry);
        assert_eq!(o.note, Note::Unwired);
        assert_eq!(o.code, EXIT_SETUP);
        assert_eq!(o.ppm, 0);
    }

    #[test]
    fn batch_short() {
        let b = batch_for(Kind::Short);
        assert_eq!(b.n, 18);
        assert_eq!(b.asserts(), 318);
        let o = sweep(&b);
        assert_eq!(o.stage, Stage::Count);
        assert_eq!(o.note, Note::Short);
        assert_eq!(o.cases, 18);
    }

    #[test]
    fn batch_zero() {
        let b = batch_for(Kind::Zero);
        assert_eq!(b.zeros(), 1);
        assert_eq!(b.asserts(), 306);
        let o = sweep(&b);
        assert_eq!(o.stage, Stage::Count);
        assert_eq!(o.note, Note::Empty);
        assert_eq!(o.code, EXIT_SETUP);
    }

    #[test]
    fn batch_many_red() {
        let b = batch_for(Kind::ManyRed);
        assert_eq!(b.failed(), 4);
        assert_eq!(b.passed(), 325);
        assert_eq!(b.ppm(), 988);
        assert_eq!(sweep(&b).note, Note::Failing);
    }

    #[test]
    fn gate_paths_are_terminal() {
        let kinds = [
            Kind::Base,
            Kind::OneRed,
            Kind::OneSkip,
            Kind::AllSkip,
            Kind::Unwired,
            Kind::Short,
            Kind::Zero,
            Kind::ManyRed,
        ];
        let mut i = 0usize;
        while i < kinds.len() {
            let o = sweep(&batch_for(kinds[i]));
            if o.ok() {
                assert_eq!(o.ppm, 1000);
                assert_eq!(o.code, 0);
            } else {
                assert!(o.code != 0);
            }
            i += 1;
        }
    }

    #[test]
    fn suite_rows() {
        let s = Suites::of_base();
        assert_eq!(s.cases(), 19);
        assert_eq!(s.asserts(), 329);
        assert_eq!(s.green_rows(), 4);
        assert_eq!(s.row(0), Some(SuiteRow { cases: 6, asserts: 112, failed: 0, skipped: 0, wired: 6 }));
        assert_eq!(s.row(1), Some(SuiteRow { cases: 5, asserts: 87, failed: 0, skipped: 0, wired: 5 }));
        assert_eq!(s.row(2), Some(SuiteRow { cases: 4, asserts: 70, failed: 0, skipped: 0, wired: 4 }));
        assert_eq!(s.row(3), Some(SuiteRow { cases: 4, asserts: 60, failed: 0, skipped: 0, wired: 4 }));
        assert_eq!(s.row(4), None);
    }

    #[test]
    fn suite_seal() {
        assert_eq!(Suites::of_base().seal(), Suites::of_base().seal());
        assert_ne!(Suites::of_base().seal(), Suites::blank().seal());
        let r = SuiteRow { cases: 2, asserts: 21, failed: 1, skipped: 0, wired: 2 };
        assert_eq!(r.passed(), 20);
        assert_eq!(r.ppm(), 952);
        assert!(!r.green());
    }

    #[test]
    fn histogram_buckets() {
        let h = Histogram::of_base();
        assert_eq!(h.total(), 19);
        assert_eq!(h.bucket(0), 2);
        assert_eq!(h.bucket(1), 6);
        assert_eq!(h.bucket(2), 8);
        assert_eq!(h.bucket(3), 3);
        assert_eq!(h.mode(), 2);
        assert_eq!(h.bucket(4), 0);
        assert_eq!(h.seal(), Histogram::of_base().seal());
        assert_ne!(h.seal(), Histogram::blank().seal());
    }

    #[test]
    fn matrix_rows() {
        let m = Matrix::of();
        assert_eq!(m.total(), 14);
        assert_eq!(m.good(), 8);
        assert!(m.balanced());
        assert_eq!(m.row(0), Some(Row { total: 3, good: 1 }));
        assert_eq!(m.row(1), Some(Row { total: 4, good: 2 }));
        assert_eq!(m.row(2), Some(Row { total: 3, good: 2 }));
        assert_eq!(m.row(3), Some(Row { total: 4, good: 3 }));
        assert_eq!(m.row(4), None);
        assert_eq!(m.perfect_rows(), 0);
    }

    #[test]
    fn matrix_seal() {
        assert_eq!(Matrix::of().seal(), Matrix::of().seal());
        assert_ne!(Matrix::of().seal(), Matrix::blank().seal());
    }

    #[test]
    fn report_counts() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.total, 14);
        assert_eq!(r.accept, 8);
        assert_eq!(r.reject, 6);
        assert_eq!(r.green, 1);
        assert_eq!(r.fail, 2);
        assert_eq!(r.held, 2);
        assert_eq!(r.invalid, 3);
        assert_eq!(r.matched, 14);
        assert_eq!(r.rate(), 1000);
        assert!(r.green_all());
    }

    #[test]
    fn report_render_exact() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(
            r.render().as_str(),
            "ktest corpus=14 accept=8 reject=6 green=1 fail=2 held=2 invalid=3 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((0, Fault::Code))) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.matched, 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(AG_ECASES));
        assert_eq!(run_with(Some((14, Fault::Code))).err(), Some(AG_ESLOT));
    }

    #[test]
    fn detail_exact() {
        let cs = cases();
        assert_eq!(detail(&cs[0]).as_str(), DETAIL_BASE);
        assert_eq!(detail(&cs[2]).as_str(), DETAIL_SKIP);
        assert_eq!(detail(&cs[4]).as_str(), DETAIL_UNWIRED);
        assert_eq!(detail(&cs[5]).as_str(), DETAIL_SHORT);
        assert_eq!(detail(&cs[0]).len(), 85);
    }

    #[test]
    fn manifest_exact() {
        assert_eq!(
            manifest_line(&JUDGES[0]).as_str(),
            "b15.core.entry suite=core asserts=19 wired=yes"
        );
        assert_eq!(
            manifest_line(&JUDGES[18]).as_str(),
            "b15.io.readv suite=io asserts=11 wired=yes"
        );
        let m = manifest_of(&base_batch());
        assert!(m.as_str().contains("b15.fs.write suite=fs asserts=31 wired=yes"));
    }

    #[test]
    fn slots_bounds() {
        assert!(case_at(0).is_some());
        assert!(case_at(13).is_some());
        assert!(case_at(14).is_none());
        assert_eq!(cover(14), Ok(()));
        assert_eq!(cover(1), Err(AG_ENOBS));
    }

    #[test]
    fn stage_order() {
        assert_eq!(Stage::Registry.order(), 0);
        assert_eq!(Stage::Count.order(), 1);
        assert_eq!(Stage::Run.order(), 2);
        assert_eq!(Stage::Rate.order(), 3);
        assert_eq!(Stage::Ready.order(), 4);
        assert!(Stage::Registry.setup());
        assert!(!Stage::Rate.setup());
    }

    #[test]
    fn tier_names() {
        assert_eq!(Tier::of(Note::Green).name(), "green");
        assert_eq!(Tier::of(Note::Failing).name(), "fail");
        assert_eq!(Tier::of(Note::Skipped).name(), "held");
        assert_eq!(Tier::of(Note::Blocked).name(), "held");
        assert_eq!(Tier::of(Note::Unwired).name(), "invalid");
        assert_eq!(Tier::of(Note::Short).name(), "invalid");
        assert_eq!(Tier::of(Note::Empty).name(), "invalid");
    }

    #[test]
    fn fault_names() {
        assert_eq!(fault_short(Fault::Ok), "ok");
        assert_eq!(fault_short(Fault::Estate), "estate");
        assert_eq!(fault_short(Fault::Ppm), "ppm");
        assert_eq!(fault_short(Fault::Bal), "bal");
        assert!(!Fault::Ok.alive());
        assert!(Fault::Cases.alive());
        assert_eq!(Fault::Code.code(), AG_ECODE);
    }

    #[test]
    fn expected_table() {
        let mut i = 0usize;
        while i < ACCEPT {
            assert_eq!(expected(i), Some(Fault::Ok));
            i += 1;
        }
        let mut j = 0usize;
        while j < REJECT {
            assert_eq!(expected(ACCEPT + j), Some(REJECT_FAULTS[j]));
            j += 1;
        }
        assert_eq!(expected(CORPUS), None);
    }

    #[test]
    fn gate_monotonic() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Count), Ok(()));
        assert_eq!(g.step(Stage::Registry), Err(AG_EORDER));
        assert_eq!(g.steps(), 1);
        assert_eq!(g.step(Stage::Run), Ok(()));
        assert_eq!(g.last(), 2);
        assert_eq!(g.step(Stage::Rate), Ok(()));
        assert_eq!(g.step(Stage::Ready), Ok(()));
        assert_eq!(g.steps(), 4);
    }

    #[test]
    fn gate_close() {
        let mut g = Gate::new();
        assert_eq!(g.close(), Ok(0));
        assert!(g.closed());
        assert_eq!(g.close(), Err(AG_ESTATE));
        assert_eq!(g.step(Stage::Ready), Err(AG_ESTATE));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(100).is_none());
        assert!(!l.is_empty());
        assert_eq!(l.sample(0).map(|s| s.value), Some(14));
        assert_eq!(l.sample(7).map(|s| s.value), Some(929));
        assert_eq!(l.sample(8).map(|s| s.code), Some(AG_ESTATE));
        assert_eq!(l.sample(13).map(|s| s.code), Some(AG_ECODE));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let r = match l.freeze() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_ne!(r, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), r);
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.register(Sample::blank()).err(), Some(AG_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(AG_EBAL));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn injections() {
        let out = inject_four();
        assert!(out[0], "inject0 table");
        assert!(out[1], "inject1 judge");
        assert!(out[2], "inject2 render");
        assert!(out[3], "inject3 ledger");
    }

    #[test]
    fn text_helpers() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(329).as_str(), "329");
        assert_eq!(u32_text(1000).as_str(), "1000");
        assert_eq!(hex_text(0x6cf10, 5).as_str(), "6cf10");
        assert_eq!(ppm_text(997).as_str(), "997");
    }

    #[test]
    fn batch_seal_differs() {
        let a = batch_for(Kind::Base).seal();
        let b = batch_for(Kind::OneRed).seal();
        let c = batch_for(Kind::Unwired).seal();
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a, batch_for(Kind::Base).seal());
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11500_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
