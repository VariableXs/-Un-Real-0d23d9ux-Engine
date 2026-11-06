//! UNX-F11681 ENOSPC 总锚（write 返回与 errno）判据
//!
//! ENOSPC 是一条纵线的总锚：`write()` 在设备满时返回 `-1` 且 `errno == ENOSPC`
//! （28，strerror 文本 `No space left on device`）。锚是**成对的**——只看返回值
//! 或只看 errno 都不算命中：返回 `-1` 但 errno 未被置成 28，是"红了但没对上
//! 暗号"；返回 `n == count` 但 errno 仍是上一次调用残留的 28，是"绿了但口袋
//! 里揣着红"。本卡把这对 (返回, errno) 作为唯一总锚，向上给 cp/dd（F11682、
//! F11683）供数，与 C3 的码档同族联签：码档说的是同一个 28，不是两个 28。
//!
//! 三条必须分立的失败形态：
//!
//! 1. **短写（short write）**：`0 < free < count` 时内核写进能写下的部分，
//!    返回 `n < count`（成功态），这是"还剩一点"的正常记账，不得判死。
//! 2. **满失败**：`free == 0` 且 `count > 0`，返回 `-1`、errno=28，是本卡
//!    的正锚样本。
//! 3. **延迟提交（deferred allocation）**：写时只进页缓存返回 `count`，真正
//!    的 ENOSPC 迟到 `fsync`/`close` 才爆——`write` 绿、`fsync` 红。判据必须
//!    承认"当场绿"合法、"事后红"归属同族锚，而不是把两者混成一条。
//!
//! 邻居 errno 不得混档：EFBIG（27，`File too large`，撞 RLIMIT_FSIZE/offset
//! 上限）、EDQUOT（122，`Quota exceeded`，配额满）、EBADF（9，`Bad file
//! descriptor`，fd 本身不合法）、EIO（5，`Input/output error`，提交期设备错）
//! 全都是"红"，但都不是 ENOSPC——把它们当 ENOSPC 记账，上层 cp/dd 的部分写
//! 计数与诊断文案就全错位。
//!
//! 闸门次序（为什么不能倒）：
//!
//! `fd → limit → quota → space → commit → pair → done`
//!
//! 1. `fd`：描述符本身不合法时一切空间判断无意义，最先短路。
//! 2. `limit`：文件尺寸上限先于空间检查（GNU 语义下 EFBIG 在配额之前判）。
//! 3. `quota`：配额是"人的额度"，先于"介质的余量"判，EDQUOT 不冒充 ENOSPC。
//! 4. `space`：即时分配型文件系统的满/短写判定在这里发生。
//! 5. `commit`：延迟分配型文件系统的 ENOSPC 在 fsync/close 提交时爆。
//! 6. `pair`：拿物理结果与申报观测的 errno 对照——返回与 errno 必须成对成立，
//!    这一闸就是"总锚"本身的守闸。
//! 7. `done`：以上全过，(返回, errno) 对干净落账。
//!
//! 关键差异（本卡深度所在）：
//!
//! * 与 F11682（cp 卡）的边界：本卡只判**系统调用面**的返回/errno 配对，
//!   不判退出码、不判部分件留档；cp 的"中止+码 1+诊断"由 F11682 判，但它
//!   的 ENOSPC 依据必须能在本卡找到同一条锚。
//! * 与 F11683（dd 卡）的边界：dd 的半块账、records 计数归 F11683；本卡
//!   对 dd 只供给"写失败那一刻的 (返回, errno)"。
//! * 与邻居 errno 卡的边界：EFBIG/EDQUOT/EBADF/EIO 在本卡只作为**分档**
//!   出现——判死它们"不是 ENOSPC"是本卡的排他职责，不是判它们的对错。
//! * 短写不是失败：`0 < free < count` 返回 `free` 是成功态；把短写判成
//!   ENOSPC 或把 ENOSPC 说成短写，都要被本卡的红档显式抓住。
//!
//! 判据六维：结束阶段（gate）、errno（code）、返回字节（value）、形态注解
//! （note）、锚位（default：(返回, errno) 是否构成 ENOSPC 总锚）、成败申报
//! （estate）。判序固定：状态 → 阶段 → errno → 返回值 → 注解 → 锚位。
//!
//! 语料 14 槽：8 槽申报正确（full 2、short 1、anchor 2、foreign 3），6 槽
//! 复刻同一满失败现场、各挂一维错报（estate/stage/code/value/note/default）。
//! 账本 14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码，冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11681）。
pub const SEG_BASE: u32 = 0x6d130;

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

// ---------------------------------------------------------------------------
// errno（POSIX/Linux 真值）
// ---------------------------------------------------------------------------

/// 设备无空间（strerror: `No space left on device`）。
pub const ERR_ENOSPC: i32 = 28;
/// 文件超过上限（strerror: `File too large`）。
pub const ERR_EFBIG: i32 = 27;
/// 配额超限（strerror: `Quota exceeded`）。
pub const ERR_EDQUOT: i32 = 122;
/// 描述符不合法（strerror: `Bad file descriptor`）。
pub const ERR_EBADF: i32 = 9;
/// 提交期设备错（strerror: `Input/output error`）。
pub const ERR_EIO: i32 = 5;
/// 成功时 errno 应停放的零值。
pub const ERR_NONE: i32 = 0;

/// 单次写入观测上限（模型内 count/free 的合法上界）。
pub const COUNT_MAX: u32 = 0xffff_ffff;

/// 合法 errno 集合的大小（0、28、27、122、9、5 六个）。
pub const ERRNO_SET_SIZE: usize = 6;

// ---------------------------------------------------------------------------
// 错误码（13 个，SEG_BASE + 0x01..0x0d）
// ---------------------------------------------------------------------------

/// 阶段申报不符。
pub const EN_ESTAGE: u32 = SEG_BASE + 0x01;
/// errno 申报不符。
pub const EN_ECODE: u32 = SEG_BASE + 0x02;
/// 返回字节申报不符。
pub const EN_EVALUE: u32 = SEG_BASE + 0x03;
/// 注解申报不符。
pub const EN_ENOTE: u32 = SEG_BASE + 0x04;
/// 锚位申报不符。
pub const EN_EDEFAULT: u32 = SEG_BASE + 0x05;
/// 返回/errno 配对破断。
pub const EN_EPAIR: u32 = SEG_BASE + 0x06;
/// 成败互报。
pub const EN_ESTATE: u32 = SEG_BASE + 0x07;
/// 计数不符。
pub const EN_ECOUNT: u32 = SEG_BASE + 0x08;
/// 槽位非法。
pub const EN_ESLOT: u32 = SEG_BASE + 0x09;
/// 指纹不符。
pub const EN_ESEAL: u32 = SEG_BASE + 0x0a;
/// 无观测。
pub const EN_ENOBS: u32 = SEG_BASE + 0x0b;
/// 矩阵不平。
pub const EN_EMATRIX: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const EN_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码（账本格的操作归类）
// ---------------------------------------------------------------------------

/// write 系统调用。
pub const OP_WRITE: u8 = 0x01;
/// 尺寸上限闸。
pub const OP_LIMIT: u8 = 0x02;
/// 配额闸。
pub const OP_QUOTA: u8 = 0x03;
/// 空间闸。
pub const OP_SPACE: u8 = 0x04;
/// 提交闸。
pub const OP_COMMIT: u8 = 0x05;
/// 配对闸。
pub const OP_PAIR: u8 = 0x06;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x07;
/// 账本重放。
pub const OP_REPLAY: u8 = 0x08;

// ---------------------------------------------------------------------------
// FNV-1a 指纹
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

/// 折入一个字符串。
fn fold_str(h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut v = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            v = fold_byte(v, *b);
        }
        i += 1;
    }
    v
}

// ---------------------------------------------------------------------------
// 数字/布尔文本
// ---------------------------------------------------------------------------

/// 十进制文本（u32，不递归）。
fn dec_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    while n > 0 && i > 0 {
        i -= 1;
        if let Some(b) = buf.get_mut(i) {
            *b = b'0' + ((n % 10) as u8);
        }
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

/// bool 文本。
fn bool_text(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

// ---------------------------------------------------------------------------
// 文案（真实 strerror / shell / coreutils 表面逐字）
// ---------------------------------------------------------------------------

/// strerror(errno) 的 glibc 原文（本卡涉及的六个值）。
pub fn strerror(code: i32) -> &'static str {
    match code {
        ERR_ENOSPC => "No space left on device",
        ERR_EFBIG => "File too large",
        ERR_EDQUOT => "Quota exceeded",
        ERR_EBADF => "Bad file descriptor",
        ERR_EIO => "Input/output error",
        ERR_NONE => "",
        _ => "Unknown error",
    }
}

/// `tee` 落到设备满时的 GNU 原文形态。
pub fn msg_tee_enospc(path: &str) -> String {
    let mut s = String::from("tee: ");
    s.push_str(path);
    s.push_str(": No space left on device");
    s
}

/// bash 内建 echo 写失败的原文形态。
pub fn msg_bash_write_err() -> String {
    String::from("bash: echo: write error: No space left on device")
}

/// 短写记账文案（本卡自研审计文本）：实际吞下的字节小于请求。
pub fn msg_short_write(written: u32, want: u32) -> String {
    let mut s = String::from("short write: accepted ");
    s.push_str(dec_text(written).as_str());
    s.push_str(" of ");
    s.push_str(dec_text(want).as_str());
    s.push_str(" requested bytes");
    s
}

/// 返回/errno 配对破断的审计文案。
pub fn msg_pair_broken(ret: u32, errno: i32) -> String {
    let mut s = String::from("write/errno pair broken (ret=");
    s.push_str(dec_text(ret).as_str());
    s.push_str(", errno=");
    s.push_str(i32_text(errno).as_str());
    s.push(')');
    s
}

/// 满失败的现场文案（工具在路径上报告 ENOSPC 的通用形态）。
pub fn msg_enospc_at(path: &str) -> String {
    let mut s = String::from("write '");
    s.push_str(path);
    s.push_str("': No space left on device");
    s
}

/// 延迟提交爆红的现场文案：fsync/close 才见的 ENOSPC。
pub fn msg_commit_enospc(op: &str, path: &str) -> String {
    let mut s = String::from(op);
    s.push_str(" '");
    s.push_str(path);
    s.push_str("': No space left on device");
    s
}

/// 邻居 errno 的现场文案（EFBIG/EDQUOT/EBADF 等，非 ENOSPC）。
pub fn msg_foreign_at(op: &str, path: &str, code: i32) -> String {
    let mut s = String::from(op);
    s.push_str(" '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

// ---------------------------------------------------------------------------
// 枚举
// ---------------------------------------------------------------------------

/// 系统调用形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    /// `write(fd, buf, count)`。
    Write,
    /// `fsync(fd)`：延迟分配的 ENOSPC 在这里爆。
    Fsync,
    /// `close(fd)`：无 fsync 时提交错误迟到在 close（Linux 会把它带回来）。
    Close,
}

impl OpKind {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            OpKind::Write => "write",
            OpKind::Fsync => "fsync",
            OpKind::Close => "close",
        }
    }

    /// 是否即时返回空间的调用（只有 write 有短写/满写之分）。
    pub fn instant(&self) -> bool {
        match self {
            OpKind::Write => true,
            OpKind::Fsync | OpKind::Close => false,
        }
    }

    /// 是否提交类调用（延迟分配在这两段见到真相）。
    pub fn commit(&self) -> bool {
        match self {
            OpKind::Fsync | OpKind::Close => true,
            OpKind::Write => false,
        }
    }
}

/// 闸门阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 描述符合法性。
    Fd,
    /// 文件尺寸上限（EFBIG）。
    Limit,
    /// 用户/组配额（EDQUOT）。
    Quota,
    /// 即时空间判定（ENOSPC/短写）。
    Space,
    /// 提交期判定（延迟分配的 ENOSPC/EIO）。
    Commit,
    /// 返回/errno 配对判定（总锚守闸）。
    Pair,
    /// 全闸通过。
    Done,
}

impl Stage {
    /// 次序（纵线不可倒）。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Fd => 0,
            Stage::Limit => 1,
            Stage::Quota => 2,
            Stage::Space => 3,
            Stage::Commit => 4,
            Stage::Pair => 5,
            Stage::Done => 6,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Fd => "fd",
            Stage::Limit => "limit",
            Stage::Quota => "quota",
            Stage::Space => "space",
            Stage::Commit => "commit",
            Stage::Pair => "pair",
            Stage::Done => "done",
        }
    }

    /// 是否失败点（Done 之外都是某道闸的短路）。
    pub fn failure(&self) -> bool {
        match self {
            Stage::Done => false,
            _ => true,
        }
    }

    /// 是否空间族闸门（space/commit 两段的红才是 ENOSPC 候选）。
    pub fn space_family(&self) -> bool {
        match self {
            Stage::Space | Stage::Commit => true,
            _ => false,
        }
    }
}

/// 结果注解（现场形态的分类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 全量写入成功（返回 == count）。
    Written,
    /// 短写成功（0 < 返回 < count）。
    Short,
    /// 延迟分配下 write 当场放行。
    DeferredPass,
    /// 零字节写（POSIX 允许直接返回 0）。
    EmptyWrite,
    /// 即时满失败：-1 + ENOSPC。
    NospcWrite,
    /// 提交满失败：fsync -1 + ENOSPC。
    NospcFsync,
    /// 提交满失败：close -1 + ENOSPC。
    NospcClose,
    /// 邻居：尺寸上限 EFBIG。
    Efbig,
    /// 邻居：配额 EDQUOT。
    Edquot,
    /// 邻居：坏描述符 EBADF。
    Ebadf,
    /// 邻居：提交期设备错 EIO。
    EioCommit,
    /// 配对破断：返回与 errno 不成对。
    PairBroken,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::Written => "written",
            Note::Short => "short",
            Note::DeferredPass => "deferred-pass",
            Note::EmptyWrite => "empty-write",
            Note::NospcWrite => "nospc-write",
            Note::NospcFsync => "nospc-fsync",
            Note::NospcClose => "nospc-close",
            Note::Efbig => "efbig",
            Note::Edquot => "edquot",
            Note::Ebadf => "ebadf",
            Note::EioCommit => "eio-commit",
            Note::PairBroken => "pair-broken",
        }
    }

    /// 是否成功态注解。
    pub fn success(&self) -> bool {
        match self {
            Note::Written | Note::Short | Note::DeferredPass | Note::EmptyWrite => true,
            _ => false,
        }
    }

    /// 是否 ENOSPC 族注解（三兄弟）。
    pub fn enospc_family(&self) -> bool {
        match self {
            Note::NospcWrite | Note::NospcFsync | Note::NospcClose => true,
            _ => false,
        }
    }

    /// 是否邻居注解（红但不是 ENOSPC）。
    pub fn foreign(&self) -> bool {
        match self {
            Note::Efbig | Note::Edquot | Note::Ebadf | Note::EioCommit => true,
            _ => false,
        }
    }

    /// 是否短写注解（成功但少于请求——不得与满失败混计）。
    pub fn partial_success(&self) -> bool {
        match self {
            Note::Short => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 文件系统几何（结构闸的载体）
// ---------------------------------------------------------------------------

/// 一块被测文件系统的几何模型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FsGeom {
    /// 设备/挂载点名。
    pub name: &'static str,
    /// 块大小（字节）。
    pub bsize: u32,
    /// 空闲块数。
    pub blocks_free: u32,
    /// 是否延迟分配型（write 当场放行、提交时爆）。
    pub delalloc: bool,
    /// 硬配额（字节）。
    pub quota_hard: u32,
}

impl FsGeom {
    /// 空白几何。
    pub const fn blank() -> FsGeom {
        FsGeom {
            name: "",
            bsize: 0,
            blocks_free: 0,
            delalloc: false,
            quota_hard: 0,
        }
    }

    /// 空闲字节数（乘法溢出即视为坏几何，返回 None）。
    pub fn bytes_free(&self) -> Option<u32> {
        self.bsize.checked_mul(self.blocks_free)
    }

    /// 是否结构良好：块大小在 {512,1024,4096}、空闲字节可算、配额非零。
    pub fn well_formed(&self) -> bool {
        if self.name.is_empty() {
            return false;
        }
        if self.bsize != 512 && self.bsize != 1024 && self.bsize != 4096 {
            return false;
        }
        if self.bytes_free().is_none() {
            return false;
        }
        self.quota_hard > 0 && self.quota_hard < COUNT_MAX
    }

    /// 是否满到零块（即时型满失败的前提）。
    pub fn is_full(&self) -> bool {
        self.blocks_free == 0
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.name);
        h = fold_word(h, self.bsize as u64);
        h = fold_word(h, self.blocks_free as u64);
        h = fold_word(h, self.delalloc as u64);
        h = fold_word(h, self.quota_hard as u64);
        match self.bytes_free() {
            Some(v) => fold_word(h, v as u64),
            None => fold_word(h, 0),
        }
    }
}

/// 被测文件系统表：即时型半满、即时型满、延迟分配型满。
pub fn geoms() -> [FsGeom; 3] {
    [
        FsGeom {
            name: "dev/root",
            bsize: 4096,
            blocks_free: 1,
            delalloc: false,
            quota_hard: 1048576,
        },
        FsGeom {
            name: "dev/loop0",
            bsize: 1024,
            blocks_free: 0,
            delalloc: false,
            quota_hard: 1048576,
        },
        FsGeom {
            name: "dev/delalloc",
            bsize: 512,
            blocks_free: 0,
            delalloc: true,
            quota_hard: 1048576,
        },
    ]
}

// ---------------------------------------------------------------------------
// 形状（本卡输入的全部维度）
// ---------------------------------------------------------------------------

/// 一次观测的形状：设备状态 + 调用形态 + 观测到的 errno。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// 目标路径名。
    pub path: &'static str,
    /// 调用形态。
    pub op: OpKind,
    /// 请求写入字节数（write 用）。
    pub count: u32,
    /// 设备空闲字节数。
    pub free: u32,
    /// 描述符是否合法。
    pub fd_ok: bool,
    /// 是否撞文件尺寸上限。
    pub size_limit: bool,
    /// 是否撞配额。
    pub quota: bool,
    /// 文件系统是否延迟分配。
    pub deferred: bool,
    /// 提交期设备报的 errno（0 表示提交干净）。
    pub commit_err: i32,
    /// 调用方观测到的 errno（配对闸的对照物）。
    pub reported: i32,
}

impl Shape {
    /// 默认形状：合法 fd、即时分配、干净观测。
    pub const fn blank() -> Shape {
        Shape {
            tag: "",
            path: "out.txt",
            op: OpKind::Write,
            count: 0,
            free: 0,
            fd_ok: true,
            size_limit: false,
            quota: false,
            deferred: false,
            commit_err: ERR_NONE,
            reported: ERR_NONE,
        }
    }

    /// 换请求字节数的副本（谓词把同一形状推到不同档位复算用）。
    pub const fn clone_count(&self, c: u32) -> Shape {
        Shape {
            tag: self.tag,
            path: self.path,
            op: self.op,
            count: c,
            free: self.free,
            fd_ok: self.fd_ok,
            size_limit: self.size_limit,
            quota: self.quota,
            deferred: self.deferred,
            commit_err: self.commit_err,
            reported: self.reported,
        }
    }

    /// 换空闲字节的副本。
    pub const fn clone_free(&self, f: u32) -> Shape {
        Shape {
            tag: self.tag,
            path: self.path,
            op: self.op,
            count: self.count,
            free: f,
            fd_ok: self.fd_ok,
            size_limit: self.size_limit,
            quota: self.quota,
            deferred: self.deferred,
            commit_err: self.commit_err,
            reported: self.reported,
        }
    }

    /// 观测 errno 是否在合法集合内（0/28/27/122/9/5）。
    pub fn errno_legal(&self) -> bool {
        self.reported == ERR_NONE
            || self.reported == ERR_ENOSPC
            || self.reported == ERR_EFBIG
            || self.reported == ERR_EDQUOT
            || self.reported == ERR_EBADF
            || self.reported == ERR_EIO
    }

    /// 提交 errno 是否在合法集合内（0/28/5——提交期只有这两种红）。
    pub fn commit_legal(&self) -> bool {
        self.commit_err == ERR_NONE || self.commit_err == ERR_ENOSPC || self.commit_err == ERR_EIO
    }

    /// 形状是否合法可判：fd 与空间数值的组合自洽。
    pub fn valid(&self) -> bool {
        if !self.errno_legal() {
            return false;
        }
        if !self.commit_legal() {
            return false;
        }
        if self.op.commit() && self.count != 0 {
            // fsync/close 没有"请求字节"维度，count 必须空置。
            return false;
        }
        if self.op == OpKind::Write && self.count == 0 && self.free > COUNT_MAX {
            return false;
        }
        true
    }

    /// 是否 write 形态。
    pub fn is_write(&self) -> bool {
        self.op == OpKind::Write
    }

    /// 指纹：对全部判定字段敏感。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_str(h, self.path);
        h = fold_str(h, self.op.text());
        h = fold_word(h, self.count as u64);
        h = fold_word(h, self.free as u64);
        h = fold_word(h, self.fd_ok as u64);
        h = fold_word(h, self.size_limit as u64);
        h = fold_word(h, self.quota as u64);
        h = fold_word(h, self.deferred as u64);
        h = fold_word(h, self.commit_err as i64 as u64);
        fold_word(h, self.reported as i64 as u64)
    }
}

/// 物理中间结果（空间/提交闸算出的真相，配对闸之前）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Phys {
    /// 结束阶段。
    stage: Stage,
    /// 物理 errno。
    errno: i32,
    /// 物理返回字节。
    value: u32,
    /// 形态注解。
    note: Note,
}

impl Phys {
    const fn ok(stage: Stage, value: u32, note: Note) -> Phys {
        Phys { stage, errno: ERR_NONE, value, note }
    }

    const fn fail(stage: Stage, errno: i32, note: Note) -> Phys {
        Phys { stage, errno, value: 0, note }
    }
}

// ---------------------------------------------------------------------------
// 结果
// ---------------------------------------------------------------------------

/// 一次判定的结果（六维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 结束阶段。
    pub stage: Stage,
    /// errno（0 表示成功）。
    pub code: i32,
    /// 返回字节（失败一律 0）。
    pub value: u32,
    /// 形态注解。
    pub note: Note,
    /// 锚位：(返回, errno) 是否构成 ENOSPC 总锚。
    pub default: bool,
}

impl Outcome {
    /// 空白结果。
    pub const fn blank() -> Outcome {
        Outcome { stage: Stage::Done, code: 0, value: 0, note: Note::Written, default: false }
    }

    /// 是否成功。
    pub fn ok(&self) -> bool {
        self.stage == Stage::Done && self.code == 0 && self.note.success()
    }

    /// 是否失败。
    pub fn failed(&self) -> bool {
        !self.ok()
    }

    /// 是否锚位命中：`-1` 且 errno==28 且形态属 ENOSPC 族。
    pub fn anchored(&self) -> bool {
        self.failed() && self.code == ERR_ENOSPC && self.note.enospc_family() && self.default
    }

    /// 由物理结果升成判定结果（配对闸之后）。
    const fn from_phys(p: &Phys, default: bool) -> Outcome {
        Outcome { stage: p.stage, code: p.errno, value: p.value, note: p.note, default }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.order() as u64);
        h = fold_word(h, self.code as i64 as u64);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        fold_word(h, self.default as u64)
    }
}

// ---------------------------------------------------------------------------
// 定义纵线（闸门次序：fd → limit → quota → space → commit → pair → done）
// ---------------------------------------------------------------------------

/// 判定一次写观测的现场结果。
///
/// 每一闸只在上一闸放行后执行；空间族结果还要过配对闸——返回与 errno
/// 不成对（`-1` 而观测 errno 非 28，或成功而观测 errno 残存 28）一律改判
/// `Pair` 段破断，锚位随之归零。这是"总锚"作为守闸的最后一段。
pub fn define(s: &Shape) -> Outcome {
    // ---- fd ----
    if !s.fd_ok {
        return Outcome::from_phys(&Phys::fail(Stage::Fd, ERR_EBADF, Note::Ebadf), false);
    }
    // ---- limit ----
    if s.size_limit {
        return Outcome::from_phys(&Phys::fail(Stage::Limit, ERR_EFBIG, Note::Efbig), false);
    }
    // ---- quota ----
    if s.quota {
        return Outcome::from_phys(&Phys::fail(Stage::Quota, ERR_EDQUOT, Note::Edquot), false);
    }

    // ---- space / commit ----
    let phys = match s.op {
        OpKind::Write => {
            if s.count == 0 {
                // 零字节写：POSIX 允许直接 0 返回，满盘也不报错。
                Phys::ok(Stage::Done, 0, Note::EmptyWrite)
            } else if s.deferred {
                // 延迟分配：写只进缓存，当场放行 count 字节。
                Phys::ok(Stage::Done, s.count, Note::DeferredPass)
            } else if s.free == 0 {
                Phys::fail(Stage::Space, ERR_ENOSPC, Note::NospcWrite)
            } else if s.free < s.count {
                Phys::ok(Stage::Done, s.free, Note::Short)
            } else {
                Phys::ok(Stage::Done, s.count, Note::Written)
            }
        }
        OpKind::Fsync | OpKind::Close => {
            if s.commit_err == ERR_ENOSPC {
                let note = if s.op == OpKind::Fsync { Note::NospcFsync } else { Note::NospcClose };
                Phys::fail(Stage::Commit, ERR_ENOSPC, note)
            } else if s.commit_err != ERR_NONE {
                Phys::fail(Stage::Commit, s.commit_err, Note::EioCommit)
            } else {
                Phys::ok(Stage::Done, 0, Note::Written)
            }
        }
    };

    // ---- pair ----
    // 观测 errno 与物理 errno 不一致：返回/errno 这对暗号没对上。
    if s.reported != phys.errno {
        return Outcome {
            stage: Stage::Pair,
            code: s.reported,
            value: phys.value,
            note: Note::PairBroken,
            default: false,
        };
    }

    // ---- done ----
    let anchor = phys.note.enospc_family();
    Outcome::from_phys(&phys, anchor)
}

/// 现场文案（note → 真实文本；成功注解空串）。
pub fn message(s: &Shape, o: &Outcome) -> String {
    match o.note {
        Note::NospcWrite => msg_enospc_at(s.path),
        Note::NospcFsync => msg_commit_enospc("fsync", s.path),
        Note::NospcClose => msg_commit_enospc("close", s.path),
        Note::Efbig => msg_foreign_at("write", s.path, ERR_EFBIG),
        Note::Edquot => msg_foreign_at("write", s.path, ERR_EDQUOT),
        Note::Ebadf => msg_foreign_at("write", s.path, ERR_EBADF),
        Note::EioCommit => msg_commit_err(s.op.text(), s.path, o.code),
        Note::PairBroken => msg_pair_broken(o.value, o.code),
        Note::Short => String::new(),
        Note::Written | Note::DeferredPass | Note::EmptyWrite => String::new(),
    }
}

/// 提交期设备错文案。
pub fn msg_commit_err(op: &str, path: &str, code: i32) -> String {
    msg_foreign_at(op, path, code)
}

// ---------------------------------------------------------------------------
// 档位
// ---------------------------------------------------------------------------

/// 结果档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 成功且足额（written / deferred-pass / empty-write）。
    Full,
    /// 成功但短写（返回小于请求）。
    Short,
    /// 失败且非 ENOSPC 锚（邻居与配对破断）。
    Foreign,
    /// 失败且命中 ENOSPC 总锚。
    Anchor,
}

impl Tier {
    /// 由结果投影。
    pub fn of(o: &Outcome) -> Tier {
        if o.ok() {
            match o.note {
                Note::Short => Tier::Short,
                Note::Written | Note::DeferredPass | Note::EmptyWrite => Tier::Full,
                _ => Tier::Foreign,
            }
        } else if o.note.enospc_family() {
            Tier::Anchor
        } else {
            Tier::Foreign
        }
    }

    /// 次序。
    pub fn index(&self) -> usize {
        match self {
            Tier::Full => 0,
            Tier::Short => 1,
            Tier::Foreign => 2,
            Tier::Anchor => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Full => "full",
            Tier::Short => "short",
            Tier::Foreign => "foreign",
            Tier::Anchor => "anchor",
        }
    }

    /// 是否成功档。
    pub fn success(&self) -> bool {
        match self {
            Tier::Full | Tier::Short => true,
            Tier::Foreign | Tier::Anchor => false,
        }
    }

    /// 是否 ENOSPC 族档。
    pub fn enospc(&self) -> bool {
        match self {
            Tier::Anchor => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 缺陷
// ---------------------------------------------------------------------------

/// 申报缺陷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 一致。
    Ok,
    /// 阶段错。
    Stage,
    /// errno 错。
    Code,
    /// 返回字节错。
    Value,
    /// 注解错。
    Note,
    /// 锚位错。
    Default,
    /// 配对破断（结构维）。
    Pair,
    /// 成败互报。
    Estate,
    /// 计数错。
    Count,
    /// 槽位错。
    Slot,
    /// 指纹错。
    Seal,
    /// 无观测。
    Nobs,
    /// 矩阵不平。
    Matrix,
    /// 账本不平。
    Bal,
}

impl Fault {
    /// 错误码。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Stage => EN_ESTAGE,
            Fault::Code => EN_ECODE,
            Fault::Value => EN_EVALUE,
            Fault::Note => EN_ENOTE,
            Fault::Default => EN_EDEFAULT,
            Fault::Pair => EN_EPAIR,
            Fault::Estate => EN_ESTATE,
            Fault::Count => EN_ECOUNT,
            Fault::Slot => EN_ESLOT,
            Fault::Seal => EN_ESEAL,
            Fault::Nobs => EN_ENOBS,
            Fault::Matrix => EN_EMATRIX,
            Fault::Bal => EN_EBAL,
        }
    }

    /// 是否非零。
    pub fn alive(&self) -> bool {
        !matches!(self, Fault::Ok)
    }
}

/// 短名。
pub fn fault_short(f: Fault) -> &'static str {
    match f {
        Fault::Ok => "ok",
        Fault::Stage => "stage",
        Fault::Code => "code",
        Fault::Value => "value",
        Fault::Note => "note",
        Fault::Default => "default",
        Fault::Pair => "pair",
        Fault::Estate => "estate",
        Fault::Count => "count",
        Fault::Slot => "slot",
        Fault::Seal => "seal",
        Fault::Nobs => "nobs",
        Fault::Matrix => "matrix",
        Fault::Bal => "bal",
    }
}

// ---------------------------------------------------------------------------
// 申报与用例
// ---------------------------------------------------------------------------

/// 一槽申报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 结束阶段。
    pub stage: Stage,
    /// errno。
    pub code: i32,
    /// 返回字节。
    pub value: u32,
    /// 形态注解。
    pub note: Note,
    /// 锚位。
    pub default: bool,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim { stage: Stage::Done, code: 0, value: 0, note: Note::Written, default: false }
    }

    /// 是否申报成功（errno 归零且注解属成功态）。
    pub fn success(&self) -> bool {
        self.code == 0 && self.note.success()
    }
}

/// 一槽用例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 形状。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
}

impl Case {
    /// 空白用例。
    pub const fn blank() -> Case {
        Case { shape: Shape::blank(), claim: Claim::blank() }
    }

    /// 标签。
    pub fn tag(&self) -> &'static str {
        self.shape.tag
    }

    /// 实际结果。
    pub fn actual(&self) -> Outcome {
        define(&self.shape)
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
// 语料（14 槽：8 活 + 6 死）
// ---------------------------------------------------------------------------

/// 8 个可正确申报的形状 + 6 个复刻形状（复刻自 enospc-immediate）。
pub const SHAPES: [Shape; CORPUS] = [
    Shape {
        tag: "write-full-ok",
        path: "data.bin",
        op: OpKind::Write,
        count: 4096,
        free: 4096,
        fd_ok: true,
        size_limit: false,
        quota: false,
        deferred: false,
        commit_err: ERR_NONE,
        reported: ERR_NONE,
    },
    Shape {
        tag: "short-write",
        path: "data.bin",
        op: OpKind::Write,
        count: 4096,
        free: 1024,
        fd_ok: true,
        size_limit: false,
        quota: false,
        deferred: false,
        commit_err: ERR_NONE,
        reported: ERR_NONE,
    },
    Shape {
        tag: "delalloc-pass",
        path: "data.bin",
        op: OpKind::Write,
        count: 4096,
        free: 0,
        fd_ok: true,
        size_limit: false,
        quota: false,
        deferred: true,
        commit_err: ERR_NONE,
        reported: ERR_NONE,
    },
    Shape {
        tag: "enospc-immediate",
        path: "data.bin",
        op: OpKind::Write,
        count: 4096,
        free: 0,
        fd_ok: true,
        size_limit: false,
        quota: false,
        deferred: false,
        commit_err: ERR_NONE,
        reported: ERR_ENOSPC,
    },
    Shape {
        tag: "enospc-fsync",
        path: "log.txt",
        op: OpKind::Fsync,
        count: 0,
        free: 0,
        fd_ok: true,
        size_limit: false,
        quota: false,
        deferred: true,
        commit_err: ERR_ENOSPC,
        reported: ERR_ENOSPC,
    },
    Shape {
        tag: "efbig-limit",
        path: "big.bin",
        op: OpKind::Write,
        count: 4096,
        free: 4096,
        fd_ok: true,
        size_limit: true,
        quota: false,
        deferred: false,
        commit_err: ERR_NONE,
        reported: ERR_EFBIG,
    },
    Shape {
        tag: "edquot-quota",
        path: "home.bin",
        op: OpKind::Write,
        count: 1024,
        free: 4096,
        fd_ok: true,
        size_limit: false,
        quota: true,
        deferred: false,
        commit_err: ERR_NONE,
        reported: ERR_EDQUOT,
    },
    Shape {
        tag: "stale-pair-errno",
        path: "cache.bin",
        op: OpKind::Write,
        count: 4096,
        free: 4096,
        fd_ok: true,
        size_limit: false,
        quota: false,
        deferred: false,
        commit_err: ERR_NONE,
        reported: ERR_ENOSPC,
    },
    Shape {
        tag: "estate-swap",
        path: "data.bin",
        op: OpKind::Write,
        count: 4096,
        free: 0,
        fd_ok: true,
        size_limit: false,
        quota: false,
        deferred: false,
        commit_err: ERR_NONE,
        reported: ERR_ENOSPC,
    },
    Shape {
        tag: "stage-swap",
        path: "data.bin",
        op: OpKind::Write,
        count: 4096,
        free: 0,
        fd_ok: true,
        size_limit: false,
        quota: false,
        deferred: false,
        commit_err: ERR_NONE,
        reported: ERR_ENOSPC,
    },
    Shape {
        tag: "code-swap",
        path: "data.bin",
        op: OpKind::Write,
        count: 4096,
        free: 0,
        fd_ok: true,
        size_limit: false,
        quota: false,
        deferred: false,
        commit_err: ERR_NONE,
        reported: ERR_ENOSPC,
    },
    Shape {
        tag: "value-swap",
        path: "data.bin",
        op: OpKind::Write,
        count: 4096,
        free: 0,
        fd_ok: true,
        size_limit: false,
        quota: false,
        deferred: false,
        commit_err: ERR_NONE,
        reported: ERR_ENOSPC,
    },
    Shape {
        tag: "note-swap",
        path: "data.bin",
        op: OpKind::Write,
        count: 4096,
        free: 0,
        fd_ok: true,
        size_limit: false,
        quota: false,
        deferred: false,
        commit_err: ERR_NONE,
        reported: ERR_ENOSPC,
    },
    Shape {
        tag: "default-swap",
        path: "data.bin",
        op: OpKind::Write,
        count: 4096,
        free: 0,
        fd_ok: true,
        size_limit: false,
        quota: false,
        deferred: false,
        commit_err: ERR_NONE,
        reported: ERR_ENOSPC,
    },
];

/// 与 `SHAPES` 一一对应的申报。
///
/// 前 8 槽逐维等于现场；后 6 槽各只错一维，错的那维依次是
/// estate/stage/code/value/note/default。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim { stage: Stage::Done, code: 0, value: 4096, note: Note::Written, default: false },
    Claim { stage: Stage::Done, code: 0, value: 1024, note: Note::Short, default: false },
    Claim { stage: Stage::Done, code: 0, value: 4096, note: Note::DeferredPass, default: false },
    Claim { stage: Stage::Space, code: 28, value: 0, note: Note::NospcWrite, default: true },
    Claim { stage: Stage::Commit, code: 28, value: 0, note: Note::NospcFsync, default: true },
    Claim { stage: Stage::Limit, code: 27, value: 0, note: Note::Efbig, default: false },
    Claim { stage: Stage::Quota, code: 122, value: 0, note: Note::Edquot, default: false },
    Claim { stage: Stage::Pair, code: 28, value: 4096, note: Note::PairBroken, default: false },
    Claim { stage: Stage::Done, code: 0, value: 4096, note: Note::Written, default: false },
    Claim { stage: Stage::Commit, code: 28, value: 0, note: Note::NospcWrite, default: true },
    Claim { stage: Stage::Space, code: 0, value: 0, note: Note::NospcWrite, default: true },
    Claim { stage: Stage::Space, code: 28, value: 4096, note: Note::NospcWrite, default: true },
    Claim { stage: Stage::Space, code: 28, value: 0, note: Note::Edquot, default: true },
    Claim { stage: Stage::Space, code: 28, value: 0, note: Note::NospcWrite, default: false },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Stage, Fault::Code, Fault::Value, Fault::Note, Fault::Default];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] = [
    EN_ESTATE,
    EN_ESTAGE,
    EN_ECODE,
    EN_EVALUE,
    EN_ENOTE,
    EN_EDEFAULT,
];

/// 账本 8 个成功格的值（总数、成、败、满档、短档、邻居档、锚档、红档 ppm）。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 2, 1, 3, 2, 929];

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

/// 取一槽。
pub fn case_at(i: usize) -> Option<Case> {
    if i >= CORPUS {
        return None;
    }
    cases().get(i).copied()
}

/// 覆盖计数。
pub fn cover(n: usize) -> Result<(), u32> {
    if n != CORPUS {
        return Err(EN_ENOBS);
    }
    Ok(())
}

/// 期望缺陷。
pub fn expected(i: usize) -> Option<Fault> {
    if i < ACCEPT {
        return Some(Fault::Ok);
    }
    REJECT_FAULTS.get(i - ACCEPT).copied()
}

/// 对判：状态 → 阶段 → errno → 返回字节 → 注解 → 锚位。
pub fn judge(c: &Case) -> Fault {
    let a = c.actual();
    let k = c.claim;
    if a.ok() != k.success() {
        return Fault::Estate;
    }
    if a.stage != k.stage {
        return Fault::Stage;
    }
    if a.code != k.code {
        return Fault::Code;
    }
    if a.value != k.value {
        return Fault::Value;
    }
    if a.note != k.note {
        return Fault::Note;
    }
    if a.default != k.default {
        return Fault::Default;
    }
    Fault::Ok
}

/// 逐槽释义。
pub fn detail(c: &Case) -> String {
    let o = c.actual();
    let msg = message(&c.shape, &o);
    let mut s = String::from(c.shape.path);
    s.push_str(" op=");
    s.push_str(c.shape.op.text());
    s.push_str(" stage=");
    s.push_str(o.stage.name());
    s.push_str(" errno=");
    s.push_str(i32_text(o.code).as_str());
    s.push_str(" ret=");
    s.push_str(dec_text(o.value).as_str());
    s.push_str(" anchor=");
    s.push_str(bool_text(o.default));
    s.push_str(" note=");
    s.push_str(o.note.name());
    if !msg.is_empty() {
        s.push_str(" msg=\"");
        s.push_str(msg.as_str());
        s.push('"');
    }
    s.push('\n');
    s
}

// ---------------------------------------------------------------------------
// 期望表（语料的第二份陈述）
// ---------------------------------------------------------------------------

/// 14 槽的期望结束阶段。
///
/// 这份表是把 `SHAPES`/`CLAIMS` 用**另一种形式**再写一遍：申报表说的是
/// "我声明什么"，期望表说的是"校验器实际会走到哪一步"。两者一致才算数，
/// 任一处笔误都会在这里露出来。
const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Done,
    Stage::Done,
    Stage::Done,
    Stage::Space,
    Stage::Commit,
    Stage::Limit,
    Stage::Quota,
    Stage::Pair,
    Stage::Space,
    Stage::Space,
    Stage::Space,
    Stage::Space,
    Stage::Space,
    Stage::Space,
];

/// 14 槽的期望 errno。
const WANT_CODE: [i32; CORPUS] = [
    0,
    0,
    0,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_EFBIG,
    ERR_EDQUOT,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_ENOSPC,
];

/// 14 槽的期望返回字节。
const WANT_VALUE: [u32; CORPUS] = [
    4096, 1024, 4096, 0, 0, 0, 0, 4096, 0, 0, 0, 0, 0, 0,
];

/// 14 槽的期望注解。
const WANT_NOTE: [Note; CORPUS] = [
    Note::Written,
    Note::Short,
    Note::DeferredPass,
    Note::NospcWrite,
    Note::NospcFsync,
    Note::Efbig,
    Note::Edquot,
    Note::PairBroken,
    Note::NospcWrite,
    Note::NospcWrite,
    Note::NospcWrite,
    Note::NospcWrite,
    Note::NospcWrite,
    Note::NospcWrite,
];

/// 14 槽的期望锚位。
const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, true, true, false, false, false, true, true, true, true, true, true,
];

/// 14 槽标签。
const WANT_TAGS: [&str; CORPUS] = [
    "write-full-ok",
    "short-write",
    "delalloc-pass",
    "enospc-immediate",
    "enospc-fsync",
    "efbig-limit",
    "edquot-quota",
    "stale-pair-errno",
    "estate-swap",
    "stage-swap",
    "code-swap",
    "value-swap",
    "note-swap",
    "default-swap",
];

/// 八格申报正确的下标。
const ACCEPT_SLOTS: [usize; ACCEPT] = [0usize, 1, 2, 3, 4, 5, 6, 7];

/// 六格申报错误的下标。
const REJECT_SLOTS: [usize; REJECT] = [8usize, 9, 10, 11, 12, 13];

// ---------------------------------------------------------------------------
// 统计矩阵
// ---------------------------------------------------------------------------

/// 一档统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// 该档实际槽数。
    pub total: usize,
    /// 该档申报正确数。
    pub good: usize,
}

impl Row {
    /// 空白行。
    pub const fn blank() -> Row {
        Row { total: 0, good: 0 }
    }

    /// 该档是否全对（有槽且申报全对）。
    pub fn perfect(&self) -> bool {
        self.total > 0 && self.total == self.good
    }

    /// 该档红档数。
    pub fn red(&self) -> usize {
        if self.good <= self.total {
            self.total - self.good
        } else {
            self.total
        }
    }

    /// 渲染一行。
    pub fn render(&self, t: Tier) -> String {
        let mut s = String::from(t.name());
        s.push_str(" total=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" good=");
        s.push_str(u32_text(self.good as u32).as_str());
        s.push_str(" red=");
        s.push_str(u32_text(self.red() as u32).as_str());
        s.push('\n');
        s
    }
}

/// 四档矩阵。
///
/// 分档只看**实际结果**落在哪一档，申报正确与否单独计。这样矩阵才能同时
/// 回答两个不同的问题：语料覆盖面是否均衡（total 分布），以及申报是否可信
/// （good/total）。如果把两者混成一维，红档槽就会从自己那一档里消失，
/// 覆盖面看起来仍是满的。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行（full/short/foreign/anchor）。
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
            let c = match cs.get(i) {
                Some(v) => *v,
                None => return m,
            };
            let t = Tier::of(&c.actual()).index();
            let good = judge(&c) == Fault::Ok;
            if let Some(row) = m.rows.get_mut(t) {
                row.total += 1;
                if good {
                    row.good += 1;
                }
            }
            i += 1;
        }
        m
    }

    /// 取一档。
    pub fn row(&self, t: Tier) -> Option<Row> {
        self.rows.get(t.index()).copied()
    }

    /// 实际槽总数（必须等于语料规模）。
    pub fn slots(&self) -> usize {
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

    /// 申报正确总数。
    pub fn goods(&self) -> usize {
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

    /// 全对档数。
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

    /// 覆盖是否平（每档都有槽）。
    pub fn covered(&self) -> bool {
        let mut i = 0usize;
        while i < TIER_COUNT {
            match self.rows.get(i) {
                Some(r) => {
                    if r.total == 0 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 渲染全矩阵。
    pub fn render(&self) -> String {
        let tiers = [Tier::Full, Tier::Short, Tier::Foreign, Tier::Anchor];
        let mut s = String::new();
        let mut i = 0usize;
        while i < tiers.len() {
            if let Some(t) = tiers.get(i) {
                if let Some(r) = self.row(*t) {
                    s.push_str(r.render(*t).as_str());
                }
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 汇总报表
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
    /// 满档正确数。
    pub full: u32,
    /// 短写档正确数。
    pub short: u32,
    /// 邻居档正确数。
    pub foreign: u32,
    /// 锚档正确数。
    pub anchor: u32,
    /// 对判命中数（实际缺陷 == 期望缺陷）。
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

    /// 闸门：不到全绿就报计数错。
    pub fn gate(&self) -> Result<(), u32> {
        if self.green() {
            Ok(())
        } else {
            Err(EN_ECOUNT)
        }
    }

    /// 成功档合计（申报正确且结果成功）。
    pub fn success_cells(&self) -> u32 {
        self.full + self.short
    }

    /// 失败档合计（申报正确且结果失败）。
    pub fn failure_cells(&self) -> u32 {
        self.foreign + self.anchor
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("enospc-master corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" full=");
        s.push_str(u32_text(self.full).as_str());
        s.push_str(" short=");
        s.push_str(u32_text(self.short).as_str());
        s.push_str(" foreign=");
        s.push_str(u32_text(self.foreign).as_str());
        s.push_str(" anchor=");
        s.push_str(u32_text(self.anchor).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 全绿渲染的逐字钉版。
const RENDER_OK: &str = "enospc-master corpus=14 accept=8 reject=6 full=2 short=1 foreign=3 anchor=2 match=14 rate=1000ppm\n";

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 可注入一槽缺陷的全跑。
///
/// 注入的作用是把"期望表"本身也放进被测面：强行宣称某槽犯了另一种缺陷，
/// 报表就必须掉 matched、rate 随之离开 1000。若注入后仍报全绿，说明
/// matched 是硬编码的，而不是算出来的。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(EN_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut full = 0u32;
    let mut short = 0u32;
    let mut foreign = 0u32;
    let mut anchor = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(EN_ENOBS),
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
            None => return Err(EN_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(&a) {
                Tier::Full => full += 1,
                Tier::Short => short += 1,
                Tier::Foreign => foreign += 1,
                Tier::Anchor => anchor += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, full, short, foreign, anchor, matched })
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门。
///
/// 观测纵线是 `fd → limit → quota → space → commit → pair → done`，七段
/// 只能向前。回退（例如已经判死在 space 又声称走到了 commit）说明申报者
/// 没有按闸门次序观测，而是事后倒推——这类申报在真实故障里最危险，因为
/// 文案看着完全合理。
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    /// 上一段次序（-1 表示未走）。
    last: i32,
    /// 已走段数。
    steps: u32,
    /// 是否已关。
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
            return Err(EN_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(EN_ESTAGE);
        }
        self.last = o;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }

    /// 关闭，返回段数。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(EN_ESTATE);
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
    /// 槽位（必须严格递增）。
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

/// 账本：14 格，8 成 6 败，冻结后拒登记。
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

    /// 登记一格；槽位不连续、越界或已冻结一律拒绝并计数。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EN_ESLOT);
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

    /// 逐格对账：槽位连续、成格值等于 `VALS`、败格码等于 `REJECT_CODES`、
    /// 败格值必归零。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(EN_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(EN_EBAL),
            };
            if s.slot as usize != i {
                return Err(EN_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(EN_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(EN_EVALUE);
                        }
                    }
                    None => return Err(EN_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(EN_EVALUE);
                }
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EN_EVALUE);
                        }
                    }
                    None => return Err(EN_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(EN_EBAL);
        }
        Ok(())
    }

    /// 全账指纹。
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
            return Err(EN_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：重建一份账本并比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(EN_ESEAL)
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

/// 计数结果 → 样本。
fn count_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 建账本：7 格统计 + 1 格红档率（929）+ 6 格缺陷码。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total)));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept)));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject)));
    let _ = l.register(count_cell(3, OP_SPACE, r.map(|x| x.full)));
    let _ = l.register(count_cell(4, OP_WRITE, r.map(|x| x.short)));
    let _ = l.register(count_cell(5, OP_LIMIT, r.map(|x| x.foreign)));
    let _ = l.register(count_cell(6, OP_QUOTA, r.map(|x| x.anchor)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_REPLAY, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_PAIR,
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

/// 四路注入：形状表、判据、渲染、账本。
///
/// 四路各自独立，任何一路静默退化都会在这里变成 false：
/// 1. 表路——段基址、枚举、文件系统几何、形状合法域；
/// 2. 判路——七闸纵线、总锚配对规则、对判、矩阵；
/// 3. 渲染路——语料、期望表、红档、文案、释义；
/// 4. 账本路——阶段闸门、逐格对账、冻结重放。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && enums_ok() && geom_ok() && shape_ok();
    out[1] = define_ok() && anchor_ok() && judge_ok() && matrix_ok();
    out[2] = corpus_ok() && slots_ok() && reds_ok() && message_ok() && detail_ok();
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

/// 名称与段基址：常量链、尺寸、errno 真值、操作码一致性。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d130 {
        return false;
    }
    if EN_ESTAGE != SEG_BASE + 0x01
        || EN_ECODE != SEG_BASE + 0x02
        || EN_EVALUE != SEG_BASE + 0x03
        || EN_ENOTE != SEG_BASE + 0x04
        || EN_EDEFAULT != SEG_BASE + 0x05
        || EN_EPAIR != SEG_BASE + 0x06
    {
        return false;
    }
    if EN_ESTATE != SEG_BASE + 0x07
        || EN_ECOUNT != SEG_BASE + 0x08
        || EN_ESLOT != SEG_BASE + 0x09
        || EN_ESEAL != SEG_BASE + 0x0a
        || EN_ENOBS != SEG_BASE + 0x0b
        || EN_EMATRIX != SEG_BASE + 0x0c
        || EN_EBAL != SEG_BASE + 0x0d
    {
        return false;
    }
    if CORPUS != 14 || SAMPLES != 14 || ACCEPT != 8 || REJECT != 6 || TIER_COUNT != 4 {
        return false;
    }
    // errno 必须是 Linux/POSIX 真值，锚卡不许自造号码。
    if ERR_ENOSPC != 28 || ERR_EFBIG != 27 || ERR_EDQUOT != 122 || ERR_EBADF != 9 || ERR_EIO != 5 {
        return false;
    }
    if ERR_NONE != 0 {
        return false;
    }
    if OP_WRITE != 1 || OP_LIMIT != 2 || OP_QUOTA != 3 || OP_SPACE != 4 {
        return false;
    }
    if OP_COMMIT != 5 || OP_PAIR != 6 || OP_CORPUS != 7 || OP_REPLAY != 8 {
        return false;
    }
    if strerror(ERR_ENOSPC) != "No space left on device" {
        return false;
    }
    if strerror(ERR_EFBIG) != "File too large" || strerror(ERR_EDQUOT) != "Quota exceeded" {
        return false;
    }
    if strerror(ERR_EBADF) != "Bad file descriptor" || strerror(ERR_EIO) != "Input/output error" {
        return false;
    }
    if !strerror(ERR_NONE).is_empty() || strerror(4242) != "Unknown error" {
        return false;
    }
    let mut v = String::new();
    v.push_str("data.bin");
    !v.is_empty()
}

/// 枚举：次序、文本唯一性、分类不重不漏。
fn enums_ok() -> bool {
    let stages = [
        Stage::Fd,
        Stage::Limit,
        Stage::Quota,
        Stage::Space,
        Stage::Commit,
        Stage::Pair,
        Stage::Done,
    ];
    let mut i = 0usize;
    while i < stages.len() {
        let s = match stages.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.order() as usize != i {
            return false;
        }
        if i + 1 == stages.len() {
            if s.failure() {
                return false;
            }
        } else if !s.failure() {
            return false;
        }
        // 空间族只有 space/commit 两段。
        if s.space_family() != (i == 3 || i == 4) {
            return false;
        }
        let mut k = i + 1;
        while k < stages.len() {
            if let Some(o) = stages.get(k) {
                if o.name() == s.name() {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }

    let notes = [
        Note::Written,
        Note::Short,
        Note::DeferredPass,
        Note::EmptyWrite,
        Note::NospcWrite,
        Note::NospcFsync,
        Note::NospcClose,
        Note::Efbig,
        Note::Edquot,
        Note::Ebadf,
        Note::EioCommit,
        Note::PairBroken,
    ];
    let mut n = 0usize;
    while n < notes.len() {
        let a = match notes.get(n) {
            Some(v) => *v,
            None => return false,
        };
        let mut m = n + 1;
        while m < notes.len() {
            if let Some(b) = notes.get(m) {
                if a.name() == b.name() {
                    return false;
                }
            }
            m += 1;
        }
        // 四分类互斥：成功 / ENOSPC 族 / 邻居 / 配对破断，一注解只属一类。
        let sc = a.success() as u8 + a.enospc_family() as u8 + a.foreign() as u8;
        let is_pair = a == Note::PairBroken;
        if sc + is_pair as u8 != 1 {
            return false;
        }
        if a.partial_success() && !a.success() {
            return false;
        }
        n += 1;
    }
    if Note::NospcFsync.enospc_family() == false || Note::NospcClose.enospc_family() == false {
        return false;
    }
    if Note::Short.partial_success() == false || Note::Written.partial_success() {
        return false;
    }
    if Note::DeferredPass.success() == false || Note::EmptyWrite.success() == false {
        return false;
    }

    let tiers = [Tier::Full, Tier::Short, Tier::Foreign, Tier::Anchor];
    let mut t = 0usize;
    while t < tiers.len() {
        let x = match tiers.get(t) {
            Some(v) => *v,
            None => return false,
        };
        if x.index() != t {
            return false;
        }
        if x.success() != (t < 2) {
            return false;
        }
        if x.enospc() != (t == 3) {
            return false;
        }
        let mut y = t + 1;
        while y < tiers.len() {
            if let Some(z) = tiers.get(y) {
                if x.name() == z.name() {
                    return false;
                }
            }
            y += 1;
        }
        t += 1;
    }

    let faults = [
        Fault::Ok,
        Fault::Stage,
        Fault::Code,
        Fault::Value,
        Fault::Note,
        Fault::Default,
        Fault::Pair,
        Fault::Estate,
        Fault::Count,
        Fault::Slot,
        Fault::Seal,
        Fault::Nobs,
        Fault::Matrix,
        Fault::Bal,
    ];
    let mut f = 0usize;
    let mut seen = [0u32; 14];
    while f < faults.len() {
        let x = match faults.get(f) {
            Some(v) => *v,
            None => return false,
        };
        if x.alive() == (x == Fault::Ok) {
            return false;
        }
        if fault_short(x).is_empty() {
            return false;
        }
        if let Some(w) = seen.get_mut(f) {
            *w = x.code();
        }
        let mut g = 0usize;
        while g < f {
            if let Some(prev) = seen.get(g) {
                if *prev == x.code() {
                    return false;
                }
            }
            g += 1;
        }
        f += 1;
    }
    Fault::Ok.code() == 0
        && Fault::Stage.code() == EN_ESTAGE
        && Fault::Pair.code() == EN_EPAIR
        && Fault::Estate.code() == EN_ESTATE
        && Fault::Bal.code() == EN_EBAL
}

/// 文件系统几何闸：三块表全良，逐项破坏必判死。
fn geom_ok() -> bool {
    let gs = geoms();
    if gs.len() != 3 {
        return false;
    }
    let mut i = 0usize;
    while i < gs.len() {
        if let Some(g) = gs.get(i) {
            if !g.well_formed() {
                return false;
            }
            if g.bytes_free().is_none() {
                return false;
            }
        }
        i += 1;
    }
    // 第一块：即时分配、剩一块（短写来源）；后两块：满盘。
    let half = match gs.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let full = match gs.get(1) {
        Some(v) => *v,
        None => return false,
    };
    let lazy = match gs.get(2) {
        Some(v) => *v,
        None => return false,
    };
    if half.is_full() || !full.is_full() || !lazy.is_full() {
        return false;
    }
    if half.delalloc || !lazy.delalloc {
        return false;
    }
    if half.bytes_free() != Some(4096) {
        return false;
    }
    if full.bytes_free() != Some(0) {
        return false;
    }
    // 逐项破坏：每坏一处都必须 well_formed() == false。
    let mut b = half;
    b.name = "";
    if b.well_formed() {
        return false;
    }
    b.name = "dev/root";
    b.bsize = 4095;
    if b.well_formed() {
        return false;
    }
    b.bsize = 3;
    if b.well_formed() {
        return false;
    }
    b.bsize = 0;
    if b.well_formed() {
        return false;
    }
    b.bsize = 4096;
    b.blocks_free = 0x8000_0000;
    if b.well_formed() || b.bytes_free().is_some() {
        // 乘法溢出：bytes_free 必须 None；quota 仍合规但几何判死在溢出。
        return false;
    }
    b.blocks_free = 1;
    b.quota_hard = 0;
    if b.well_formed() {
        return false;
    }
    b.quota_hard = COUNT_MAX;
    if b.well_formed() {
        return false;
    }
    b.quota_hard = 1048576;
    // 复原必须重新良好，且指纹可复算。
    b.well_formed()
        && half.seal() == FsGeom {
            name: half.name,
            bsize: half.bsize,
            blocks_free: half.blocks_free,
            delalloc: half.delalloc,
            quota_hard: half.quota_hard,
        }
        .seal()
}

/// 形状合法域：调用形态与 errno 取值域的 lex 边界。
fn shape_ok() -> bool {
    if !OpKind::Write.instant()
        || OpKind::Fsync.instant()
        || OpKind::Close.instant()
        || OpKind::Write.commit()
        || !OpKind::Fsync.commit()
        || !OpKind::Close.commit()
    {
        return false;
    }
    if OpKind::Write.text() != "write"
        || OpKind::Fsync.text() != "fsync"
        || OpKind::Close.text() != "close"
    {
        return false;
    }
    // 合法形状：write 全维默认。
    let mut w = Shape::blank();
    w.tag = "probe";
    w.count = 4096;
    w.free = 1024;
    w.reported = ERR_ENOSPC;
    if !w.valid() {
        return false;
    }
    // 观测 errno 越界 → 非法形状（不许判）。
    w.reported = 4242;
    if w.valid() {
        return false;
    }
    w.reported = -1;
    if w.valid() {
        return false;
    }
    // fsync 不该带请求字节 → 非法。
    let mut f = Shape::blank();
    f.tag = "probe-fsync";
    f.op = OpKind::Fsync;
    f.count = 4096;
    if f.valid() {
        return false;
    }
    f.count = 0;
    if !f.valid() {
        return false;
    }
    // 提交 errno 的合法域只有 {0, 28, 5}。
    f.commit_err = ERR_EFBIG;
    if f.valid() {
        return false;
    }
    f.commit_err = ERR_EIO;
    if !f.valid() {
        return false;
    }
    // clone_count/clone_free 只改目标维，其余维必须原样。
    let mut base = Shape::blank();
    base.tag = "clone-src";
    base.path = "data.bin";
    base.count = 1024;
    base.free = 2048;
    base.reported = ERR_ENOSPC;
    let c1 = base.clone_count(4096);
    if c1.count != 4096 || c1.free != 2048 || c1.reported != ERR_ENOSPC || c1.tag != base.tag {
        return false;
    }
    let c2 = base.clone_free(0);
    if c2.free != 0 || c2.count != 1024 {
        return false;
    }
    // 指纹对形状字段敏感：改一个字段必换指纹。
    let s1 = base.seal();
    let s2 = base.clone_count(1025).seal();
    let s3 = base.clone_free(2049).seal();
    let mut b4 = base;
    b4.fd_ok = false;
    let s4 = b4.seal();
    let mut b5 = base;
    b5.op = OpKind::Close;
    let s5 = b5.seal();
    s1 != s2 && s1 != s3 && s1 != s4 && s1 != s5 && s4 != s5
}

/// 定义纵线：七闸逐段推进，先短路的段不得被后段覆盖。
fn define_ok() -> bool {
    // 闸一：坏 fd 短路一切——即便满盘也轮不到 ENOSPC。
    let mut bad = Shape::blank();
    bad.count = 4096;
    bad.free = 0;
    bad.fd_ok = false;
    bad.reported = ERR_EBADF;
    let o = define(&bad);
    if o.stage != Stage::Fd || o.code != ERR_EBADF || o.note != Note::Ebadf || o.default {
        return false;
    }
    if o.ok() || Tier::of(&o) != Tier::Foreign {
        return false;
    }
    // 闸二：尺寸上限先于配额与空间。
    let mut lim = bad;
    lim.fd_ok = true;
    lim.size_limit = true;
    lim.reported = ERR_EFBIG;
    let o = define(&lim);
    if o.stage != Stage::Limit || o.code != ERR_EFBIG || o.note != Note::Efbig {
        return false;
    }
    // 闸三：配额先于空间；EDQUOT 不许冒充 ENOSPC（锚位必须 false）。
    let mut quo = lim;
    quo.size_limit = false;
    quo.quota = true;
    quo.reported = ERR_EDQUOT;
    let o = define(&quo);
    if o.stage != Stage::Quota || o.code != ERR_EDQUOT || o.note != Note::Edquot || o.default {
        return false;
    }

    // 闸四：空间判定三分。
    let mut sp = Shape::blank();
    sp.count = 4096;
    // free >= count → 足额成功。
    sp.free = 4096;
    let o = define(&sp);
    if !o.ok() || o.note != Note::Written || o.value != 4096 || Tier::of(&o) != Tier::Full {
        return false;
    }
    sp.free = 8192;
    if define(&sp).note != Note::Written {
        return false;
    }
    // 0 < free < count → 短写，成功态，返回 free。
    sp.free = 1024;
    let o = define(&sp);
    if !o.ok() || o.note != Note::Short || o.value != 1024 || Tier::of(&o) != Tier::Short {
        return false;
    }
    if !o.note.partial_success() || o.default {
        return false;
    }
    // free == 0 → 满失败 -1/ENOSPC，锚位命中。
    sp.free = 0;
    sp.reported = ERR_ENOSPC;
    let o = define(&sp);
    if o.stage != Stage::Space || o.code != ERR_ENOSPC || o.note != Note::NospcWrite {
        return false;
    }
    if o.ok() || !o.default || !o.anchored() || o.value != 0 {
        return false;
    }
    if Tier::of(&o) != Tier::Anchor || !Tier::of(&o).enospc() {
        return false;
    }
    // 零字节写在满盘上也成功返回 0（POSIX），不许判死成 ENOSPC。
    let mut zero = sp.clone_count(0);
    zero.reported = ERR_NONE;
    let o = define(&zero);
    if !o.ok() || o.note != Note::EmptyWrite || o.value != 0 || Tier::of(&o) != Tier::Full {
        return false;
    }

    // 延迟分配：write 当场放行，fsync/close 才爆。
    let mut lazy = sp.clone_count(4096);
    lazy.deferred = true;
    lazy.reported = ERR_NONE;
    let o = define(&lazy);
    if !o.ok() || o.note != Note::DeferredPass || o.value != 4096 || o.default {
        return false;
    }
    // fsync 提交爆红：同属 ENOSPC 族，锚位命中。
    let mut fs = Shape::blank();
    fs.tag = "gate-fsync";
    fs.op = OpKind::Fsync;
    fs.deferred = true;
    fs.commit_err = ERR_ENOSPC;
    fs.reported = ERR_ENOSPC;
    let o = define(&fs);
    if o.stage != Stage::Commit
        || o.code != ERR_ENOSPC
        || o.note != Note::NospcFsync
        || !o.default
        || !o.anchored()
    {
        return false;
    }
    // close 提交爆红：与 fsync 同族异注解。
    let mut cl = fs;
    cl.op = OpKind::Close;
    let o = define(&cl);
    if o.note != Note::NospcClose || o.stage != Stage::Commit || !o.anchored() {
        return false;
    }
    if define(&fs).seal() == define(&cl).seal() {
        return false;
    }
    // 提交干净：成功。
    cl.commit_err = ERR_NONE;
    cl.reported = ERR_NONE;
    let o = define(&cl);
    if !o.ok() || o.note != Note::Written {
        return false;
    }
    // 提交期 EIO：红但不是 ENOSPC，锚位不得命中。
    let mut eio = fs;
    eio.commit_err = ERR_EIO;
    eio.reported = ERR_EIO;
    let o = define(&eio);
    if o.stage != Stage::Commit || o.code != ERR_EIO || o.note != Note::EioCommit {
        return false;
    }
    if o.default || o.anchored() || Tier::of(&o) != Tier::Foreign {
        return false;
    }

    // 闸六：配对。满失败但观测 errno 归零 → Pair 破断、锚位丢失。
    let mut broken = Shape::blank();
    broken.count = 4096;
    broken.free = 0;
    broken.reported = ERR_NONE;
    let o = define(&broken);
    if o.stage != Stage::Pair
        || o.note != Note::PairBroken
        || o.code != 0
        || o.value != 0
        || o.default
        || o.anchored()
    {
        return false;
    }
    if Tier::of(&o) != Tier::Foreign {
        return false;
    }
    // 成功但观测 errno 残存 28（stale errno）→ 同判 Pair，返回字节保留。
    let mut stale = Shape::blank();
    stale.count = 4096;
    stale.free = 4096;
    stale.reported = ERR_ENOSPC;
    let o = define(&stale);
    if o.stage != Stage::Pair || o.code != ERR_ENOSPC || o.value != 4096 || o.ok() {
        return false;
    }
    if o.note != Note::PairBroken || o.default {
        return false;
    }
    // 观测 errno 报错邻居（满失败却说 EFBIG）→ 也是 Pair 破断。
    let mut lie = broken;
    lie.reported = ERR_EFBIG;
    let o = define(&lie);
    if o.stage != Stage::Pair || o.note != Note::PairBroken {
        return false;
    }
    // 指纹：同形同指纹，异形异指纹。
    define(&sp).seal() == define(&sp).seal() && define(&sp).seal() != define(&fs).seal()
}

/// 总锚配对规则：锚 = 返回 -1 且 errno 28 且属 ENOSPC 族且配对完好。
fn anchor_ok() -> bool {
    // 三兄弟都命中锚。
    let w = match case_at(3) {
        Some(c) => c.shape,
        None => return false,
    };
    if !define(&w).anchored() {
        return false;
    }
    let mut f = Shape::blank();
    f.op = OpKind::Fsync;
    f.commit_err = ERR_ENOSPC;
    f.reported = ERR_ENOSPC;
    if !define(&f).anchored() {
        return false;
    }
    let mut c = f;
    c.op = OpKind::Close;
    if !define(&c).anchored() {
        return false;
    }
    // 邻居不命中：EFBIG/EDQUOT/EBADF/EIO 全部锚位 false。
    let mut efbig = match case_at(5) {
        Some(cs) => cs.shape,
        None => return false,
    };
    if define(&efbig).default {
        return false;
    }
    efbig.size_limit = false;
    efbig.quota = true;
    efbig.reported = ERR_EDQUOT;
    if define(&efbig).default {
        return false;
    }
    // 短写不命中：短写是成功态，锚只在失败侧亮。
    let short = match case_at(1) {
        Some(cs) => cs.shape,
        None => return false,
    };
    let o = define(&short);
    if o.default || o.anchored() || !o.ok() {
        return false;
    }
    // 延迟通过不命中：write 绿的时候锚不许亮（红在 fsync 侧）。
    let lazy = match case_at(2) {
        Some(cs) => cs.shape,
        None => return false,
    };
    if define(&lazy).default {
        return false;
    }
    // 配对破断不命中：-1/0 与 n/28 两种破断都丢掉锚。
    let stale = match case_at(7) {
        Some(cs) => cs.shape,
        None => return false,
    };
    if define(&stale).anchored() {
        return false;
    }
    // 锚的充要：失败 && errno==28 && note 属族。
    let mut probe = Shape::blank();
    probe.count = 4096;
    let grid_free = [0u32, 1, 4096];
    let grid_rep = [ERR_NONE, ERR_ENOSPC, ERR_EFBIG];
    let mut hit = 0usize;
    let mut gi = 0usize;
    while gi < grid_free.len() {
        let mut ri = 0usize;
        while ri < grid_rep.len() {
            let mut p = probe;
            if let (Some(fv), Some(rv)) = (grid_free.get(gi), grid_rep.get(ri)) {
                p.free = *fv;
                p.reported = *rv;
            }
            let o = define(&p);
            let expect = o.failed() && o.code == ERR_ENOSPC && o.note.enospc_family();
            if o.default != expect {
                return false;
            }
            if o.anchored() {
                hit += 1;
            }
            ri += 1;
        }
        gi += 1;
    }
    // 3x3 网格中恰好一格命中锚（free=0 且报 28）。
    hit == 1
}

/// 申报对判：判序固定，六维逐维独立可检。
fn judge_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let k = c.claim;
        let f = judge(&c);
        match expected(i) {
            Some(w) => {
                if f != w {
                    return false;
                }
            }
            None => return false,
        }
        if i < ACCEPT {
            if f != Fault::Ok {
                return false;
            }
            if !(a.stage == k.stage
                && a.code == k.code
                && a.value == k.value
                && a.note == k.note
                && a.default == k.default)
            {
                return false;
            }
        } else if f == Fault::Ok {
            return false;
        }
        // 逐维单点注入：只改一维必须落到该维对应缺陷。
        if i < ACCEPT {
            let mut c2 = c;
            c2.claim.value = a.value.saturating_add(7);
            if judge(&c2) != Fault::Value {
                return false;
            }
            let mut c1 = c;
            c1.claim.stage = Stage::Pair;
            if a.stage != Stage::Pair && judge(&c1) != Fault::Stage {
                return false;
            }
            if a.stage == Stage::Pair {
                c1.claim.stage = Stage::Fd;
                if judge(&c1) != Fault::Stage {
                    return false;
                }
            }
        }
        i += 1;
    }
    // 状态互报必须优先：失败实际 + 成功申报 → Estate（不是 Stage/Code/Note）。
    let red = match case_at(8) {
        Some(c) => c,
        None => return false,
    };
    if red.actual().ok() || judge(&red) != Fault::Estate {
        return false;
    }
    let mut rev = red;
    rev.claim =
        Claim { stage: Stage::Done, code: 0, value: 4096, note: Note::Written, default: false };
    if judge(&rev) != Fault::Estate {
        return false;
    }
    // 反向互报：成功实际 + 失败申报（报成满失败）→ 同为 Estate。
    let green = match case_at(0) {
        Some(c) => c,
        None => return false,
    };
    let mut g2 = green;
    g2.claim =
        Claim { stage: Stage::Space, code: 28, value: 0, note: Note::NospcWrite, default: true };
    judge(&g2) == Fault::Estate
}

/// 矩阵统计：分档覆盖、红档只出现在申报错的那一维。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    if m.slots() != CORPUS || m.goods() != ACCEPT {
        return false;
    }
    if !m.covered() {
        return false;
    }
    let full = match m.row(Tier::Full) {
        Some(r) => r,
        None => return false,
    };
    let short = match m.row(Tier::Short) {
        Some(r) => r,
        None => return false,
    };
    let foreign = match m.row(Tier::Foreign) {
        Some(r) => r,
        None => return false,
    };
    let anchor = match m.row(Tier::Anchor) {
        Some(r) => r,
        None => return false,
    };
    if full.total != 2 || full.good != 2 {
        return false;
    }
    if short.total != 1 || short.good != 1 {
        return false;
    }
    if foreign.total != 3 || foreign.good != 3 {
        return false;
    }
    // 六格复刻形状全部是 enospc-immediate，实际档同落 anchor，红档数恰为 6。
    if anchor.total != 8 || anchor.good != 2 || anchor.red() != 6 {
        return false;
    }
    if m.perfect_rows() != 3 {
        return false;
    }
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if r.total != CORPUS as u32
        || r.accept != ACCEPT as u32
        || r.reject != REJECT as u32
        || r.full != full.good as u32
        || r.short != short.good as u32
        || r.foreign != foreign.good as u32
        || r.anchor != anchor.good as u32
    {
        return false;
    }
    if r.matched != r.total || !r.green() || r.gate() != Ok(()) {
        return false;
    }
    if r.success_cells() + r.failure_cells() != r.accept {
        return false;
    }
    // 注入一槽缺陷后：matched 必降、rate 必离 1000、gate 必报计数错。
    let red = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    red.matched == r.matched - 1
        && red.rate() == 929
        && red.gate() == Err(EN_ECOUNT)
        && !red.green()
        && run_with(Some((CORPUS, Fault::Code))).is_err()
        && m.row(Tier::Full).map(|x| x.render(Tier::Full)).is_some()
}

/// 语料：14 槽、标签唯一、渲染逐字、越界无观测。
fn corpus_ok() -> bool {
    let cs = cases();
    if cs.len() != CORPUS || case_at(CORPUS).is_some() || case_at(0).is_none() {
        return false;
    }
    if cover(CORPUS) != Ok(()) || cover(CORPUS - 1) != Err(EN_ENOBS) {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if let Some(w) = WANT_TAGS.get(i) {
            if c.tag() != *w {
                return false;
            }
        } else {
            return false;
        }
        if !c.shape.valid() {
            return false;
        }
        let mut k = i + 1;
        while k < CORPUS {
            if let Some(o) = cs.get(k) {
                if o.tag() == c.tag() {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    if ACCEPT_SLOTS.len() != ACCEPT || REJECT_SLOTS.len() != REJECT {
        return false;
    }
    let mut seen = [false; CORPUS];
    let mut a = 0usize;
    while a < ACCEPT {
        if let Some(s) = ACCEPT_SLOTS.get(a) {
            if *s >= CORPUS || seen[*s] {
                return false;
            }
            seen[*s] = true;
        }
        a += 1;
    }
    let mut b = 0usize;
    while b < REJECT {
        if let Some(s) = REJECT_SLOTS.get(b) {
            if *s >= CORPUS || seen[*s] {
                return false;
            }
            seen[*s] = true;
        }
        b += 1;
    }
    let mut z = 0usize;
    while z < CORPUS {
        if let Some(v) = seen.get(z) {
            if !*v {
                return false;
            }
        }
        z += 1;
    }
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    // 红档率必须是账本最后一格的申报值：注入一槽缺陷后 13/14 → 929‰。
    let red_rate = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v.rate(),
        Err(_) => return false,
    };
    let last_val = match VALS.last() {
        Some(v) => *v,
        None => return false,
    };
    r.render() == RENDER_OK && red_rate == 929 && last_val == red_rate
}

/// 期望表逐槽核对（把语料用第二种形式再判一遍）。
fn slots_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let st = match WANT_STAGE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let code = match WANT_CODE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let val = match WANT_VALUE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let note = match WANT_NOTE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let def = match WANT_DEFAULT.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a.stage != st || a.code != code || a.value != val || a.note != note || a.default != def
        {
            return false;
        }
        // 期望表自身的一致性：成功阶段必须零 errno，反之亦然。
        if (a.stage == Stage::Done && a.note.success()) != (a.code == 0 && a.stage != Stage::Pair)
        {
            // done 且成功注解 ⇔ errno 0 且不在 pair（stale-errno 槽是唯一
            // errno 非零但结果字节非零的破断例，排除在外）。
            if !(a.stage == Stage::Pair && a.code != 0) {
                return false;
            }
        }
        i += 1;
    }
    // 后六槽申报各自偏离实际的那一维，与 REJECT_FAULTS 一一对应。
    let mut t = 0usize;
    while t < REJECT {
        let c = match cs.get(t + ACCEPT) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let hit = match REJECT_FAULTS.get(t) {
            Some(Fault::Estate) => a.ok() != c.claim.success(),
            Some(Fault::Stage) => a.ok() == c.claim.success() && a.stage != c.claim.stage,
            Some(Fault::Code) => a.stage == c.claim.stage && a.code != c.claim.code,
            Some(Fault::Value) => {
                a.code == c.claim.code && a.value != c.claim.value
            }
            Some(Fault::Note) => a.value == c.claim.value && a.note != c.claim.note,
            Some(Fault::Default) => a.note == c.claim.note && a.default != c.claim.default,
            _ => false,
        };
        if !hit {
            return false;
        }
        t += 1;
    }
    case_at(CORPUS).is_none() && case_at(3).map(|c| c.tag()) == Some("enospc-immediate")
}

/// 红档显式：六格各自挂在哪一维，缺陷码互不相同。
fn reds_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        let c = match cs.get(i + ACCEPT) {
            Some(v) => *v,
            None => return false,
        };
        let f = judge(&c);
        match REJECT_FAULTS.get(i) {
            Some(w) => {
                if f != *w {
                    return false;
                }
            }
            None => return false,
        }
        match REJECT_CODES.get(i) {
            Some(w) => {
                if f.code() != *w {
                    return false;
                }
            }
            None => return false,
        }
        if f == Fault::Ok || !f.alive() {
            return false;
        }
        // 六格实际结果全同（复刻自 enospc-immediate），差别只在申报——这正是
        // "红档显式"的立卡理由：同一现场能被六种不同方式误报。
        if let Some(base) = cs.get(3) {
            if c.actual().seal() != base.actual().seal() {
                return false;
            }
        }
        let mut k = 0usize;
        while k < i {
            if let Some(prev) = REJECT_CODES.get(k) {
                if *prev == f.code() {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    // 六格六维：期望表与申报表逐维比较，各只错一维。
    let want = match cs.get(3) {
        Some(c) => c.claim,
        None => return false,
    };
    let mut diffs = 0usize;
    let mut j = 0usize;
    while j < REJECT {
        let c = match cs.get(j + ACCEPT) {
            Some(v) => *v,
            None => return false,
        };
        let mut n = 0usize;
        if c.claim.stage != want.stage {
            n += 1;
        }
        if c.claim.code != want.code {
            n += 1;
        }
        if c.claim.value != want.value {
            n += 1;
        }
        if c.claim.note != want.note {
            n += 1;
        }
        if c.claim.default != want.default {
            n += 1;
        }
        if c.claim.success() != want.success() {
            n += 1;
        }
        if n == 0 {
            return false;
        }
        diffs += 1;
        j += 1;
    }
    diffs == REJECT
}

/// 文案逐字：strerror 与工具表面原文。
fn message_ok() -> bool {
    if strerror(ERR_ENOSPC) != "No space left on device" {
        return false;
    }
    if msg_tee_enospc("notes.txt").as_str() != "tee: notes.txt: No space left on device" {
        return false;
    }
    if msg_bash_write_err().as_str() != "bash: echo: write error: No space left on device" {
        return false;
    }
    if msg_short_write(1024, 4096).as_str() != "short write: accepted 1024 of 4096 requested bytes"
    {
        return false;
    }
    if msg_pair_broken(4096, 28).as_str() != "write/errno pair broken (ret=4096, errno=28)" {
        return false;
    }
    if msg_enospc_at("data.bin").as_str() != "write 'data.bin': No space left on device" {
        return false;
    }
    if msg_commit_enospc("fsync", "log.txt").as_str()
        != "fsync 'log.txt': No space left on device"
    {
        return false;
    }
    if msg_foreign_at("write", "big.bin", ERR_EFBIG).as_str() != "write 'big.bin': File too large"
    {
        return false;
    }
    // message() 只在失败注解上有文案，成功注解一律空串。
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let m = message(&c.shape, &a);
        if a.note.success() {
            if !m.is_empty() {
                return false;
            }
        } else if m.is_empty() {
            return false;
        }
        i += 1;
    }
    // 六复刻格同现场同文案：红档差异只在申报，不在现场。
    let a3 = match cs.get(3) {
        Some(c) => message(&c.shape, &c.actual()),
        None => return false,
    };
    let a13 = match cs.get(13) {
        Some(c) => message(&c.shape, &c.actual()),
        None => return false,
    };
    a3 == a13 && !a3.is_empty()
}

/// 满失败槽释义。
const DETAIL_NOSPC: &str = "data.bin op=write stage=space errno=28 ret=0 anchor=true note=nospc-write msg=\"write 'data.bin': No space left on device\"\n";

/// 足额成功槽释义（无 msg 段）。
const DETAIL_FULL: &str = "data.bin op=write stage=done errno=0 ret=4096 anchor=false note=written\n";

/// 提交爆红槽释义。
const DETAIL_FSYNC: &str = "log.txt op=fsync stage=commit errno=28 ret=0 anchor=true note=nospc-fsync msg=\"fsync 'log.txt': No space left on device\"\n";

/// 配对破断槽释义。
const DETAIL_PAIR: &str = "cache.bin op=write stage=pair errno=28 ret=4096 anchor=false note=pair-broken msg=\"write/errno pair broken (ret=4096, errno=28)\"\n";

/// 释义逐字 + 账本形状。
fn detail_ok() -> bool {
    let cs = cases();
    let d0 = match cs.get(0) {
        Some(c) => detail(c).as_str() == DETAIL_FULL,
        None => false,
    };
    if !d0 {
        return false;
    }
    let d3 = match cs.get(3) {
        Some(c) => detail(c).as_str() == DETAIL_NOSPC,
        None => false,
    };
    if !d3 {
        return false;
    }
    let d4 = match cs.get(4) {
        Some(c) => detail(c).as_str() == DETAIL_FSYNC,
        None => false,
    };
    if !d4 {
        return false;
    }
    let d7 = match cs.get(7) {
        Some(c) => detail(c).as_str() == DETAIL_PAIR,
        None => false,
    };
    if !d7 {
        return false;
    }
    // 成功档释义不含 msg=，失败档必含；行尾必为换行。
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let s = detail(&c);
        if !s.ends_with('\n') {
            return false;
        }
        let has_msg = s.contains("msg=");
        if c.actual().note.success() == has_msg {
            return false;
        }
        i += 1;
    }
    // 锚位文本只出现在 anchor=true 的行。
    let mut anchored = 0usize;
    let mut j = 0usize;
    while j < ACCEPT {
        if let Some(c) = cs.get(j) {
            if c.actual().anchored() {
                anchored += 1;
                if !detail(&c).contains("anchor=true") {
                    return false;
                }
            }
        }
        j += 1;
    }
    anchored == 2
}

/// 阶段闸门：单调、回退拒、关闭后拒走。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    if g.steps() != 0 || g.closed() || g.last() != -1 {
        return false;
    }
    if g.step(Stage::Space) != Ok(()) || g.steps() != 1 || g.last() != 3 {
        return false;
    }
    // 回退到更早的段必须短路为 ESTAGE（阶段申报不符）。
    if g.step(Stage::Fd) != Err(EN_ESTAGE) || g.steps() != 1 {
        return false;
    }
    if g.step(Stage::Commit) != Ok(()) || g.last() != 4 {
        return false;
    }
    // 同段重复允许（幂等观测），但不得倒退。
    if g.step(Stage::Commit) != Ok(()) || g.steps() != 3 {
        return false;
    }
    if g.step(Stage::Pair) != Ok(()) || g.step(Stage::Done) != Ok(()) {
        return false;
    }
    if g.close() != Ok(5) || !g.closed() {
        return false;
    }
    if g.close() != Err(EN_ESTATE) {
        return false;
    }
    // 关闭之后任何一段都不许再走。
    g.step(Stage::Done) == Err(EN_ESTATE)
}

/// 账本：14 格 8 成 6 败，逐格对账，越界与冻结拒登记。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.len() != SAMPLES || l.ok_count() != ACCEPT || l.err_count() != REJECT {
        return false;
    }
    if l.audit() != Ok(()) || l.sample(SAMPLES).is_some() || l.is_empty() {
        return false;
    }
    if l.rejects() != 0 {
        return false;
    }
    // 成格值必须逐格等于 VALS，败格码必须逐格等于 REJECT_CODES。
    let mut i = 0usize;
    while i < ACCEPT {
        let s = match l.sample(i) {
            Some(v) => v,
            None => return false,
        };
        match VALS.get(i) {
            Some(w) => {
                if s.value != *w || !s.ok || s.code != 0 || s.slot as usize != i {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < REJECT {
        let s = match l.sample(ACCEPT + j) {
            Some(v) => v,
            None => return false,
        };
        match REJECT_CODES.get(j) {
            Some(w) => {
                if s.code != *w || s.ok || s.value != 0 {
                    return false;
                }
            }
            None => return false,
        }
        j += 1;
    }
    // 槽位不连续必须被拒。
    let mut m = Ledger::new();
    if m.register(Sample { slot: 1, op: OP_SPACE, ok: true, code: 0, value: 1 }).is_ok() {
        return false;
    }
    if m.rejects() != 1 || m.len() != 0 {
        return false;
    }
    if m.register(Sample { slot: 0, op: OP_SPACE, ok: true, code: 0, value: 1 }).is_err() {
        return false;
    }
    if m.audit() == Ok(()) {
        return false;
    }
    let mut e = Ledger::new();
    if e.register(Sample { slot: SAMPLES as u8, op: OP_COMMIT, ok: false, code: EN_EBAL, value: 0 })
        .is_ok()
    {
        return false;
    }
    e.rejects() == 1 && m.len() == 1
}

// ---------------------------------------------------------------------------
// 判据集
// ---------------------------------------------------------------------------

/// 13 条判据。
pub fn run_unx11681_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11681_enospc_master");

    s.add("EN01-名称与段基址", name_ok(), "static");

    s.add("EN02-枚举与纵线", enums_ok(), "static");

    s.add("EN03-文件系统几何闸", geom_ok(), "static");

    s.add("EN04-形态与合法域", shape_ok(), "static");

    s.add("EN05-写回七闸纵线", define_ok(), "static");

    s.add("EN06-总锚配对规则", anchor_ok(), "static");

    s.add("EN07-申报对判", judge_ok(), "static");

    s.add("EN08-矩阵统计", matrix_ok(), "static");

    s.add("EN09-语料与渲染", corpus_ok(), "static");

    s.add("EN10-红档显式", reds_ok() && slots_ok(), "static");

    s.add("EN11-文案逐字", message_ok(), "static");

    s.add("EN12-释义与账本", detail_ok() && ledger_ok() && gate_ok(), "static");

    s.add(
        "EN13-冻结重放四注入",
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

    #[test]
    fn seg_and_sizes() {
        assert_eq!(SEG_BASE, 0x6d130);
        assert_eq!(EN_ESTAGE, SEG_BASE + 0x01);
        assert_eq!(EN_EPAIR, SEG_BASE + 0x06);
        assert_eq!(EN_EBAL, SEG_BASE + 0x0d);
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(TIER_COUNT, 4);
        assert!(name_ok());
    }

    #[test]
    fn fnv_is_stable() {
        assert_eq!(fold_str(FNV_OFF, "write"), fold_str(FNV_OFF, "write"));
        assert_ne!(fold_str(FNV_OFF, "write"), fold_str(FNV_OFF, "fsync"));
        assert_ne!(fold_word(FNV_OFF, 28), fold_word(FNV_OFF, 27));
        assert_ne!(fold_byte(FNV_OFF, 7), fold_byte(FNV_OFF, 8));
    }

    #[test]
    fn number_text_helpers() {
        assert_eq!(dec_text(0).as_str(), "0");
        assert_eq!(dec_text(929).as_str(), "929");
        assert_eq!(u32_text(4096).as_str(), "4096");
        assert_eq!(i32_text(-22).as_str(), "-22");
        assert_eq!(i32_text(122).as_str(), "122");
        assert_eq!(hex_text(0x6d130, 5).as_str(), "6d130");
        assert_eq!(bool_text(true), "true");
        assert_eq!(bool_text(false), "false");
    }

    #[test]
    fn strerror_table_is_glibc_verbatim() {
        assert_eq!(strerror(28), "No space left on device");
        assert_eq!(strerror(27), "File too large");
        assert_eq!(strerror(122), "Quota exceeded");
        assert_eq!(strerror(9), "Bad file descriptor");
        assert_eq!(strerror(5), "Input/output error");
        assert_eq!(strerror(0), "");
        assert_eq!(strerror(4242), "Unknown error");
    }

    #[test]
    fn stage_order_and_space_family() {
        assert_eq!(Stage::Fd.order(), 0);
        assert_eq!(Stage::Done.order(), 6);
        assert_eq!(Stage::Pair.name(), "pair");
        assert!(Stage::Space.failure());
        assert!(!Stage::Done.failure());
        assert!(Stage::Space.space_family());
        assert!(Stage::Commit.space_family());
        assert!(!Stage::Pair.space_family());
        assert!(enums_ok());
    }

    #[test]
    fn note_partition_is_disjoint() {
        assert!(Note::Written.success());
        assert!(Note::Short.success());
        assert!(Note::DeferredPass.success());
        assert!(Note::EmptyWrite.success());
        assert!(Note::NospcWrite.enospc_family());
        assert!(Note::NospcFsync.enospc_family());
        assert!(Note::NospcClose.enospc_family());
        assert!(!Note::NospcWrite.success());
        assert!(Note::Efbig.foreign());
        assert!(Note::Edquot.foreign());
        assert!(Note::Ebadf.foreign());
        assert!(Note::EioCommit.foreign());
        assert!(!Note::PairBroken.foreign());
        assert!(!Note::PairBroken.enospc_family());
        assert!(!Note::PairBroken.success());
    }

    #[test]
    fn op_kind_properties() {
        assert_eq!(OpKind::Write.text(), "write");
        assert_eq!(OpKind::Fsync.text(), "fsync");
        assert_eq!(OpKind::Close.text(), "close");
        assert!(OpKind::Write.instant());
        assert!(!OpKind::Fsync.instant());
        assert!(OpKind::Fsync.commit());
        assert!(OpKind::Close.commit());
        assert!(shape_ok());
    }

    #[test]
    fn geoms_are_well_formed_and_break() {
        let gs = geoms();
        assert_eq!(gs.len(), 3);
        for g in gs.iter() {
            assert!(g.well_formed());
            assert!(g.bytes_free().is_some());
        }
        assert_eq!(gs[0].bytes_free(), Some(4096));
        assert!(gs[1].is_full());
        assert!(gs[2].delalloc);
        let mut b = gs[0];
        b.bsize = 700;
        assert!(!b.well_formed());
        b.bsize = 4096;
        b.blocks_free = 0x8000_0000;
        assert!(b.bytes_free().is_none());
        assert!(!b.well_formed());
        assert!(geom_ok());
    }

    #[test]
    fn slot0_write_full_is_clean_success() {
        let c = case_at(0).expect("slot0");
        let a = c.actual();
        assert_eq!(a.stage, Stage::Done);
        assert_eq!(a.code, 0);
        assert_eq!(a.value, 4096);
        assert_eq!(a.note, Note::Written);
        assert!(!a.default);
        assert!(a.ok());
        assert_eq!(Tier::of(&a), Tier::Full);
        assert_eq!(judge(&c), Fault::Ok);
    }

    #[test]
    fn slot1_short_write_is_success_not_enospc() {
        let c = case_at(1).expect("slot1");
        let a = c.actual();
        assert_eq!(a.note, Note::Short);
        assert_eq!(a.value, 1024);
        assert!(a.ok());
        assert!(!a.default);
        assert_eq!(Tier::of(&a), Tier::Short);
        assert!(message(&c.shape, &a).is_empty());
        assert_eq!(msg_short_write(1024, 4096).as_str(), "short write: accepted 1024 of 4096 requested bytes");
    }

    #[test]
    fn slot2_delalloc_write_passes_cleanly() {
        let c = case_at(2).expect("slot2");
        let a = c.actual();
        assert!(a.ok());
        assert_eq!(a.note, Note::DeferredPass);
        assert_eq!(a.value, 4096);
        assert!(!a.anchored());
        assert_eq!(Tier::of(&a), Tier::Full);
    }

    #[test]
    fn slot3_enospc_immediate_is_the_master_anchor() {
        let c = case_at(3).expect("slot3");
        let a = c.actual();
        assert_eq!(a.stage, Stage::Space);
        assert_eq!(a.code, ERR_ENOSPC);
        assert_eq!(a.value, 0);
        assert!(a.default);
        assert!(a.anchored());
        assert_eq!(Tier::of(&a), Tier::Anchor);
        assert_eq!(detail(&c).as_str(), DETAIL_NOSPC);
        assert_eq!(message(&c.shape, &a).as_str(), "write 'data.bin': No space left on device");
    }

    #[test]
    fn slot4_deferred_commit_reports_enospc_at_fsync() {
        let c = case_at(4).expect("slot4");
        let a = c.actual();
        assert_eq!(a.stage, Stage::Commit);
        assert_eq!(a.code, ERR_ENOSPC);
        assert_eq!(a.note, Note::NospcFsync);
        assert!(a.anchored());
        assert_eq!(Tier::of(&a), Tier::Anchor);
        assert_eq!(detail(&c).as_str(), DETAIL_FSYNC);
    }

    #[test]
    fn slot5_efbig_is_not_enospc() {
        let c = case_at(5).expect("slot5");
        let a = c.actual();
        assert_eq!(a.stage, Stage::Limit);
        assert_eq!(a.code, ERR_EFBIG);
        assert_eq!(a.note, Note::Efbig);
        assert!(!a.default);
        assert_eq!(Tier::of(&a), Tier::Foreign);
        assert_eq!(message(&c.shape, &a).as_str(), "write 'big.bin': File too large");
    }

    #[test]
    fn slot6_edquot_is_not_enospc() {
        let c = case_at(6).expect("slot6");
        let a = c.actual();
        assert_eq!(a.stage, Stage::Quota);
        assert_eq!(a.code, ERR_EDQUOT);
        assert!(!a.default);
        assert_eq!(Tier::of(&a), Tier::Foreign);
        assert_eq!(message(&c.shape, &a).as_str(), "write 'home.bin': Quota exceeded");
    }

    #[test]
    fn slot7_stale_errno_breaks_the_pair() {
        let c = case_at(7).expect("slot7");
        let a = c.actual();
        assert_eq!(a.stage, Stage::Pair);
        assert_eq!(a.note, Note::PairBroken);
        // 写其实成功了 4096 字节，但 errno 残存 28：对不上暗号。
        assert_eq!(a.value, 4096);
        assert_eq!(a.code, ERR_ENOSPC);
        assert!(!a.ok());
        assert!(!a.anchored());
        assert_eq!(detail(&c).as_str(), DETAIL_PAIR);
    }

    #[test]
    fn six_reds_are_six_dimensions() {
        assert_eq!(REJECT_FAULTS.len(), REJECT);
        assert_eq!(REJECT_CODES.len(), REJECT);
        let mut i = 0usize;
        while i < REJECT {
            let c = case_at(i + ACCEPT).expect("red slot");
            assert_eq!(judge(&c), *REJECT_FAULTS.get(i).expect("fault"));
            assert_eq!(judge(&c).code(), *REJECT_CODES.get(i).expect("code"));
            i += 1;
        }
        assert!(reds_ok());
        assert!(slots_ok());
    }

    #[test]
    fn estate_beats_stage_and_code() {
        let c = case_at(8).expect("slot8");
        assert!(c.actual().failed());
        assert!(c.claim.success());
        assert_eq!(judge(&c), Fault::Estate);
        assert_eq!(judge(&c).code(), EN_ESTATE);
    }

    #[test]
    fn code_swap_is_the_errno_not_set_defect() {
        // 现场是 -1/ENOSPC，申报却说 errno 没置——这是"红了但没对上暗号"。
        let c = case_at(10).expect("slot10");
        assert_eq!(judge(&c), Fault::Code);
        assert_eq!(c.claim.code, 0);
        assert_eq!(c.actual().code, ERR_ENOSPC);
        assert_eq!(judge(&c).code(), EN_ECODE);
    }

    #[test]
    fn value_swap_claims_bytes_on_a_failure() {
        // 满失败返回 -1，申报却记了 4096 字节"写进去了"。
        let c = case_at(11).expect("slot11");
        assert_eq!(judge(&c), Fault::Value);
        assert_eq!(c.actual().value, 0);
        assert_eq!(c.claim.value, 4096);
    }

    #[test]
    fn note_swap_conflates_errno_families() {
        // 同一 ENOSPC 现场被申报成 edquot 形态——邻居混档。
        let c = case_at(12).expect("slot12");
        assert_eq!(judge(&c), Fault::Note);
        assert_eq!(c.actual().note, Note::NospcWrite);
        assert_eq!(c.claim.note, Note::Edquot);
    }

    #[test]
    fn default_swap_drops_the_anchor_bit() {
        let c = case_at(13).expect("slot13");
        assert_eq!(judge(&c), Fault::Default);
        assert!(c.actual().anchored());
        assert!(!c.claim.default);
    }

    #[test]
    fn zero_byte_write_on_full_device_succeeds() {
        let mut s = Shape::blank();
        s.count = 0;
        s.free = 0;
        let o = define(&s);
        assert!(o.ok());
        assert_eq!(o.note, Note::EmptyWrite);
        assert_eq!(o.value, 0);
        assert!(!o.default);
    }

    #[test]
    fn close_defers_enospc_like_fsync() {
        let mut s = Shape::blank();
        s.op = OpKind::Close;
        s.deferred = true;
        s.commit_err = ERR_ENOSPC;
        s.reported = ERR_ENOSPC;
        let o = define(&s);
        assert_eq!(o.stage, Stage::Commit);
        assert_eq!(o.note, Note::NospcClose);
        assert!(o.anchored());
        assert_eq!(message(&s, &o).as_str(), "close 'out.txt': No space left on device");
    }

    #[test]
    fn eio_at_commit_is_foreign_not_anchor() {
        let mut s = Shape::blank();
        s.op = OpKind::Fsync;
        s.commit_err = ERR_EIO;
        s.reported = ERR_EIO;
        let o = define(&s);
        assert_eq!(o.stage, Stage::Commit);
        assert_eq!(o.code, ERR_EIO);
        assert_eq!(o.note, Note::EioCommit);
        assert!(!o.default);
        assert_eq!(Tier::of(&o), Tier::Foreign);
    }

    #[test]
    fn bad_fd_short_circuits_full_device() {
        let mut s = Shape::blank();
        s.count = 4096;
        s.free = 0;
        s.fd_ok = false;
        let o = define(&s);
        assert_eq!(o.stage, Stage::Fd);
        assert_eq!(o.code, ERR_EBADF);
        assert_ne!(o.note, Note::NospcWrite);
        assert!(!o.anchored());
    }

    #[test]
    fn broken_pair_minus_one_errno_zero_loses_anchor() {
        let mut s = Shape::blank();
        s.count = 4096;
        s.free = 0;
        s.reported = ERR_NONE;
        let o = define(&s);
        assert_eq!(o.stage, Stage::Pair);
        assert!(!o.default);
        assert_eq!(Tier::of(&o), Tier::Foreign);
        assert!(message(&s, &o).contains("pair broken"));
    }

    #[test]
    fn anchor_only_when_minus_one_pairs_with_28() {
        // 3x3 网格：只有 free=0 且报 28 的组合命中锚。
        let mut hit = 0usize;
        let frees = [0u32, 1, 4096];
        let reps = [0i32, 28, 27];
        for f in frees.iter() {
            for r in reps.iter() {
                let mut s = Shape::blank();
                s.count = 4096;
                s.free = *f;
                s.reported = *r;
                if define(&s).anchored() {
                    hit += 1;
                }
            }
        }
        assert_eq!(hit, 1);
        assert!(anchor_ok());
    }

    #[test]
    fn report_is_all_green() {
        let r = run().expect("report");
        assert_eq!(r.total, 14);
        assert_eq!(r.accept, 8);
        assert_eq!(r.reject, 6);
        assert_eq!(r.full, 2);
        assert_eq!(r.short, 1);
        assert_eq!(r.foreign, 3);
        assert_eq!(r.anchor, 2);
        assert_eq!(r.matched, 14);
        assert_eq!(r.rate(), 1000);
        assert!(r.green());
        assert_eq!(r.gate(), Ok(()));
        assert_eq!(r.render().as_str(), RENDER_OK);
    }

    #[test]
    fn injected_slot_lowers_rate() {
        let r = run_with(Some((0, Fault::Code))).expect("red report");
        assert_eq!(r.matched, 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(*VALS.last().expect("last val"), 929);
        assert!(!r.green());
        assert_eq!(r.gate(), Err(EN_ECOUNT));
        assert!(run_with(Some((CORPUS, Fault::Ok))).is_err());
    }

    #[test]
    fn matrix_rows_are_balanced() {
        let m = Matrix::of();
        assert_eq!(m.slots(), CORPUS);
        assert_eq!(m.goods(), ACCEPT);
        assert!(m.covered());
        assert_eq!(m.perfect_rows(), 3);
        let bad = m.row(Tier::Anchor).expect("row");
        assert_eq!(bad.total, 8);
        assert_eq!(bad.good, 2);
        assert_eq!(bad.red(), 6);
        assert!(m.render().contains("full total=2 good=2 red=0"));
    }

    #[test]
    fn gate_is_monotonic() {
        let mut g = Gate::new();
        assert_eq!(g.last(), -1);
        assert_eq!(g.step(Stage::Done), Ok(()));
        assert_eq!(g.step(Stage::Fd), Err(EN_ESTAGE));
        assert_eq!(g.steps(), 1);
        assert_eq!(g.close(), Ok(1));
        assert_eq!(g.close(), Err(EN_ESTATE));
        assert!(gate_ok());
    }

    #[test]
    fn ledger_shape_is_fourteen_by_eight_six() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), ACCEPT);
        assert_eq!(l.err_count(), REJECT);
        assert_eq!(l.audit(), Ok(()));
        assert!(!l.is_empty());
        assert!(l.sample(SAMPLES).is_none());
        assert!(ledger_ok());
    }

    #[test]
    fn ledger_rejects_out_of_order_slot() {
        let mut l = Ledger::new();
        assert!(l.is_empty());
        assert_eq!(
            l.register(Sample { slot: 5, op: OP_SPACE, ok: true, code: 0, value: 1 }),
            Err(EN_ESLOT)
        );
        assert_eq!(l.rejects(), 1);
        assert_eq!(
            l.register(Sample { slot: 0, op: OP_SPACE, ok: true, code: 0, value: 14 }),
            Ok(())
        );
        assert_eq!(l.sample(0).expect("s0").value, 14);
        assert_eq!(l.audit(), Err(EN_EBAL));
    }

    #[test]
    fn ledger_freeze_replays_same_root() {
        let mut l = build_ledger();
        let r1 = l.freeze().expect("freeze");
        assert_ne!(r1, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), r1);
        assert_eq!(l.replay(), Ok(r1));
        assert_eq!(l.freeze(), Err(EN_EBAL));
        assert_eq!(
            l.register(Sample { slot: 0, op: OP_WRITE, ok: true, code: 0, value: 1 }),
            Err(EN_ESLOT)
        );
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn four_injections_all_caught() {
        let i = inject_four();
        assert!(i[0], "表路退化");
        assert!(i[1], "判路退化");
        assert!(i[2], "渲染路退化");
        assert!(i[3], "账本路退化");
    }

    #[test]
    fn corpus_bounds_and_cover() {
        assert_eq!(cases().len(), CORPUS);
        assert!(case_at(0).is_some());
        assert!(case_at(CORPUS - 1).is_some());
        assert!(case_at(CORPUS).is_none());
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(13), Err(EN_ENOBS));
    }

    #[test]
    fn tags_match_expectation_table() {
        let mut i = 0usize;
        while i < CORPUS {
            let c = case_at(i).expect("case");
            assert_eq!(c.tag(), *WANT_TAGS.get(i).expect("tag"));
            i += 1;
        }
    }

    #[test]
    fn messages_are_verbatim() {
        assert_eq!(msg_tee_enospc("x").as_str(), "tee: x: No space left on device");
        assert_eq!(
            msg_bash_write_err().as_str(),
            "bash: echo: write error: No space left on device"
        );
        assert_eq!(
            msg_pair_broken(4096, 28).as_str(),
            "write/errno pair broken (ret=4096, errno=28)"
        );
        assert_eq!(
            msg_commit_enospc("close", "f").as_str(),
            "close 'f': No space left on device"
        );
        assert_eq!(msg_foreign_at("write", "f", 9).as_str(), "write 'f': Bad file descriptor");
        assert!(message_ok());
    }

    #[test]
    fn success_notes_have_no_message() {
        let mut i = 0usize;
        while i < ACCEPT {
            let c = case_at(i).expect("case");
            let a = c.actual();
            if a.note.success() {
                assert!(message(&c.shape, &a).is_empty());
            }
            i += 1;
        }
        let c = case_at(3).expect("case");
        assert!(!message(&c.shape, &c.actual()).is_empty());
    }

    #[test]
    fn detail_lines_are_verbatim() {
        let c0 = case_at(0).expect("case0");
        assert_eq!(detail(&c0).as_str(), DETAIL_FULL);
        let c3 = case_at(3).expect("case3");
        assert_eq!(detail(&c3).as_str(), DETAIL_NOSPC);
        let c4 = case_at(4).expect("case4");
        assert_eq!(detail(&c4).as_str(), DETAIL_FSYNC);
        let c7 = case_at(7).expect("case7");
        assert_eq!(detail(&c7).as_str(), DETAIL_PAIR);
    }

    #[test]
    fn seal_is_field_sensitive() {
        let mut a = Shape::blank();
        a.tag = "s";
        a.count = 100;
        a.free = 200;
        let b = a.clone_count(101);
        assert_ne!(a.seal(), b.seal());
        let c = a.clone_free(199);
        assert_ne!(a.seal(), c.seal());
        let x = define(&a).seal();
        let y = define(&b).seal();
        assert_ne!(x, y);
    }

    #[test]
    fn short_and_full_tiers_are_distinguished_by_bytes() {
        // 短写与足额同走 done，分档只看返回字节是否等于请求。
        let c0 = case_at(0).expect("case0");
        let c1 = case_at(1).expect("case1");
        let full = define(&c0.shape);
        let short = define(&c1.shape);
        assert_eq!(Tier::of(&full), Tier::Full);
        assert_eq!(Tier::of(&short), Tier::Short);
        assert_eq!(full.value, 4096);
        assert_eq!(short.value, 1024);
    }

    #[test]
    fn all_thirteen_checks_pass() {
        let s = run_unx11681_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
