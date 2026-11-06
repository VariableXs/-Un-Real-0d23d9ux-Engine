//! UNX-F11634 .so 权限位与映射 PROT 联动判据
//!
//! 动态装载器对一段 PT_LOAD 的最终保护是**两层**决定的：
//!
//! 1. 文件权限位决定"能不能开"：没有读位，`open()`/`read()` 先死，映射
//!    根本不会发生。这一步与 PROT 无关，但它是 PROT 的前置——所以本模块
//!    把"无读位拒载"和"映射 PROT"放进同一张判据表，而不是拆成两卡。
//! 2. 段的 `p_flags`（PF_R/PF_W/PF_X）决定"该给什么 PROT"：`mmap(PROT_*)`。
//!    但这不是简单的一一对应：
//!    - `PF_W` 只说明**私有映射**里可以写。`MAP_PRIVATE` 的写是写时复制，
//!      与文件是否有写位无关；`MAP_SHARED` 才要求文件可写/可写打开，否则
//!      EACCES。
//!    - `PF_X` 的映射要 `PROT_EXEC`，在 `noexec` 挂载点上被拒（EACCES）。
//!    - `PF_W|PF_X` 同段在 W^X 策略下非法（EINVAL），装载器必须先拆段再
//!      映射；本模块按"未拆即拒"建模。
//!    - `GNU_RELRO` 覆盖的写段在重定位完成后必须 `mprotect` 收掉写位，
//!      最终 PROT 只剩读——"文件里有 W 段"不等于"运行期可写"。
//!
//! 因此同一份输入会出现三种非常容易被混淆的结论：
//!
//! - 文件是 `0444`、段是 `PF_R|PF_W`：**成功**，私有写时复制，最终
//!   `PROT_READ|PROT_WRITE`。误判"文件无写位 → 拒绝写映射"是错的。
//! - 文件是 `0644`、段是 `PF_R|PF_W`、`MAP_SHARED`：**失败**，EACCES，
//!   错在共享映射而不是文件权限。
//! - 文件是 `0755`、段是 `PF_R|PF_W`、带 RELRO：**成功**，最终
//!   `PROT_READ`，写位在重定位后被收掉，`PROT_WRITE` 只是中间态。
//!
//! 判据的五个维度：阶段（open/protect/map/reloc/ready）、errno、期望 PROT
//! （段旗标并集）、实际 PROT（成功时的最终值）、注解（plain/private-copy/
//! relro/denied-read/shared-write/no-exec/wx/empty）。只比成功与失败，
//! 上面三例的第一例与第三例会与第二例混在一起；只比最终 PROT，第三例会
//! 被当成"丢了一位"的缺陷。
//!
//! 判序：状态 → 阶段 → errno → 实际 PROT → 注解。状态（成功/失败互报）
//! 最先，因为它不是数值差异而是语义矛盾。
//!
//! 语料 14 槽：8 槽申报正确（拒绝档 1、写档 4、执行档 2、纯读档 1），
//! 6 槽各挂一处错误申报（状态/码/阶段/注解/PROT/注解）。账本 14 格
//! 8 成 6 败，冻结后重放同指纹。
//!
//! 闸门次序：读位 → 段表 → 并集 → W^X → 共享写 → noexec → RELRO → 就绪。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本在
//! `audit` 逐格对账。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfcb0;

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

/// 段表容量。
pub const MAX_SEGS: usize = 3;

/// 文案长度上限。
pub const MAX_MSG: usize = 256;

// ---------------------------------------------------------------------------
// mmap PROT（Linux 取值）
// ---------------------------------------------------------------------------

/// 可读。
pub const PROT_READ: u8 = 0x1;
/// 可写。
pub const PROT_WRITE: u8 = 0x2;
/// 可执行。
pub const PROT_EXEC: u8 = 0x4;

/// PROT 全集。
pub const PROT_ALL: u8 = PROT_READ | PROT_WRITE | PROT_EXEC;

// ---------------------------------------------------------------------------
// ELF p_flags（ELF 取值）
// ---------------------------------------------------------------------------

/// 可执行段。
pub const PF_X: u8 = 0x1;
/// 可写段。
pub const PF_W: u8 = 0x2;
/// 可读段。
pub const PF_R: u8 = 0x4;

/// 段旗标全集。
pub const PF_ALL: u8 = PF_R | PF_W | PF_X;

// ---------------------------------------------------------------------------
// errno
// ---------------------------------------------------------------------------

/// 不存在。
pub const ERR_ENOENT: i32 = 2;
/// 不是可执行格式。
pub const ERR_ENOEXEC: i32 = 8;
/// 拒绝访问。
pub const ERR_EACCES: i32 = 13;
/// 参数非法。
pub const ERR_EINVAL: i32 = 22;

// ---------------------------------------------------------------------------
// 错误码（13 个）
// ---------------------------------------------------------------------------

/// 阶段申报不符。
pub const EA_ESTAGE: u32 = SEG_BASE + 0x01;
/// errno 申报不符。
pub const EA_ECODE: u32 = SEG_BASE + 0x02;
/// 实际 PROT 申报不符。
pub const EA_EPROT: u32 = SEG_BASE + 0x03;
/// 注解申报不符。
pub const EA_ENOTE: u32 = SEG_BASE + 0x04;
/// 状态矛盾。
pub const EA_ESTATE: u32 = SEG_BASE + 0x05;
/// 计数不符。
pub const EA_ECOUNT: u32 = SEG_BASE + 0x06;
/// 观测值不符。
pub const EA_EVALUE: u32 = SEG_BASE + 0x07;
/// 槽位非法。
pub const EA_ESLOT: u32 = SEG_BASE + 0x08;
/// 指纹不符。
pub const EA_ESEAL: u32 = SEG_BASE + 0x09;
/// 无观测。
pub const EA_ENOBS: u32 = SEG_BASE + 0x0a;
/// 矩阵不平。
pub const EA_EMATRIX: u32 = SEG_BASE + 0x0b;
/// 阶段次序倒置。
pub const EA_EORDER: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const EA_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 读位。
pub const OP_OPEN: u8 = 0x01;
/// 段表。
pub const OP_SEG: u8 = 0x02;
/// 并集。
pub const OP_UNION: u8 = 0x03;
/// 保护。
pub const OP_PROT: u8 = 0x04;
/// 映射。
pub const OP_MAP: u8 = 0x05;
/// RELRO。
pub const OP_RELRO: u8 = 0x06;
/// 语料。
pub const OP_CORPUS: u8 = 0x07;

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

/// 三位八进制文本（权限位）。
pub fn oct_text(mode: u16) -> String {
    let mut s = String::new();
    let mut k = 3usize;
    while k > 0 {
        k -= 1;
        let shift = (k as u32) * 3;
        let d = ((mode >> shift) & 0o7) as u8;
        s.push((b'0' + d) as char);
    }
    s
}

/// 有符号十进制文本。
fn i32_text(v: i32) -> String {
    if v == 0 {
        return String::from("0");
    }
    if v < 0 {
        let mut s = String::from("-");
        s.push_str(dec_text(v.unsigned_abs()).as_str());
        s
    } else {
        dec_text(v as u32)
    }
}

/// u32 文本。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// PROT 三位文本（r/w/x 次序，缺位为 `-`）。
pub fn prot_text(p: u8) -> String {
    let mut s = String::new();
    s.push(if (p & PROT_READ) != 0 { 'r' } else { '-' });
    s.push(if (p & PROT_WRITE) != 0 { 'w' } else { '-' });
    s.push(if (p & PROT_EXEC) != 0 { 'x' } else { '-' });
    s
}

/// 段旗标三位文本（r/w/x 次序）。
pub fn flags_text(f: u8) -> String {
    let mut s = String::new();
    s.push(if (f & PF_R) != 0 { 'r' } else { '-' });
    s.push(if (f & PF_W) != 0 { 'w' } else { '-' });
    s.push(if (f & PF_X) != 0 { 'x' } else { '-' });
    s
}

// ---------------------------------------------------------------------------
// 权限位
// ---------------------------------------------------------------------------

/// 某一档的读位（class 0 owner / 1 group / 2 other）。
pub fn read_bit(mode: u16, class: u8) -> bool {
    let shift = match class {
        0 => 6u32,
        1 => 3u32,
        _ => 0u32,
    };
    ((mode >> shift) & 0o4) != 0
}

/// 某一档的写位。
pub fn write_bit(mode: u16, class: u8) -> bool {
    let shift = match class {
        0 => 6u32,
        1 => 3u32,
        _ => 0u32,
    };
    ((mode >> shift) & 0o2) != 0
}

/// 某一档的执行位。
pub fn exec_bit(mode: u16, class: u8) -> bool {
    let shift = match class {
        0 => 6u32,
        1 => 3u32,
        _ => 0u32,
    };
    ((mode >> shift) & 0o1) != 0
}

// ---------------------------------------------------------------------------
// 段与镜像
// ---------------------------------------------------------------------------

/// 一段 PT_LOAD。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seg {
    /// p_flags。
    pub flags: u8,
}

impl Seg {
    /// 构造。
    pub const fn new(flags: u8) -> Seg {
        Seg { flags: flags & PF_ALL }
    }

    /// 可读。
    pub fn readable(&self) -> bool {
        (self.flags & PF_R) != 0
    }

    /// 可写。
    pub fn writable(&self) -> bool {
        (self.flags & PF_W) != 0
    }

    /// 可执行。
    pub fn executable(&self) -> bool {
        (self.flags & PF_X) != 0
    }

    /// W^X 冲突。
    pub fn wx(&self) -> bool {
        self.writable() && self.executable()
    }
}

/// 段表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Image {
    /// 段。
    pub segs: [Seg; MAX_SEGS],
    /// 有效段数。
    pub count: usize,
    /// 是否带 GNU_RELRO。
    pub relro: bool,
}

impl Image {
    /// 空镜像。
    pub const fn none() -> Image {
        Image {
            segs: [Seg::new(0), Seg::new(0), Seg::new(0)],
            count: 0,
            relro: false,
        }
    }

    /// 单段镜像。
    pub const fn one(flags: u8) -> Image {
        Image {
            segs: [Seg::new(flags), Seg::new(0), Seg::new(0)],
            count: 1,
            relro: false,
        }
    }

    /// 双段镜像。
    pub const fn two(a: u8, b: u8) -> Image {
        Image {
            segs: [Seg::new(a), Seg::new(b), Seg::new(0)],
            count: 2,
            relro: false,
        }
    }

    /// 单段 + RELRO。
    pub const fn relro(flags: u8) -> Image {
        Image {
            segs: [Seg::new(flags), Seg::new(0), Seg::new(0)],
            count: 1,
            relro: true,
        }
    }

    /// 取一段。
    pub fn seg(&self, i: usize) -> Option<Seg> {
        if i >= self.count {
            return None;
        }
        self.segs.get(i).copied()
    }

    /// 段旗标并集（按 PROT 位表示）。
    pub fn union(&self) -> u8 {
        let mut p = 0u8;
        let mut i = 0usize;
        while i < self.count {
            if let Some(s) = self.seg(i) {
                if s.readable() {
                    p |= PROT_READ;
                }
                if s.writable() {
                    p |= PROT_WRITE;
                }
                if s.executable() {
                    p |= PROT_EXEC;
                }
            }
            i += 1;
        }
        p
    }

    /// 首段 W^X。
    pub fn wx(&self) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if let Some(s) = self.seg(i) {
                if s.wx() {
                    return true;
                }
            }
            i += 1;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// 环境
// ---------------------------------------------------------------------------

/// 装载环境。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Env {
    /// 文件权限位。
    pub mode: u16,
    /// 调用者档位（0/1/2）。
    pub class: u8,
    /// 是否 MAP_SHARED。
    pub shared: bool,
    /// 是否 noexec 挂载点。
    pub noexec: bool,
}

impl Env {
    /// 构造。
    pub const fn new(mode: u16, class: u8, shared: bool, noexec: bool) -> Env {
        Env { mode: mode & 0o777, class: class % 3, shared, noexec }
    }

    /// 文件是否可读。
    pub fn readable(&self) -> bool {
        read_bit(self.mode, self.class)
    }
}

// ---------------------------------------------------------------------------
// 阶段
// ---------------------------------------------------------------------------

/// 装载阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 打开。
    Open,
    /// 保护。
    Protect,
    /// 映射。
    Map,
    /// 重定位/RELRO。
    Reloc,
    /// 就绪。
    Ready,
}

impl Stage {
    /// 次序。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Open => 0,
            Stage::Protect => 1,
            Stage::Map => 2,
            Stage::Reloc => 3,
            Stage::Ready => 4,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Open => "open",
            Stage::Protect => "protect",
            Stage::Map => "map",
            Stage::Reloc => "reloc",
            Stage::Ready => "ready",
        }
    }
}

// ---------------------------------------------------------------------------
// 注解
// ---------------------------------------------------------------------------

/// 本次装载的注解。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 纯只读。
    Plain,
    /// 私有可写（写时复制）。
    PrivateCopy,
    /// RELRO 收写。
    Relro,
    /// 无读位拒载。
    DeniedRead,
    /// 共享写被拒。
    SharedWrite,
    /// noexec 拒执行映射。
    NoExec,
    /// W^X 冲突。
    Wx,
    /// 无 PT_LOAD。
    Empty,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::Plain => "plain",
            Note::PrivateCopy => "private-copy",
            Note::Relro => "relro",
            Note::DeniedRead => "denied-read",
            Note::SharedWrite => "shared-write",
            Note::NoExec => "no-exec",
            Note::Wx => "wx",
            Note::Empty => "empty",
        }
    }
}

// ---------------------------------------------------------------------------
// 结果
// ---------------------------------------------------------------------------

/// 一次装载计划的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 死在（或走到）的阶段。
    pub stage: Stage,
    /// errno（成功为 0）。
    pub code: i32,
    /// 期望 PROT（段旗标并集）。
    pub want: u8,
    /// 实际 PROT（成功时的最终值；失败为 0）。
    pub prot: u8,
    /// 注解。
    pub note: Note,
}

impl Outcome {
    /// 是否成功。
    pub fn ok(&self) -> bool {
        self.stage == Stage::Ready && self.code == 0
    }

    /// 失败。
    pub fn failed(&self) -> bool {
        !self.ok()
    }

    /// 期望 PROT 文本。
    pub fn want_text(&self) -> String {
        prot_text(self.want)
    }

    /// 实际 PROT 文本。
    pub fn prot_text(&self) -> String {
        prot_text(self.prot)
    }
}

/// 装载计划：读位 → 段表 → 并集 → W^X → 共享写 → noexec → RELRO。
///
/// 次序不可倒：无读位时连段表都不该读（否则会把"打不开"报成"段非法"）；
/// W^X 必须在共享写之前判（同一段的两种拒绝理由里 W^X 是结构性的、与
/// 映射方式无关）；RELRO 收写必须放在所有拒绝之后（它只作用于成功路径）。
pub fn plan(env: Env, image: &Image) -> Outcome {
    let want = image.union();
    if !env.readable() {
        return Outcome {
            stage: Stage::Open,
            code: ERR_EACCES,
            want,
            prot: 0,
            note: Note::DeniedRead,
        };
    }
    if image.count == 0 {
        return Outcome {
            stage: Stage::Protect,
            code: ERR_ENOEXEC,
            want: 0,
            prot: 0,
            note: Note::Empty,
        };
    }
    if image.wx() {
        return Outcome {
            stage: Stage::Protect,
            code: ERR_EINVAL,
            want,
            prot: 0,
            note: Note::Wx,
        };
    }
    if (want & PROT_WRITE) != 0 && env.shared {
        return Outcome {
            stage: Stage::Map,
            code: ERR_EACCES,
            want,
            prot: 0,
            note: Note::SharedWrite,
        };
    }
    if (want & PROT_EXEC) != 0 && env.noexec {
        return Outcome {
            stage: Stage::Map,
            code: ERR_EACCES,
            want,
            prot: 0,
            note: Note::NoExec,
        };
    }
    if image.relro && (want & PROT_WRITE) != 0 {
        return Outcome {
            stage: Stage::Ready,
            code: 0,
            want,
            prot: want & !PROT_WRITE,
            note: Note::Relro,
        };
    }
    if (want & PROT_WRITE) != 0 {
        return Outcome {
            stage: Stage::Ready,
            code: 0,
            want,
            prot: want,
            note: Note::PrivateCopy,
        };
    }
    Outcome { stage: Stage::Ready, code: 0, want, prot: want, note: Note::Plain }
}

// ---------------------------------------------------------------------------
// 档位
// ---------------------------------------------------------------------------

/// 判据档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 拒绝档。
    Denied,
    /// 写档。
    Write,
    /// 执行档。
    Exec,
    /// 纯读档。
    Plain,
}

impl Tier {
    /// 由结果归位：拒绝优先，其次按期望 PROT 的 W/X 位。
    pub fn of(o: &Outcome) -> Tier {
        if o.note == Note::DeniedRead {
            return Tier::Denied;
        }
        if (o.want & PROT_WRITE) != 0 {
            return Tier::Write;
        }
        if (o.want & PROT_EXEC) != 0 {
            return Tier::Exec;
        }
        Tier::Plain
    }

    /// 下标。
    pub fn index(&self) -> usize {
        match self {
            Tier::Denied => 0,
            Tier::Write => 1,
            Tier::Exec => 2,
            Tier::Plain => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Denied => "denied",
            Tier::Write => "write",
            Tier::Exec => "exec",
            Tier::Plain => "plain",
        }
    }
}

// ---------------------------------------------------------------------------
// 缺陷
// ---------------------------------------------------------------------------

/// 申报与实际比对得到的缺陷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 一致。
    Ok,
    /// 阶段错。
    Stage,
    /// errno 错。
    Code,
    /// 实际 PROT 错。
    Prot,
    /// 注解错。
    Note,
    /// 成功/失败互报。
    Estate,
    /// 计数错。
    Count,
    /// 观测值错。
    Value,
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
            Fault::Stage => EA_ESTAGE,
            Fault::Code => EA_ECODE,
            Fault::Prot => EA_EPROT,
            Fault::Note => EA_ENOTE,
            Fault::Estate => EA_ESTATE,
            Fault::Count => EA_ECOUNT,
            Fault::Value => EA_EVALUE,
            Fault::Slot => EA_ESLOT,
            Fault::Seal => EA_ESEAL,
            Fault::Nobs => EA_ENOBS,
            Fault::Matrix => EA_EMATRIX,
            Fault::Order => EA_EORDER,
            Fault::Bal => EA_EBAL,
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
        Fault::Stage => "stage",
        Fault::Code => "code",
        Fault::Prot => "prot",
        Fault::Note => "note",
        Fault::Estate => "estate",
        Fault::Count => "count",
        Fault::Value => "value",
        Fault::Slot => "slot",
        Fault::Seal => "seal",
        Fault::Nobs => "nobs",
        Fault::Matrix => "matrix",
        Fault::Order => "order",
        Fault::Bal => "bal",
    }
}

// ---------------------------------------------------------------------------
// 申报与语料
// ---------------------------------------------------------------------------

/// 一条申报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 申报阶段。
    pub stage: Stage,
    /// 申报 errno。
    pub code: i32,
    /// 申报实际 PROT。
    pub prot: u8,
    /// 申报注解。
    pub note: Note,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim { stage: Stage::Open, code: 0, prot: 0, note: Note::Plain }
    }

    /// 是否申报成功。
    pub fn success(&self) -> bool {
        self.stage == Stage::Ready && self.code == 0
    }
}

/// 一个语料形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// 请求名。
    pub name: &'static str,
    /// 环境。
    pub env: Env,
    /// 镜像。
    pub image: Image,
}

impl Shape {
    /// 空形状。
    pub const fn blank() -> Shape {
        Shape { tag: "", name: "", env: Env::new(0, 0, false, false), image: Image::none() }
    }

    /// 实际结果。
    pub fn actual(&self) -> Outcome {
        plan(self.env, &self.image)
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

    /// 请求名。
    pub fn name(&self) -> &'static str {
        self.shape.name
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

/// 14 个形状。前 8 个是"可正确申报"的形状，后 6 个复刻其中六个形状。
pub const SHAPES: [Shape; CORPUS] = [
    Shape {
        tag: "ro-read",
        name: "./ro.so",
        env: Env::new(0o444, 0, false, false),
        image: Image::one(PF_R),
    },
    Shape {
        tag: "private-write",
        name: "./priv.so",
        env: Env::new(0o444, 0, false, false),
        image: Image::one(PF_R | PF_W),
    },
    Shape {
        tag: "exec-seg",
        name: "./exec.so",
        env: Env::new(0o555, 0, false, false),
        image: Image::one(PF_R | PF_X),
    },
    Shape {
        tag: "locked-read",
        name: "./locked.so",
        env: Env::new(0o000, 0, false, false),
        image: Image::one(PF_R),
    },
    Shape {
        tag: "shared-write",
        name: "./shared.so",
        env: Env::new(0o644, 0, true, false),
        image: Image::one(PF_R | PF_W),
    },
    Shape {
        tag: "noexec-exec",
        name: "./nx.so",
        env: Env::new(0o644, 0, false, true),
        image: Image::one(PF_R | PF_X),
    },
    Shape {
        tag: "wx-seg",
        name: "./wx.so",
        env: Env::new(0o644, 0, false, false),
        image: Image::one(PF_R | PF_W | PF_X),
    },
    Shape {
        tag: "relro-write",
        name: "./relro.so",
        env: Env::new(0o755, 0, false, false),
        image: Image::relro(PF_R | PF_W),
    },
    Shape {
        tag: "denied-as-ready",
        name: "./locked.so",
        env: Env::new(0o000, 0, false, false),
        image: Image::one(PF_R),
    },
    Shape {
        tag: "wx-as-eacces",
        name: "./wx.so",
        env: Env::new(0o644, 0, false, false),
        image: Image::one(PF_R | PF_W | PF_X),
    },
    Shape {
        tag: "shared-as-protect",
        name: "./shared.so",
        env: Env::new(0o644, 0, true, false),
        image: Image::one(PF_R | PF_W),
    },
    Shape {
        tag: "nx-as-shared",
        name: "./nx.so",
        env: Env::new(0o644, 0, false, true),
        image: Image::one(PF_R | PF_X),
    },
    Shape {
        tag: "relro-as-full",
        name: "./relro.so",
        env: Env::new(0o755, 0, false, false),
        image: Image::relro(PF_R | PF_W),
    },
    Shape {
        tag: "private-as-plain",
        name: "./priv.so",
        env: Env::new(0o444, 0, false, false),
        image: Image::one(PF_R | PF_W),
    },
];

/// 与 `SHAPES` 一一对应的申报。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim { stage: Stage::Ready, code: 0, prot: PROT_READ, note: Note::Plain },
    Claim { stage: Stage::Ready, code: 0, prot: PROT_READ | PROT_WRITE, note: Note::PrivateCopy },
    Claim { stage: Stage::Ready, code: 0, prot: PROT_READ | PROT_EXEC, note: Note::Plain },
    Claim { stage: Stage::Open, code: ERR_EACCES, prot: 0, note: Note::DeniedRead },
    Claim { stage: Stage::Map, code: ERR_EACCES, prot: 0, note: Note::SharedWrite },
    Claim { stage: Stage::Map, code: ERR_EACCES, prot: 0, note: Note::NoExec },
    Claim { stage: Stage::Protect, code: ERR_EINVAL, prot: 0, note: Note::Wx },
    Claim { stage: Stage::Ready, code: 0, prot: PROT_READ, note: Note::Relro },
    Claim { stage: Stage::Ready, code: 0, prot: 0, note: Note::Plain },
    Claim { stage: Stage::Protect, code: ERR_EACCES, prot: 0, note: Note::Wx },
    Claim { stage: Stage::Protect, code: ERR_EACCES, prot: 0, note: Note::SharedWrite },
    Claim { stage: Stage::Map, code: ERR_EACCES, prot: 0, note: Note::SharedWrite },
    Claim { stage: Stage::Ready, code: 0, prot: PROT_READ | PROT_WRITE, note: Note::Relro },
    Claim { stage: Stage::Ready, code: 0, prot: PROT_READ | PROT_WRITE, note: Note::Plain },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Code, Fault::Stage, Fault::Note, Fault::Prot, Fault::Note];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] =
    [EA_ESTATE, EA_ECODE, EA_ESTAGE, EA_ENOTE, EA_EPROT, EA_ENOTE];

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
        Err(EA_ENOBS)
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

/// 对判：状态 → 阶段 → errno → PROT → 注解。
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
    if claim.prot != a.prot {
        return Fault::Prot;
    }
    if claim.note != a.note {
        return Fault::Note;
    }
    Fault::Ok
}

// ---------------------------------------------------------------------------
// 释义
// ---------------------------------------------------------------------------

/// 单行释义：
/// `<name> mode=<oct> class=<c> want=<r/w/x> prot=<r/w/x> stage=<s> note=<n>`。
pub fn detail(c: &Case) -> String {
    let a = c.actual();
    let mut s = String::from(a_name(c));
    s.push_str(" mode=");
    s.push_str(oct_text(c.shape.env.mode).as_str());
    s.push_str(" class=");
    s.push_str(dec_text(c.shape.env.class as u32).as_str());
    s.push_str(" want=");
    s.push_str(a.want_text().as_str());
    s.push_str(" prot=");
    s.push_str(a.prot_text().as_str());
    s.push_str(" stage=");
    s.push_str(a.stage.name());
    s.push_str(" note=");
    s.push_str(a.note.name());
    s.push_str("\n");
    s
}

/// 名字（占位以保持 `detail` 单出口）。
fn a_name(c: &Case) -> &'static str {
    c.shape.name
}

// ---------------------------------------------------------------------------
// 矩阵
// ---------------------------------------------------------------------------

/// 一行统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// 该档槽数。
    pub total: usize,
    /// 该档申报正确数。
    pub good: usize,
}

impl Row {
    /// 全对。
    pub fn perfect(&self) -> bool {
        self.total > 0 && self.total == self.good
    }

    /// 空白行。
    pub const fn blank() -> Row {
        Row { total: 0, good: 0 }
    }
}

/// 四档矩阵。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行（denied/write/exec/plain）。
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
                let t = Tier::of(&c.actual()).index();
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
#[derive(Clone, Copy, Debug)]
pub struct Report {
    /// 总槽数。
    pub total: u32,
    /// 申报正确数。
    pub accept: u32,
    /// 申报错误数。
    pub reject: u32,
    /// 拒绝档正确数。
    pub denied: u32,
    /// 写档正确数。
    pub write: u32,
    /// 执行档正确数。
    pub exec: u32,
    /// 纯读档正确数。
    pub plain: u32,
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
    pub fn green(&self) -> bool {
        self.total == CORPUS as u32 && self.matched == self.total
    }

    /// 闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.green() {
            Ok(())
        } else {
            Err(EA_ECOUNT)
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("prot corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" denied=");
        s.push_str(u32_text(self.denied).as_str());
        s.push_str(" write=");
        s.push_str(u32_text(self.write).as_str());
        s.push_str(" exec=");
        s.push_str(u32_text(self.exec).as_str());
        s.push_str(" plain=");
        s.push_str(u32_text(self.plain).as_str());
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
            return Err(EA_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut denied = 0u32;
    let mut write = 0u32;
    let mut exec = 0u32;
    let mut plain = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(EA_ENOBS),
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
            None => return Err(EA_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(&a) {
                Tier::Denied => denied += 1,
                Tier::Write => write += 1,
                Tier::Exec => exec += 1,
                Tier::Plain => plain += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, denied, write, exec, plain, matched })
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门。
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
            return Err(EA_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(EA_EORDER);
        }
        self.last = o;
        self.steps += 1;
        Ok(())
    }

    /// 关闭。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(EA_ESTATE);
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
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            root: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EA_ESLOT);
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

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(EA_EBAL);
        }
        const VALS: [u32; ACCEPT] = [14, 8, 6, 1, 4, 2, 1, 929];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(EA_EBAL),
            };
            if s.slot as usize != i {
                return Err(EA_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(EA_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(EA_EVALUE);
                        }
                    }
                    None => return Err(EA_EBAL),
                }
                vi += 1;
            } else {
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EA_EVALUE);
                        }
                    }
                    None => return Err(EA_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(EA_EBAL);
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
            return Err(EA_EBAL);
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
            Err(EA_ESEAL)
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
    let _ = l.register(count_cell(3, OP_OPEN, r.map(|x| x.denied)));
    let _ = l.register(count_cell(4, OP_PROT, r.map(|x| x.write)));
    let _ = l.register(count_cell(5, OP_MAP, r.map(|x| x.exec)));
    let _ = l.register(count_cell(6, OP_SEG, r.map(|x| x.plain)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_UNION, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_RELRO,
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
    out[0] = name_ok() && perm_ok() && image_ok();
    out[1] = plan_ok() && judge_ok() && matrix_ok();
    out[2] = corpus_ok() && reds_ok() && detail_ok() && slots_ok();
    out[3] = gate_ok()
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

/// 名称与段基址。
fn name_ok() -> bool {
    let codes = [
        EA_ESTAGE, EA_ECODE, EA_EPROT, EA_ENOTE, EA_ESTATE, EA_ECOUNT, EA_EVALUE, EA_ESLOT,
        EA_ESEAL, EA_ENOBS, EA_EMATRIX, EA_EORDER, EA_EBAL,
    ];
    let mut i = 0usize;
    while i < codes.len() {
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    let a = SEG_BASE == 0xfcb0;
    let b = EA_ESTAGE == SEG_BASE + 0x01 && EA_EBAL == SEG_BASE + 0x0d;
    let c = SAMPLES == 14 && CORPUS == 14 && ACCEPT == 8 && REJECT == 6;
    let d = PROT_READ == 1 && PROT_WRITE == 2 && PROT_EXEC == 4 && PROT_ALL == 7;
    let e = PF_X == 1 && PF_W == 2 && PF_R == 4 && PF_ALL == 7;
    a && b && c && d && e
}

/// 权限位与档位。
fn perm_ok() -> bool {
    let a = read_bit(0o444, 0) && read_bit(0o444, 1) && read_bit(0o444, 2);
    let b = !read_bit(0o000, 0) && !read_bit(0o000, 1) && !read_bit(0o000, 2);
    let c = read_bit(0o040, 1) && !read_bit(0o040, 0) && !read_bit(0o040, 2);
    let d = read_bit(0o004, 2) && !read_bit(0o004, 1) && write_bit(0o020, 1);
    let e = exec_bit(0o100, 0) && exec_bit(0o010, 1) && exec_bit(0o001, 2);
    let f = oct_text(0o444).as_str() == "444" && oct_text(0o000).as_str() == "000";
    let g = Env::new(0o444, 0, false, false).readable();
    let h = !Env::new(0o000, 0, false, false).readable();
    let i = Env::new(0o040, 1, false, false).readable();
    let j = !Env::new(0o040, 0, false, false).readable();
    a && b && c && d && e && f && g && h && i && j
}

/// 段与镜像。
fn image_ok() -> bool {
    let a = Image::one(PF_R).union() == PROT_READ;
    let b = Image::one(PF_R | PF_W).union() == (PROT_READ | PROT_WRITE);
    let c = Image::one(PF_R | PF_X).union() == (PROT_READ | PROT_EXEC);
    let d = Image::two(PF_R, PF_R | PF_W).union() == (PROT_READ | PROT_WRITE);
    let e = Image::none().union() == 0 && Image::none().count == 0;
    let f = Image::one(PF_R | PF_W | PF_X).wx();
    let g = !Image::two(PF_R, PF_R | PF_X).wx();
    let h = Image::relro(PF_R | PF_W).relro;
    let i = match Image::two(PF_R, PF_R | PF_X).seg(1) {
        Some(s) => s.executable() && s.readable() && !s.writable(),
        None => false,
    };
    let j = Image::one(PF_R).seg(1).is_none();
    let k = flags_text(PF_R | PF_W | PF_X).as_str() == "rwx"
        && flags_text(PF_R).as_str() == "r--"
        && flags_text(PF_X).as_str() == "--x";
    let l = prot_text(PROT_READ | PROT_EXEC).as_str() == "r-x"
        && prot_text(0).as_str() == "---";
    a && b && c && d && e && f && g && h && i && j && k && l
}

/// 装载计划五条线。
fn plan_ok() -> bool {
    let ro = plan(Env::new(0o444, 0, false, false), &Image::one(PF_R));
    let a = ro.ok() && ro.prot == PROT_READ && ro.note == Note::Plain && ro.stage == Stage::Ready;
    let pw = plan(Env::new(0o444, 0, false, false), &Image::one(PF_R | PF_W));
    let b = pw.ok()
        && pw.prot == (PROT_READ | PROT_WRITE)
        && pw.note == Note::PrivateCopy
        && pw.want == (PROT_READ | PROT_WRITE);
    let lk = plan(Env::new(0o000, 0, false, false), &Image::one(PF_R));
    let c = lk.failed()
        && lk.stage == Stage::Open
        && lk.code == ERR_EACCES
        && lk.note == Note::DeniedRead;
    let sh = plan(Env::new(0o644, 0, true, false), &Image::one(PF_R | PF_W));
    let d = sh.stage == Stage::Map && sh.code == ERR_EACCES && sh.note == Note::SharedWrite;
    let nx = plan(Env::new(0o644, 0, false, true), &Image::one(PF_R | PF_X));
    let e = nx.stage == Stage::Map && nx.code == ERR_EACCES && nx.note == Note::NoExec;
    let wx = plan(Env::new(0o644, 0, false, false), &Image::one(PF_R | PF_W | PF_X));
    let f = wx.stage == Stage::Protect && wx.code == ERR_EINVAL && wx.note == Note::Wx;
    let rl = plan(Env::new(0o755, 0, false, false), &Image::relro(PF_R | PF_W));
    let g = rl.ok() && rl.want == (PROT_READ | PROT_WRITE) && rl.prot == PROT_READ;
    let g2 = rl.note == Note::Relro;
    let em = plan(Env::new(0o644, 0, false, false), &Image::none());
    let h = em.stage == Stage::Protect && em.code == ERR_ENOEXEC && em.note == Note::Empty;
    let den = plan(Env::new(0o000, 0, false, false), &Image::none());
    let i = den.stage == Stage::Open && den.note == Note::DeniedRead;
    a && b && c && d && e && f && g && g2 && h && i
}

/// 申报对判。
fn judge_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < ACCEPT {
        if let Some(c) = cs.get(i) {
            if judge(c) != Fault::Ok {
                return false;
            }
        } else {
            return false;
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
        if judge(&c) != want {
            return false;
        }
        if judge(&c).code() != code {
            return false;
        }
        j += 1;
    }
    true
}

/// 矩阵。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    m.row(0) == Some(Row { total: 2, good: 1 })
        && m.row(1) == Some(Row { total: 8, good: 4 })
        && m.row(2) == Some(Row { total: 3, good: 2 })
        && m.row(3) == Some(Row { total: 1, good: 1 })
        && m.row(4).is_none()
        && m.total() == CORPUS
        && m.good() == ACCEPT
        && m.balanced()
        && m.perfect_rows() == 1
        && m.seal() == Matrix::of().seal()
        && m.seal() != Matrix::blank().seal()
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

/// 逐槽注解合规。
fn notes_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        if a.ok() != (a.note == Note::Plain || a.note == Note::PrivateCopy || a.note == Note::Relro)
        {
            return false;
        }
        if a.ok() && (a.prot & !a.want) != 0 {
            return false;
        }
        if !a.ok() && a.prot != 0 {
            return false;
        }
        if detail(&c).as_str().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 逐槽对判表。
fn table_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let f = judge(&c);
        if i < ACCEPT {
            if f != Fault::Ok {
                return false;
            }
        } else if f == Fault::Ok {
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
    let b = r.denied == 1 && r.write == 4 && r.exec == 2 && r.plain == 1;
    let c = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    let d = tags_unique() && notes_ok() && table_ok();
    a && b && c && d
}

/// 红档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == 13 && r.rate() == 929;
    let b = r.gate() == Err(EA_ECOUNT) && !r.green();
    let c = run_with(Some((CORPUS, Fault::Code))).err() == Some(EA_ESLOT);
    a && b && c
}

/// 释义逐字。
const DETAIL_RO: &str =
    "./ro.so mode=444 class=0 want=r-- prot=r-- stage=ready note=plain\n";
/// 拒绝档释义。
const DETAIL_DENIED: &str =
    "./locked.so mode=000 class=0 want=r-- prot=--- stage=open note=denied-read\n";
/// RELRO 档释义。
const DETAIL_RELRO: &str =
    "./relro.so mode=755 class=0 want=rw- prot=r-- stage=ready note=relro\n";

/// 释义。
fn detail_ok() -> bool {
    let cs = cases();
    let a = match cs.get(0) {
        Some(c) => detail(c).as_str() == DETAIL_RO,
        None => false,
    };
    let b = match cs.get(3) {
        Some(c) => detail(c).as_str() == DETAIL_DENIED,
        None => false,
    };
    let d = match cs.get(7) {
        Some(c) => detail(c).as_str() == DETAIL_RELRO,
        None => false,
    };
    let e = match cs.get(4) {
        Some(c) => {
            let o = c.actual();
            o.want_text().as_str() == "rw-" && o.prot_text().as_str() == "---"
        }
        None => false,
    };
    a && b && d && e
}

/// 槽号与档位。
fn slots_ok() -> bool {
    let a = match case_at(0) {
        Some(c) => c.tag() == "ro-read" && c.name() == "./ro.so",
        None => false,
    };
    let b = match case_at(13) {
        Some(c) => c.tag() == "private-as-plain",
        None => false,
    };
    let c = case_at(CORPUS).is_none();
    let d = cover(CORPUS) == Ok(()) && cover(1) == Err(EA_ENOBS);
    let e = Stage::Open.order() == 0
        && Stage::Protect.order() == 1
        && Stage::Map.order() == 2
        && Stage::Reloc.order() == 3
        && Stage::Ready.order() == 4;
    let f = Stage::Protect.name() == "protect"
        && Stage::Reloc.name() == "reloc"
        && Stage::Ready.name() == "ready";
    let g = Tier::Denied.index() == 0
        && Tier::Write.index() == 1
        && Tier::Exec.index() == 2
        && Tier::Plain.index() == 3;
    let h = Tier::Denied.name() == "denied"
        && Tier::Write.name() == "write"
        && Tier::Exec.name() == "exec"
        && Tier::Plain.name() == "plain";
    let i = fault_short(Fault::Prot) == "prot"
        && fault_short(Fault::Note) == "note"
        && fault_short(Fault::Ok) == "ok";
    let j = Note::PrivateCopy.name() == "private-copy"
        && Note::DeniedRead.name() == "denied-read"
        && Note::NoExec.name() == "no-exec"
        && Note::SharedWrite.name() == "shared-write";
    a && b && c && d && e && f && g && h && i && j
}

/// 阶段闸门。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    let a = g.steps() == 0 && !g.closed() && g.last() == -1;
    let b = g.step(Stage::Protect) == Ok(()) && g.steps() == 1;
    let c = g.step(Stage::Open) == Err(EA_EORDER) && g.steps() == 1;
    let d = g.step(Stage::Map) == Ok(()) && g.last() == 2 && g.steps() == 2;
    let e = g.close() == Ok(2) && g.closed();
    let f = g.close() == Err(EA_ESTATE);
    let h = g.step(Stage::Ready) == Err(EA_ESTATE);
    a && b && c && d && e && f && h
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
pub fn run_unx11634_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11634_prot_linkage");

    s.add("PR01-名称与段基址", name_ok(), "static");

    s.add("PR02-权限位与档位", perm_ok(), "static");

    s.add("PR03-段表与并集", image_ok(), "static");

    s.add("PR04-装载计划五线", plan_ok(), "static");

    s.add("PR05-申报对判", judge_ok(), "static");

    s.add("PR06-矩阵统计", matrix_ok(), "static");

    s.add("PR07-语料与渲染", corpus_ok(), "static");

    s.add("PR08-红档显式", reds_ok(), "static");

    s.add("PR09-释义逐字", detail_ok(), "static");

    s.add("PR10-槽号与档位", slots_ok(), "static");

    s.add("PR11-阶段闸门", gate_ok(), "static");

    s.add("PR12-账本对账", ledger_ok(), "static");

    s.add(
        "PR13-冻结重放四注入",
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
        panic!("unx11634 test fail")
    }

    #[test]
    fn prot_consts() {
        assert_eq!(PROT_READ, 1);
        assert_eq!(PROT_WRITE, 2);
        assert_eq!(PROT_EXEC, 4);
        assert_eq!(PROT_ALL, 7);
        assert_eq!(PF_R, 4);
        assert_eq!(PF_W, 2);
        assert_eq!(PF_X, 1);
    }

    #[test]
    fn prot_texts() {
        assert_eq!(prot_text(PROT_READ).as_str(), "r--");
        assert_eq!(prot_text(PROT_READ | PROT_WRITE).as_str(), "rw-");
        assert_eq!(prot_text(PROT_READ | PROT_EXEC).as_str(), "r-x");
        assert_eq!(prot_text(PROT_ALL).as_str(), "rwx");
        assert_eq!(prot_text(0).as_str(), "---");
        assert_eq!(flags_text(PF_R | PF_W | PF_X).as_str(), "rwx");
    }

    #[test]
    fn read_bits() {
        assert!(read_bit(0o400, 0));
        assert!(!read_bit(0o400, 1));
        assert!(read_bit(0o040, 1));
        assert!(read_bit(0o004, 2));
        assert!(!read_bit(0o200, 0));
    }

    #[test]
    fn write_bits() {
        assert!(write_bit(0o200, 0));
        assert!(write_bit(0o020, 1));
        assert!(write_bit(0o002, 2));
        assert!(!write_bit(0o400, 0));
    }

    #[test]
    fn exec_bits() {
        assert!(exec_bit(0o100, 0));
        assert!(exec_bit(0o010, 1));
        assert!(exec_bit(0o001, 2));
        assert!(!exec_bit(0o400, 0));
    }

    #[test]
    fn oct_texts() {
        assert_eq!(oct_text(0o444).as_str(), "444");
        assert_eq!(oct_text(0o755).as_str(), "755");
        assert_eq!(oct_text(0).as_str(), "000");
    }

    #[test]
    fn seg_flags() {
        let s = Seg::new(PF_R | PF_W);
        assert!(s.readable());
        assert!(s.writable());
        assert!(!s.executable());
        assert!(!s.wx());
        assert!(Seg::new(PF_R | PF_W | PF_X).wx());
    }

    #[test]
    fn image_union() {
        assert_eq!(Image::none().union(), 0);
        assert_eq!(Image::one(PF_R).union(), PROT_READ);
        assert_eq!(Image::one(PF_R | PF_W).union(), PROT_READ | PROT_WRITE);
        assert_eq!(Image::two(PF_R, PF_R | PF_X).union(), PROT_READ | PROT_EXEC);
        assert_eq!(Image::relro(PF_R | PF_W).union(), PROT_READ | PROT_WRITE);
    }

    #[test]
    fn image_wx() {
        assert!(Image::one(PF_R | PF_W | PF_X).wx());
        assert!(!Image::two(PF_R, PF_R | PF_W).wx());
        assert!(!Image::none().wx());
    }

    #[test]
    fn image_count() {
        assert_eq!(Image::none().count, 0);
        assert_eq!(Image::one(PF_R).count, 1);
        assert_eq!(Image::two(PF_R, PF_W).count, 2);
        assert!(Image::one(PF_R).seg(1).is_none());
    }

    #[test]
    fn env_readable() {
        assert!(Env::new(0o444, 0, false, false).readable());
        assert!(Env::new(0o040, 1, false, false).readable());
        assert!(!Env::new(0o040, 0, false, false).readable());
        assert!(!Env::new(0o000, 2, false, false).readable());
    }

    #[test]
    fn plan_read_only() {
        let o = plan(Env::new(0o444, 0, false, false), &Image::one(PF_R));
        assert!(o.ok());
        assert_eq!(o.prot, PROT_READ);
        assert_eq!(o.want, PROT_READ);
        assert_eq!(o.note, Note::Plain);
    }

    #[test]
    fn plan_private_copy() {
        let o = plan(Env::new(0o444, 0, false, false), &Image::one(PF_R | PF_W));
        assert!(o.ok());
        assert_eq!(o.prot, PROT_READ | PROT_WRITE);
        assert_eq!(o.note, Note::PrivateCopy);
    }

    #[test]
    fn plan_exec() {
        let o = plan(Env::new(0o555, 0, false, false), &Image::one(PF_R | PF_X));
        assert!(o.ok());
        assert_eq!(o.prot, PROT_READ | PROT_EXEC);
        assert_eq!(o.note, Note::Plain);
    }

    #[test]
    fn plan_denied_read() {
        let o = plan(Env::new(0o000, 0, false, false), &Image::one(PF_R));
        assert!(o.failed());
        assert_eq!(o.stage, Stage::Open);
        assert_eq!(o.code, ERR_EACCES);
        assert_eq!(o.note, Note::DeniedRead);
        assert_eq!(o.prot, 0);
    }

    #[test]
    fn plan_shared_write() {
        let o = plan(Env::new(0o644, 0, true, false), &Image::one(PF_R | PF_W));
        assert_eq!(o.stage, Stage::Map);
        assert_eq!(o.code, ERR_EACCES);
        assert_eq!(o.note, Note::SharedWrite);
        assert_eq!(o.want, PROT_READ | PROT_WRITE);
    }

    #[test]
    fn plan_noexec() {
        let o = plan(Env::new(0o644, 0, false, true), &Image::one(PF_R | PF_X));
        assert_eq!(o.stage, Stage::Map);
        assert_eq!(o.code, ERR_EACCES);
        assert_eq!(o.note, Note::NoExec);
    }

    #[test]
    fn plan_wx() {
        let o = plan(Env::new(0o644, 0, false, false), &Image::one(PF_R | PF_W | PF_X));
        assert_eq!(o.stage, Stage::Protect);
        assert_eq!(o.code, ERR_EINVAL);
        assert_eq!(o.note, Note::Wx);
        assert_eq!(o.want, PROT_ALL);
    }

    #[test]
    fn plan_relro() {
        let o = plan(Env::new(0o755, 0, false, false), &Image::relro(PF_R | PF_W));
        assert!(o.ok());
        assert_eq!(o.want, PROT_READ | PROT_WRITE);
        assert_eq!(o.prot, PROT_READ);
        assert_eq!(o.note, Note::Relro);
    }

    #[test]
    fn plan_empty() {
        let o = plan(Env::new(0o644, 0, false, false), &Image::none());
        assert_eq!(o.stage, Stage::Protect);
        assert_eq!(o.code, ERR_ENOEXEC);
        assert_eq!(o.note, Note::Empty);
    }

    #[test]
    fn plan_open_before_empty() {
        let o = plan(Env::new(0o000, 0, false, false), &Image::none());
        assert_eq!(o.stage, Stage::Open);
        assert_eq!(o.note, Note::DeniedRead);
    }

    #[test]
    fn plan_wx_before_shared() {
        let o = plan(Env::new(0o644, 0, true, false), &Image::one(PF_R | PF_W | PF_X));
        assert_eq!(o.stage, Stage::Protect);
        assert_eq!(o.note, Note::Wx);
    }

    #[test]
    fn shape_table() {
        assert_eq!(SHAPES.len(), CORPUS);
        assert_eq!(CLAIMS.len(), CORPUS);
        assert!(tags_unique());
        assert_eq!(SHAPES[0].tag, "ro-read");
        assert_eq!(SHAPES[13].tag, "private-as-plain");
    }

    #[test]
    fn judge_accepts() {
        let cs = cases();
        let mut i = 0usize;
        while i < ACCEPT {
            assert_eq!(judge(&cs[i]), Fault::Ok);
            i += 1;
        }
    }

    #[test]
    fn judge_rejects() {
        let cs = cases();
        let mut i = 0usize;
        while i < REJECT {
            let c = &cs[i + ACCEPT];
            assert_eq!(judge(c), REJECT_FAULTS[i]);
            assert_eq!(judge(c).code(), REJECT_CODES[i]);
            i += 1;
        }
    }

    #[test]
    fn judge_estate() {
        let c = &cases()[8];
        assert_eq!(c.actual().stage, Stage::Open);
        assert_eq!(judge(c), Fault::Estate);
    }

    #[test]
    fn judge_prot() {
        let c = &cases()[12];
        let a = c.actual();
        assert_eq!(a.prot, PROT_READ);
        assert_eq!(a.note, Note::Relro);
        assert_eq!(judge(c), Fault::Prot);
    }

    #[test]
    fn judge_note() {
        let c = &cases()[13];
        let a = c.actual();
        assert_eq!(a.note, Note::PrivateCopy);
        assert_eq!(judge(c), Fault::Note);
    }

    #[test]
    fn matrix_rows() {
        let m = Matrix::of();
        assert_eq!(m.total(), 14);
        assert_eq!(m.good(), 8);
        assert!(m.balanced());
        assert_eq!(m.row(0), Some(Row { total: 2, good: 1 }));
        assert_eq!(m.row(1), Some(Row { total: 8, good: 4 }));
        assert_eq!(m.row(2), Some(Row { total: 3, good: 2 }));
        assert_eq!(m.row(3), Some(Row { total: 1, good: 1 }));
        assert_eq!(m.perfect_rows(), 1);
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
        assert_eq!(r.denied, 1);
        assert_eq!(r.write, 4);
        assert_eq!(r.exec, 2);
        assert_eq!(r.plain, 1);
        assert_eq!(r.matched, 14);
        assert_eq!(r.rate(), 1000);
    }

    #[test]
    fn report_render_exact() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(
            r.render().as_str(),
            "prot corpus=14 accept=8 reject=6 denied=1 write=4 exec=2 plain=1 match=14 rate=1000ppm\n"
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
        assert_eq!(r.gate(), Err(EA_ECOUNT));
        assert_eq!(run_with(Some((14, Fault::Code))).err(), Some(EA_ESLOT));
    }

    #[test]
    fn detail_exact() {
        let cs = cases();
        assert_eq!(detail(&cs[0]).as_str(), DETAIL_RO);
        assert_eq!(detail(&cs[3]).as_str(), DETAIL_DENIED);
        assert_eq!(detail(&cs[7]).as_str(), DETAIL_RELRO);
    }

    #[test]
    fn gate_steps() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Protect), Ok(()));
        assert_eq!(g.step(Stage::Open), Err(EA_EORDER));
        assert_eq!(g.steps(), 1);
        assert_eq!(g.step(Stage::Map), Ok(()));
        assert_eq!(g.last(), 2);
    }

    #[test]
    fn gate_close() {
        let mut g = Gate::new();
        assert!(g.close().is_ok());
        assert_eq!(g.close(), Err(EA_ESTATE));
        assert_eq!(g.step(Stage::Ready), Err(EA_ESTATE));
    }

    #[test]
    fn slot_bounds() {
        assert!(case_at(0).is_some());
        assert!(case_at(CORPUS - 1).is_some());
        assert!(case_at(CORPUS).is_none());
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(1), Err(EA_ENOBS));
        assert_eq!(fault_short(Fault::Ok), "ok");
        assert_eq!(fault_short(Fault::Order), "order");
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
        assert_eq!(l.register(Sample::blank()).err(), Some(EA_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(EA_EBAL));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn injections() {
        let out = inject_four();
        assert!(out[0], "inject0");
        assert!(out[1], "inject1");
        assert!(out[2], "inject2");
        assert!(out[3], "inject3");
    }

    #[test]
    fn text_helpers() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(hex_text(0xfcb0, 4).as_str(), "fcb0");
        assert_eq!(i32_text(-22).as_str(), "-22");
        assert_eq!(i32_text(13).as_str(), "13");
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11634_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
