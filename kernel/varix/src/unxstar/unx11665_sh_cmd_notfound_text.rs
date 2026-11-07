//! UNX-F11665 sh 内 command not found 127 判据
//!
//! 同一件"命令找不到"的事，四台机器会说出四句不同的话：
//!
//! ```text
//! sh: foo: not found                                （busybox ash，argv[0]="sh"）
//! bash: line 1: foo: command not found              （GNU bash，bash -c / 脚本内）
//! bash: foo: command not found                      （GNU bash，交互式，无 line 段）
//! sh: 1: foo: not found                             （dash，永远带裸行号段）
//! bash: ./missing: No such file or directory        （GNU bash，词里带斜杠）
//! sh: can't open './missing': No such file or directory（busybox ash，脚本打不开）
//! busybox: 'foo' applet not found                   （busybox 本体——不是本卡的事）
//! ```
//!
//! 判据原文（UNX-F11665-J1）：**ash 报错文案 + 127 判据绿，与 B30 联签**。
//! 这句话说的是**字节**，不是数字：前缀是谁的名字、名字是不是 `argv[0]` 的
//! basename、有没有 `line 1: ` 这一段、词是原样回显还是被 basename/小写/截断、
//! 词有没有被单引号抱住、尾句是 `not found` 还是 `command not found` 还是
//! strerror 原文、这一行走的是 stderr 还是漏进了 stdout、locale 是不是 C。
//! 这九件事每一件都能单独失效，而失效之后 `$?` 仍然是 127——**数字看不见文案的
//! 崩坏**，这正是 UNX-F11664（B30）那台机器照不到的地方，也是本卡存在的理由。
//!
//! "与 B30 联签"不是"本卡也判一次数"。本卡只从文案的**族属**反推它蕴含的数
//! （`not found` / `command not found` / `can't open …No such file` 蕴含 127；
//! `Is a directory` / `Permission denied` 蕴含 126），然后要求 B30 那一侧给出的
//! 归因结论与之一致。两卡各持一本账：F11664 说"这个 127 是谁给的"（内建/哈希/
//! PATH/errno/spawn/rc 八闸），本卡说"这行字是不是这台 shell 在该说的时候
//! 逐字说对了"。任意一侧单独绿都不算数：文案对而数歪（`not found` 行配 `$?`=1，
//! 把错误行当装饰打印的 shell）→ 本卡绿、B30 红；数对而文案歪（127 但行写成
//! `bash: foo: not found`——串了方言）→ 本卡红、B30 绿。联签位就是这两本账
//! **同时**翻页的那一格。
//!
//! # 闸门次序
//!
//! `prefix → flavor → stream → segment → echo → quote → phrase → sign`，八段
//! 单调，后段不得在前段短路之后再"事后补记"。次序不可倒的理由：
//!
//! 1. `prefix` 最先——行首那个 `xxx:` 是 shell 自己的 `argv[0]` basename
//!    （busybox 的 `bb_error_msg` 用的就是 `applet_name`，GNU bash 用的是
//!    `shell_name`），它不是命令名、也不是程序里写死的字符串。前缀错意味着
//!    **整行的归属都错**：拿 `sh: can't open ...` 去和 `ash: ...` 的规范行比字节，
//!    比出来的差异全是假的。所以第一闸只问一件事：冒号前那个串是不是
//!    `argv[0]` 的最后一段、带不带斜杠、空 `argv[0]` 时是不是落成了 `""`。
//! 2. `flavor` 次之——语法表是按方言查的。方言不认识（`Unknown`）时本卡**不判**，
//!    判死在这闸而不是继续往下比字节：把未知方言塞进 ash 的模板，等于用
//!    ash 的语法去要求一台 ksh，红档毫无信息量。
//! 3. `stream` 第三——字节走哪条流决定了后面所有逐字比对读到的是哪一份数据。
//!    报错行进 stdout 是真发生过的构建缺陷（漏了 `fflush(stderr)` 顺序、或
//!    自己写了 `printf`）；这一闸必须在比对之前，否则"stdout 上那条漂亮的规范行"
//!    会被判成全对。
//! 4. `segment` 第四——`line 1: ` / `1: ` / 无行号段，三选一，由方言与
//!    交互与否唯一决定。这一闸排在回显之前，因为行号段与词之间也是一个冒号加
//!    空格；先比词就会被"段丢了导致词的冒号看起来像段的冒号"这类字节重叠骗到。
//! 5. `echo` 第五——命令字必须**逐字**回显：不 basename（`./missing` 不许缩成
//!    `missing`）、不小写、不截断、不整体丢弃。这一维是本卡与"只看有没有
//!    not found 字样"的粗糙实现的分水岭。
//! 6. `quote` 第六——单引号只在 `can't open '<word>'` 与 `'<word>' applet not found`
//!    两处出现；把引号加进 `not found` 行、或在 can't-open 行里漏掉引号，
//!    都是字节级违约，而且恰好是各家移植版最容易写错的地方。
//! 7. `phrase` 第七——尾句族属与其逐字拼写（含 strerror 的大小写与 locale）。
//!    `not found` / `command not found` / `No such file or directory` /
//!    `Is a directory` / `未找到命令` 各归各家，串族即红。
//! 8. `sign` 收尾——族属反推出的数必须与 B30（UNX-F11664）的归因结论一致。
//!    不一致时本卡判死在联签闸，并把联签位置假：这条就是卡面那句"与 B30 联签"
//!    的物理实现。
//!
//! # 关键差异（本卡深度所在）
//!
//! - **本卡 vs UNX-F11664（B30）**：同一起事件，那张判**数**，这张判**字**。
//!   F11664 把 stderr 抽象成 `TextChan` 五档族属、连一个字节都不比对；本卡反过来，
//!   把 `ash: foo: not found` 与 `bash: line 1: foo: command not found` 的前缀、
//!   行号段、引号、回显、尾句、locale、流逐字钉死，而**不判**这个数该是 126 还是
//!   127（本卡只按族属**引用**F11664 的结论做联签）。两卡互为第二台机器。
//! - **本卡 vs UNX-F11661**：F11661 判 busybox **applet 注册表**的答复行
//!   `busybox: 'foo' applet not found`——那一行不是 shell 说的，是 busybox 本体
//!   在查自己的编译期表。本卡的语法表里明确**不收** applet 行：一旦 `Flavor` 是
//!   `BusyboxApplet`，判死在方言闸，并在注解里点名 `NotOurLine`。字节很像，
//!   归属不同，这是两卡最硬的分界。
//! - **本卡 vs UNX-F11663**：F11663 判"已知但被裁掉的 applet 名"是否与"从未存在的
//!   名"**逐字同文案**"，输入是 applet 名与编译期表，判据面是"同案"与"泄漏面"
//!   （不许多说一句"它曾经存在"）；本卡的输入是 shell 方言与整行字节，判据面是
//!   **语法**（前缀/段/引号/回显/尾句）。F11663 只有一种规范文案可比，本卡要在
//!   四套方言语法之间选对那一个——选错方言就全错。
//! - **本卡 vs UNX-F11662**：F11662 判 `argv[0]` 伪名怎么参与**查表**（软链接名、
//!   带路径前缀、空 argv）；本卡只取 `argv[0]` 的 basename 做**行首归属**，
//!   一次查表都不做。同一个 `argv[0]` 字段，两张卡读的是不同的事实。
//!
//! 本模块是 `no_std` 纯功能判据：不读真实文件系统、不 fork 进程，把上面八闸与
//! 四套语法建成一张可逐字复核的事实表，再由十三项聚合判据（TX01…TX13）一次全跑。
//! 所有取值走 `match` / `.get()` / `checked_*` / `saturating_*`，生产码零
//! `unwrap`、零 `unsafe`、零 `std`。

use crate::alloc::string::String;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 段基址与规模
// ---------------------------------------------------------------------------

/// 本卡段基址：F11665 在 VX-1029-5 批里占用 0x6d1b0 段。
///
/// 同批邻居（不得撞）：F11661=0x6d100 系、F11664=0x6d1a0、F11666=0x6d1d0 起。
pub const SEG_BASE: u32 = 0x6d1b0;

/// 语料规模：14 槽 = 8 活 + 6 死。
pub const CORPUS: usize = 14usize;
/// 账本格数。
pub const SAMPLES: usize = 14usize;
/// 成格数。
pub const ACCEPT: usize = 8usize;
/// 败格数。
pub const REJECT: usize = 6usize;
/// 统计档数。
pub const TIER_COUNT: usize = 4usize;
/// 闸段数。
pub const STAGE_COUNT: usize = 8usize;
/// 申报维数（Estate/Stage/Code/Value/Note/Default 之外的兜底档号）。
pub const CAP: usize = 6;
/// 单行文案字节上限（超出即认为不是这条事件的行）。
pub const LINE_MAX: usize = 200;
/// 命令字长度上限（POSIX 名字的现实上限，超出不再逐字比）。
pub const WORD_MAX: usize = 127;
/// 前缀（argv[0] basename）长度上限。
pub const PREFIX_MAX: usize = 64;

/// POSIX 留给 shell 的 notfound 数。本卡只引用它做联签，不生产它。
pub const EXIT_NOT_FOUND: u16 = 127;
/// POSIX 留给 shell 的 not-executable 数（`Is a directory` 一族蕴含它）。
pub const EXIT_NOT_EXEC: u16 = 126;
/// 无判词（不是这条事件的行）。
pub const EXIT_NONE: u16 = 0;

/// 真机 errno 值：ENOENT。
pub const CH_ENOENT: i32 = 2;
/// 真机 errno 值：EACCES。
pub const CH_EACCES: i32 = 13;
/// 真机 errno 值：EISDIR。
pub const CH_EISDIR: i32 = 21;
/// 本卡哨兵：文案内部自相矛盾，字节不可信。
pub const CH_EINVAL: i32 = 22;
/// 零：这一行不蕴含任何 errno。
pub const CH_ENONE: i32 = 0;

// ---------------------------------------------------------------------------
// 13 个错误码（前缀 TX）
// ---------------------------------------------------------------------------

/// 闸段错：行死在第几闸，申报成了另一闸。
pub const TX_ESTAGE: u32 = SEG_BASE + 0x01u32;
/// 蕴含通道错：这一行族属反推出来的 errno 报错了。
pub const TX_ECODE: u32 = SEG_BASE + 0x02u32;
/// 字节数错：整行字节长度、段数或词长数错了。
pub const TX_EVALUE: u32 = SEG_BASE + 0x03u32;
/// 族属错：`not found` / `command not found` / `can't open` 认错家族。
pub const TX_ENOTE: u32 = SEG_BASE + 0x04u32;
/// 联签位错：与 B30 的结论是否一致被报反了。
pub const TX_EDEFAULT: u32 = SEG_BASE + 0x05u32;
/// 文案内部背离：分段拼出来的行与申报的整行不是同一份字节。
pub const TX_ECHANNEL: u32 = SEG_BASE + 0x06u32;
/// 成败互报：不是这条事件的行被说成是（或反之）。
pub const TX_ESTATE: u32 = SEG_BASE + 0x07u32;
/// 计数错：语法表/方言表数出来的行与清单不符。
pub const TX_ECOUNT: u32 = SEG_BASE + 0x08u32;
/// 槽位错：账本插队或越界登记。
pub const TX_ESLOT: u32 = SEG_BASE + 0x09u32;
/// 指纹错：重放出来的根与封存的根不同。
pub const TX_ESEAL: u32 = SEG_BASE + 0x0au32;
/// 无观测：行是空的、或一个字节都没落进被测流。
pub const TX_ENOBS: u32 = SEG_BASE + 0x0bu32;
/// 矩阵不平：四档语法面缺档或错位。
pub const TX_EMATRIX: u32 = SEG_BASE + 0x0cu32;
/// 账本不平：成/败格数或逐格值对不上。
pub const TX_EBAL: u32 = SEG_BASE + 0x0du32;

/// 十三码表（与 `Fault::code()` 逐格对齐的第二形式）。
pub const CODE_TABLE: [u32; 13usize] = [
    TX_ESTAGE,
    TX_ECODE,
    TX_EVALUE,
    TX_ENOTE,
    TX_EDEFAULT,
    TX_ECHANNEL,
    TX_ESTATE,
    TX_ECOUNT,
    TX_ESLOT,
    TX_ESEAL,
    TX_ENOBS,
    TX_EMATRIX,
    TX_EBAL,
];

// ---------------------------------------------------------------------------
// 账本操作码（这一格的观测来自哪一闸）
// ---------------------------------------------------------------------------

/// 前缀归属。
pub const OP_PREFIX: u8 = 0x01;
/// 方言识别。
pub const OP_FLAVOR: u8 = 0x02;
/// 流归属。
pub const OP_STREAM: u8 = 0x03;
/// 行号段。
pub const OP_SEGMENT: u8 = 0x04;
/// 回显词。
pub const OP_ECHO: u8 = 0x05;
/// 引号档。
pub const OP_QUOTE: u8 = 0x06;
/// 尾句族。
pub const OP_PHRASE: u8 = 0x07;
/// 联签与全账。
pub const OP_SIGN: u8 = 0x08;

// ---------------------------------------------------------------------------
// 逐字语法锚（真实 shell 的原文，含大小写与引号）
// ---------------------------------------------------------------------------

/// busybox ash 的 notfound 尾句（逐字）。
pub const ASH_TAIL: &str = ": not found";
/// GNU bash 的 command-not-found 尾句（逐字）。
pub const BASH_TAIL: &str = ": command not found";
/// busybox ash 打不开脚本时的中段（逐字，含撇号）。
pub const ASH_OPEN: &str = "can't open ";
/// GNU bash 的行号段前缀（逐字）。
pub const BASH_LINE: &str = "line ";
/// glibc strerror(ENOENT) 原文（首字母大写）。
pub const STR_ENOENT: &str = "No such file or directory";
/// glibc strerror(EISDIR) 原文。
pub const STR_EISDIR: &str = "Is a directory";
/// glibc strerror(EACCES) 原文。
pub const STR_EACCES: &str = "Permission denied";
/// busybox 自带的短 strerror 表（全小写）——ash 的 can't-open 行用它才算这台机器。
pub const BUSY_ENOENT: &str = "no such file";
/// zh_CN.UTF-8 下 GNU bash 的译文案（本卡判死：默认套件必须 LC_ALL=C）。
pub const L10N_ZH: &str = "未找到命令";
/// zh_CN.UTF-8 下 strerror(ENOENT) 的译文案。
pub const L10N_ENOENT: &str = "没有那个文件或目录";
/// zh_CN.UTF-8 下 strerror(EISDIR) 的译文案。
pub const L10N_EISDIR: &str = "是一个目录";
/// zh_CN.UTF-8 下 strerror(EACCES) 的译文案。
pub const L10N_EACCES: &str = "权限不够";
/// busybox 本体的 applet 行尾句（本卡边界外，逐字留在码里只为点名它）。
pub const APPLET_TAIL: &str = " applet not found";

/// 规范正例锚：`argv[0]="/bin/sh"` 的 busybox ash 报 `foo` 找不到。
pub const ASH_FOO_LINE: &str = "sh: foo: not found\n";
/// 规范正例锚：`bash -c` 的整行。
pub const BASH_C_LINE: &str = "bash: line 1: foo: command not found\n";
/// 规范正例锚：交互式 bash（无行号段）。
pub const BASH_I_LINE: &str = "bash: foo: command not found\n";
/// 规范正例锚：dash（裸行号段，永远带）。
pub const DASH_FOO_LINE: &str = "dash: 1: foo: not found\n";

// ---------------------------------------------------------------------------
// FNV-1a 指纹
// ---------------------------------------------------------------------------

/// FNV-1a 初始偏移。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 素数。
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

/// 折入一个字（大端逐字节）。
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

/// 折入一串字节。
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

/// 折入一串可选字节（None 折 0xFF 哨兵，避免与空串同指纹）。
fn fold_opt(h: u64, s: Option<&str>) -> u64 {
    match s {
        Some(v) => fold_str(h, v),
        None => fold_byte(FNV_OFF ^ h, 0xffu8),
    }
}

// ---------------------------------------------------------------------------
// 数字与布尔文本
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
        if let Some(slot) = buf.get_mut(i) {
            *slot = b'0' + ((n % 10) as u8);
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

/// 定宽小写十六进制。
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

/// 有符号十进制（errno 通道留负数位）。
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
// 字节串基元（手写，零下标取值）
// ---------------------------------------------------------------------------

/// 逐字相等。
pub fn same_text(a: &str, b: &str) -> bool {
    let x = a.as_bytes();
    let y = b.as_bytes();
    if x.len() != y.len() {
        return false;
    }
    let mut i = 0usize;
    while i < x.len() {
        match (x.get(i), y.get(i)) {
            (Some(p), Some(q)) => {
                if p != q {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 串长（字节）。
pub fn text_len(s: &str) -> usize {
    s.as_bytes().len()
}

/// 是否含某字节。
pub fn has_byte(s: &str, needle: u8) -> bool {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == needle {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 从 `from` 起找子串（手写窗口）。
pub fn find_from(hay: &str, needle: &str, from: usize) -> Option<usize> {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    let mut i = from;
    while i + n.len() <= h.len() {
        let mut k = 0usize;
        let mut hit = true;
        while k < n.len() {
            match (h.get(i + k), n.get(k)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        hit = false;
                        break;
                    }
                }
                _ => {
                    hit = false;
                    break;
                }
            }
            k += 1;
        }
        if hit {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 前缀逐字。
pub fn starts_with(hay: &str, pre: &str) -> bool {
    let h = hay.as_bytes();
    let p = pre.as_bytes();
    if h.len() < p.len() {
        return false;
    }
    let mut i = 0usize;
    while i < p.len() {
        match (h.get(i), p.get(i)) {
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

/// 子串是否逐字出现在任意位置。
pub fn contains_text(hay: &str, needle: &str) -> bool {
    find_from(hay, needle, 0).is_some()
}

/// `argv[0]` 的 basename：取最后一段 `/` 之后的字节（owned 拷贝，零切片）。
///
/// busybox 的 `applet_name` 与 GNU bash 的 `shell_name` 都做的事——真实现场里
/// `/bin/sh` 打的行首是 `sh:`，不是 `/bin/sh:`。带尾斜杠的 argv[0]（`sh/`）
/// 在这里返回空串，让前缀闸把它当违约处理，而不是编出一个假名字。
pub fn basename_of(s: &str) -> String {
    let b = s.as_bytes();
    let mut last = None;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b'/' {
                last = Some(i);
            }
        }
        i += 1;
    }
    let from = match last {
        Some(p) => p + 1,
        None => 0usize,
    };
    let mut out = String::new();
    let mut j = from;
    while j < b.len() {
        if let Some(c) = b.get(j) {
            out.push(*c as char);
        }
        j += 1;
    }
    if text_len(out.as_str()) > PREFIX_MAX {
        return String::new();
    }
    out
}

/// basename 是否逐字等于候选串（不构造中间量的那条路）。
pub fn basename_eq(s: &str, cand: &str) -> bool {
    same_text(basename_of(s).as_str(), cand)
}

/// 词里是否含 `/`（含斜杠的词在 bash 里走 strerror 那一族，在 ash 里仍是 not found）。
pub fn word_has_slash(s: &str) -> bool {
    has_byte(s, b'/')
}

/// 词是否为空（空词不是本卡的事件）。
pub fn word_is_empty(s: &str) -> bool {
    s.is_empty()
}

/// 串里是否含控制字节（换行、制表、CR 之外的 <0x20）。
pub fn has_control(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c < 0x20 && *c != b'\n' {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 串是否全为大写/小写敏感检查留的位：是否存在大写字母。
pub fn has_upper(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c >= b'A' && *c <= b'Z' {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 非 ASCII 字节（本地化文案的探测：`未找到命令` 每个汉字 3 字节）。
pub fn has_non_ascii(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c >= 0x80 {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 换行数（本卡的行只在末尾有一个）。
pub fn newline_count(s: &str) -> usize {
    let b = s.as_bytes();
    let mut n = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b'\n' {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 串是否以换行收尾（真机的一行必然收尾在 `\n`）。
pub fn ends_newline(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() {
        return false;
    }
    match b.get(b.len() - 1) {
        Some(c) => *c == b'\n',
        None => false,
    }
}

/// 冒号计数（本卡行里的冒号是结构位：前缀一个、词后一个、行号段一个）。
pub fn colon_count(s: &str) -> usize {
    let b = s.as_bytes();
    let mut n = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b':' {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 十进制数字串。
pub fn digits_of(v: u32) -> String {
    dec_text(v)
}

/// 大写/小写转换（只 ASCII；用来复现"词被小写化"这类缺陷）。
pub fn ascii_lower(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c >= b'A' && *c <= b'Z' {
                out.push((c + 32) as char);
            } else {
                out.push(*c as char);
            }
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 方言
// ---------------------------------------------------------------------------

/// 说这句话的是哪台 shell。
///
/// 本卡的语法表**按方言分列**：同一件 notfound 事件，ash 说 `foo: not found`、
/// bash 说 `foo: command not found`、dash 说 `1: foo: not found`。方言认错，
/// 后面每一闸的期望字节都跟着错，所以方言闸只许问一件事：这台机器的行语法
/// 在本卡的表里有没有。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    /// busybox ash（`sh`/`ash` 两个名字同一套语法）。
    Ash,
    /// GNU bash。
    Bash,
    /// dash。
    Dash,
    /// busybox 本体的 applet 查表行——字节很像，归属不是 shell，本卡不收。
    BusyboxApplet,
    /// 不认识的方言（ksh/zsh/fish/自研 shell）：本卡拒绝比字节。
    Unknown,
}

impl Flavor {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Flavor::Ash => "ash",
            Flavor::Bash => "bash",
            Flavor::Dash => "dash",
            Flavor::BusyboxApplet => "busybox-applet",
            Flavor::Unknown => "unknown",
        }
    }

    /// 是否本卡语法表内的方言。
    ///
    /// `BusyboxApplet` 刻意不在表内：`busybox: 'foo' applet not found` 是
    /// UNX-F11661/F11663 的现场本体，本卡若收下它，就等于用 shell 的语法
    /// 去验收一条查表答复，两张卡的判据面会互相吃掉。
    pub fn in_table(&self) -> bool {
        match self {
            Flavor::Ash | Flavor::Bash | Flavor::Dash => true,
            _ => false,
        }
    }

    /// 该方言允许的句族（语法表的第一列）。
    pub fn allows(&self, p: Phrase) -> bool {
        match self {
            Flavor::Ash => match p {
                Phrase::NotFound | Phrase::CantOpen => true,
                _ => false,
            },
            Flavor::Bash => match p {
                Phrase::CommandNotFound | Phrase::SysError => true,
                _ => false,
            },
            Flavor::Dash => match p {
                Phrase::NotFound => true,
                _ => false,
            },
            _ => false,
        }
    }

    /// 该方言允许的 strerror 拼写表（语法表的第二列）。
    ///
    /// GNU bash 与 dash 只会说 C 库的原文；busybox ash 在这一行上可能带
    /// busybox 自己的短表（`no such file`），短表是合法方言，写进 bash 行里
    /// 就是字节违约。
    pub fn allows_tail(&self, t: ErrnoTail) -> bool {
        match self {
            Flavor::Ash => match t {
                ErrnoTail::Glibc | ErrnoTail::BusyShort => true,
                _ => false,
            },
            Flavor::Bash | Flavor::Dash => matches!(t, ErrnoTail::Glibc | ErrnoTail::None),
            _ => false,
        }
    }

    /// 该方言的规范句族数（本卡语法表在本方言下有几行）。
    pub fn family_count(&self) -> usize {
        let all = [
            Phrase::NotFound,
            Phrase::CommandNotFound,
            Phrase::CantOpen,
            Phrase::SysError,
            Phrase::AppletNotFound,
            Phrase::Foreign,
        ];
        let mut n = 0usize;
        let mut i = 0usize;
        while i < all.len() {
            if let Some(p) = all.get(i) {
                if self.allows(*p) {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 表内方言数（判据用来对表）。
    pub const fn table_size() -> usize {
        3usize
    }

    /// 规范句族总数（ash 两族 + bash 两族 + dash 一族，按方言去重后四个族名）。
    pub const fn grammar_family_count() -> usize {
        4usize
    }
}

// ---------------------------------------------------------------------------
// 流
// ---------------------------------------------------------------------------

/// 这一行字节实际落在哪条流上。
///
/// 报错行必须在 stderr。`Both` 是真发生过的缺陷形态（既写日志又回显），
/// `Nowhere` 则是"只退了 127 一个字都没说"——那是 F11664 的现场，对本卡
/// 来说是无观测。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    /// 只有 stderr——唯一合法档。
    Stderr,
    /// 漏进了 stdout（脚本里 `echo`/`printf` 自己打的行最常见）。
    Stdout,
    /// 两条流各打一遍。
    Both,
    /// 一个字节都没有。
    Nowhere,
}

impl Stream {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stream::Stderr => "stderr",
            Stream::Stdout => "stdout",
            Stream::Both => "both",
            Stream::Nowhere => "silent",
        }
    }

    /// 是否合法（只有 stderr）。
    pub fn is_ok(&self) -> bool {
        matches!(self, Stream::Stderr)
    }

    /// 是否"被测流上读不到字节"。
    pub fn is_void(&self) -> bool {
        matches!(self, Stream::Nowhere)
    }

    /// 流档数。
    pub const fn count() -> usize {
        4usize
    }
}

// ---------------------------------------------------------------------------
// 句族（尾句那一串字节的归属）
// ---------------------------------------------------------------------------

/// 报错行的句族。本卡的"族属"就是**尾句**，不是数字。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phrase {
    /// `<词>: not found`——ash 与 dash。
    NotFound,
    /// `<词>: command not found`——GNU bash。
    CommandNotFound,
    /// `can't open '<词>': <strerror>`——busybox ash 打不开脚本。
    CantOpen,
    /// `<词>: <strerror>`——GNU bash 对含斜杠的词与坏脚本走 C 库原文。
    SysError,
    /// `'<词>' applet not found`——busybox 本体（本卡边界外）。
    AppletNotFound,
    /// 不是这条事件的行（子进程自己写的字节）。
    Foreign,
}

impl Phrase {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Phrase::NotFound => "not-found",
            Phrase::CommandNotFound => "command-not-found",
            Phrase::CantOpen => "cant-open",
            Phrase::SysError => "system-error",
            Phrase::AppletNotFound => "applet-not-found",
            Phrase::Foreign => "foreign-line",
        }
    }

    /// 词该用的引号档。
    ///
    /// 全仓只有两处加单引号：`can't open '<词>'` 与 `'<词>' applet not found`。
    /// 这两处的字节形状还不一样——前者词后还有冒号，后者没有——这正是
    /// F11661 与本卡的分界所在。
    pub fn quote(&self) -> Quote {
        match self {
            Phrase::CantOpen | Phrase::AppletNotFound => Quote::Single,
            Phrase::Foreign => Quote::Absent,
            _ => Quote::Bare,
        }
    }

    /// 词之前、行号段之后的固定中段。
    pub fn mid(&self) -> &'static str {
        match self {
            Phrase::CantOpen => "can't open ",
            _ => "",
        }
    }

    /// 尾句里是否带 strerror 原文。
    pub fn uses_strerror(&self) -> bool {
        matches!(self, Phrase::CantOpen | Phrase::SysError)
    }

    /// 尾句里 strerror 之前是否有冒号（`': No such…'` 对 `' applet not found'`）。
    pub fn colon_before_tail(&self) -> bool {
        match self {
            Phrase::AppletNotFound => false,
            _ => true,
        }
    }

    /// 句族静态尾句（不带 strerror 的那几族逐字给全，带的给到标点为止）。
    pub fn static_tail(&self) -> &'static str {
        match self {
            Phrase::NotFound => ASH_TAIL,
            Phrase::CommandNotFound => BASH_TAIL,
            Phrase::AppletNotFound => APPLET_TAIL,
            _ => ": ",
        }
    }

    /// 本卡语法表收的族（applet 行与外来行不收）。
    pub fn in_table(&self) -> bool {
        match self {
            Phrase::NotFound | Phrase::CommandNotFound | Phrase::CantOpen | Phrase::SysError => true,
            _ => false,
        }
    }

    /// 该句族蕴含的 errno（文案反推的内部通道）。
    pub fn implied_errno(&self, tok: ErrnoTok) -> i32 {
        match self {
            Phrase::NotFound | Phrase::CommandNotFound => CH_ENOENT,
            Phrase::CantOpen | Phrase::SysError => tok.value(),
            Phrase::AppletNotFound | Phrase::Foreign => CH_ENONE,
        }
    }

    /// 该句族蕴含的退出数——**本卡不判这个数**，只在联签闸引用它比对 B30。
    pub fn implied_exit(&self, tok: ErrnoTok) -> u16 {
        match self {
            Phrase::NotFound | Phrase::CommandNotFound => EXIT_NOT_FOUND,
            Phrase::CantOpen | Phrase::SysError => match tok {
                ErrnoTok::Enoent => EXIT_NOT_FOUND,
                ErrnoTok::Eacces | ErrnoTok::Eisdir => EXIT_NOT_EXEC,
                _ => EXIT_NONE,
            },
            Phrase::AppletNotFound | Phrase::Foreign => EXIT_NONE,
        }
    }

    /// 句族数。
    pub const fn count() -> usize {
        6usize
    }
}

// ---------------------------------------------------------------------------
// 引号档
// ---------------------------------------------------------------------------

/// 回显词的包裹档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quote {
    /// 裸词：`sh: foo: not found`。
    Bare,
    /// 单引号：`can't open './x'`。
    Single,
    /// 双引号：真机没有这一档，本卡专门留着抓"好心加了引号"的移植版。
    Double,
    /// 没有回显位（外来行）。
    Absent,
}

impl Quote {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Quote::Bare => "bare",
            Quote::Single => "single",
            Quote::Double => "double",
            Quote::Absent => "absent",
        }
    }

    /// 包裹后的字节。
    pub fn wrap(&self, word: &str) -> String {
        match self {
            Quote::Bare => {
                let mut s = String::new();
                s.push_str(word);
                s
            }
            Quote::Single => {
                let mut s = String::from("'");
                s.push_str(word);
                s.push('\'');
                s
            }
            Quote::Double => {
                let mut s = String::from("\"");
                s.push_str(word);
                s.push('"');
                s
            }
            Quote::Absent => String::new(),
        }
    }

    /// 引号档数。
    pub const fn count() -> usize {
        4usize
    }
}

// ---------------------------------------------------------------------------
// 行号段
// ---------------------------------------------------------------------------

/// 该方言在该运行形态下该有的行号段档。
///
/// GNU bash 只在非交互（`bash -c`、脚本里）打印 `line 1: `；dash **永远**打印
/// 裸行号段 `1: `；busybox ash 一个字节都不打。三档互斥，认错即结构违约。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegKind {
    /// 无行号段。
    None,
    /// 裸行号段：`1: `（dash）。
    BareLine,
    /// 带关键字的行号段：`line 1: `（GNU bash 非交互）。
    LineWord,
}

impl SegKind {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            SegKind::None => "no-seg",
            SegKind::BareLine => "bare-line",
            SegKind::LineWord => "line-word",
        }
    }

    /// 该档的期望字节。
    pub fn span(&self, line_no: u32) -> String {
        match self {
            SegKind::None => String::new(),
            SegKind::BareLine => {
                let mut s = dec_text(line_no);
                s.push_str(": ");
                s
            }
            SegKind::LineWord => {
                let mut s = String::from(BASH_LINE);
                s.push_str(dec_text(line_no).as_str());
                s.push_str(": ");
                s
            }
        }
    }

    /// 段档数。
    pub const fn count() -> usize {
        3usize
    }
}

// ---------------------------------------------------------------------------
// 回显形态
// ---------------------------------------------------------------------------

/// 观测到的回显与申报的命令字之间的字节关系。
///
/// 本卡要求**逐字**回显。这五个非规范形态都是真实现场里出现过的写法：
/// basename（把 `./missing` 缩成 `missing`）、小写化、大写化、截断、整体丢弃。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EchoForm {
    /// 逐字。
    Verbatim,
    /// 只回了 basename。
    Basename,
    /// 被小写化。
    Lowered,
    /// 被大写化。
    Uppered,
    /// 被截断。
    Truncated,
    /// 整体丢失。
    Dropped,
    /// 多了别的字节（转义、加空格）。
    Extra,
}

impl EchoForm {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            EchoForm::Verbatim => "verbatim",
            EchoForm::Basename => "basename",
            EchoForm::Lowered => "lowered",
            EchoForm::Uppered => "uppered",
            EchoForm::Truncated => "truncated",
            EchoForm::Dropped => "dropped",
            EchoForm::Extra => "extra",
        }
    }

    /// 是否规范。
    pub fn is_verbatim(&self) -> bool {
        matches!(self, EchoForm::Verbatim)
    }

    /// 由观测回显与命令字复算形态。
    pub fn of(word: &str, echo: &str) -> EchoForm {
        if same_text(word, echo) {
            return EchoForm::Verbatim;
        }
        if echo.is_empty() {
            return EchoForm::Dropped;
        }
        if same_text(basename_of(word).as_str(), echo) {
            return EchoForm::Basename;
        }
        if same_text(ascii_lower(word).as_str(), echo) {
            return EchoForm::Lowered;
        }
        if same_text(ascii_lower(echo).as_str(), ascii_lower(word).as_str()) {
            return EchoForm::Uppered;
        }
        if text_len(echo) < text_len(word) && starts_with(word, echo) {
            return EchoForm::Truncated;
        }
        EchoForm::Extra
    }

    /// 形态数。
    pub const fn count() -> usize {
        7usize
    }
}

// ---------------------------------------------------------------------------
// locale 与 strerror 拼写表
// ---------------------------------------------------------------------------

/// 这一行是在什么 locale 下打出来的。
///
/// 默认套件必须 `LC_ALL=C`：GNU bash 在 zh_CN.UTF-8 下把尾句译成
/// `未找到命令`，字节面与规范行完全不同，而 `$?` 仍是 127——**数字看不见它**。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    /// `LC_ALL=C`。
    C,
    /// 未设 locale（POSIX 视作 C）。
    Unset,
    /// `POSIX` locale。
    Posix,
    /// 已本地化（尾句被翻译）。
    Localized,
}

impl Locale {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Locale::C => "C",
            Locale::Unset => "unset",
            Locale::Posix => "POSIX",
            Locale::Localized => "localized",
        }
    }

    /// 是否允许（前三种都说英文原文）。
    pub fn is_english(&self) -> bool {
        match self {
            Locale::C | Locale::Unset | Locale::Posix => true,
            Locale::Localized => false,
        }
    }

    /// locale 档数。
    pub const fn count() -> usize {
        4usize
    }
}

/// 这一行的 strerror 片段来自哪张表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrnoTail {
    /// 这一族不带 strerror 片段。
    None,
    /// C 库原文（首字母大写、句内无句号）。
    Glibc,
    /// busybox 自带短表（全小写、截短）。
    BusyShort,
    /// 被 locale 翻译过的表。
    Translated,
}

impl ErrnoTail {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            ErrnoTail::None => "no-tail",
            ErrnoTail::Glibc => "libc-table",
            ErrnoTail::BusyShort => "busy-table",
            ErrnoTail::Translated => "translated",
        }
    }

    /// 是否合法表。
    pub fn is_table(&self) -> bool {
        matches!(self, ErrnoTail::None | ErrnoTail::Glibc | ErrnoTail::BusyShort)
    }

    /// 表档数。
    pub const fn count() -> usize {
        4usize
    }
}

/// 这一行的 strerror 片段说的是哪个 errno。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrnoTok {
    /// 这一族不带 errno 词。
    Absent,
    /// ENOENT。
    Enoent,
    /// EACCES。
    Eacces,
    /// EISDIR。
    Eisdir,
    /// 表里没有的 errno（本卡拒绝拼字节）。
    Other,
}

impl ErrnoTok {
    /// 真机数值。
    pub fn value(&self) -> i32 {
        match self {
            ErrnoTok::Absent => CH_ENONE,
            ErrnoTok::Enoent => CH_ENOENT,
            ErrnoTok::Eacces => CH_EACCES,
            ErrnoTok::Eisdir => CH_EISDIR,
            ErrnoTok::Other => CH_EINVAL,
        }
    }

    /// 由数值回档。
    pub fn of(v: i32) -> ErrnoTok {
        match v {
            CH_ENONE => ErrnoTok::Absent,
            CH_ENOENT => ErrnoTok::Enoent,
            CH_EACCES => ErrnoTok::Eacces,
            CH_EISDIR => ErrnoTok::Eisdir,
            _ => ErrnoTok::Other,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            ErrnoTok::Absent => "0",
            ErrnoTok::Enoent => "ENOENT",
            ErrnoTok::Eacces => "EACCES",
            ErrnoTok::Eisdir => "EISDIR",
            ErrnoTok::Other => "EINVAL",
        }
    }

    /// 是否本卡的 strerror 表覆盖它。
    pub fn in_table(&self) -> bool {
        match self {
            ErrnoTok::Enoent | ErrnoTok::Eacces | ErrnoTok::Eisdir => true,
            _ => false,
        }
    }

    /// C 库原文（逐字）。
    pub fn libc_text(&self) -> &'static str {
        match self {
            ErrnoTok::Enoent => STR_ENOENT,
            ErrnoTok::Eisdir => STR_EISDIR,
            ErrnoTok::Eacces => STR_EACCES,
            _ => "",
        }
    }

    /// busybox 短表原文（逐字；本卡只有 ENOENT 一族留了短表位）。
    pub fn busy_text(&self) -> &'static str {
        match self {
            ErrnoTok::Enoent => BUSY_ENOENT,
            _ => "",
        }
    }
}

// ---------------------------------------------------------------------------
// 联签（B30）
// ---------------------------------------------------------------------------

/// UNX-F11664（B30）那一侧对本事件的结论。
///
/// 本卡**不生产**这个结论，只引用它。四档里只有 `Agree` 能让联签位置真。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sign {
    /// B30 说：这个数是本卡该说的 shell 判词，且与本行族属蕴含的数一致。
    Agree,
    /// 两本账对不上（文案蕴含 127，B30 记的是别的数）。
    Disagree,
    /// B30 没跑：无法联签，本卡拒绝单独报绿。
    Absent,
    /// B30 判这数来自子进程：文案是 shell 的，数却不是 shell 给的。
    ChildOwn,
}

impl Sign {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Sign::Agree => "signed",
            Sign::Disagree => "disagreed",
            Sign::Absent => "unsigned",
            Sign::ChildOwn => "child-owned",
        }
    }

    /// 是否可联签。
    pub fn is_agree(&self) -> bool {
        matches!(self, Sign::Agree)
    }

    /// 联签档数。
    pub const fn count() -> usize {
        4usize
    }
}

// ---------------------------------------------------------------------------
// 八闸
// ---------------------------------------------------------------------------

/// 一行的字节在哪一道闸上判死。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 闸一：行首归属（`argv[0]` 的 basename + 冒号）。
    Prefix,
    /// 闸二：方言识别。
    Flavor,
    /// 闸三：流归属。
    Stream,
    /// 闸四：行号段。
    Segment,
    /// 闸五：词的回显。
    Echo,
    /// 闸六：包装（中段 + 引号档）。
    Quote,
    /// 闸七：尾句族与逐字拼写（含 locale 与 strerror 表）。
    Phrase,
    /// 闸八：与 B30 的联签。
    Sign,
}

impl Stage {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Prefix => "prefix",
            Stage::Flavor => "flavor",
            Stage::Stream => "stream",
            Stage::Segment => "segment",
            Stage::Echo => "echo",
            Stage::Quote => "quote",
            Stage::Phrase => "phrase",
            Stage::Sign => "sign",
        }
    }

    /// 次序（0 基）。
    pub fn order(&self) -> usize {
        match self {
            Stage::Prefix => 0,
            Stage::Flavor => 1,
            Stage::Stream => 2,
            Stage::Segment => 3,
            Stage::Echo => 4,
            Stage::Quote => 5,
            Stage::Phrase => 6,
            Stage::Sign => 7,
        }
    }

    /// 由次序回档（越界返回 `None`，闸门自检另有码）。
    pub fn from_order(i: usize) -> Option<Stage> {
        match i {
            0usize => Some(Stage::Prefix),
            1usize => Some(Stage::Flavor),
            2usize => Some(Stage::Stream),
            3usize => Some(Stage::Segment),
            4usize => Some(Stage::Echo),
            5usize => Some(Stage::Quote),
            6usize => Some(Stage::Phrase),
            7usize => Some(Stage::Sign),
            _ => None,
        }
    }

    /// 该闸专属的错误码（八闸各自能点名）。
    pub fn code(&self) -> u32 {
        match self {
            Stage::Prefix => TX_ECHANNEL,
            Stage::Flavor => TX_ENOBS,
            Stage::Stream => TX_ESTATE,
            Stage::Segment => TX_ESTAGE,
            Stage::Echo => TX_EVALUE,
            Stage::Quote => TX_ECOUNT,
            Stage::Phrase => TX_ENOTE,
            Stage::Sign => TX_EDEFAULT,
        }
    }

    /// 是否字节闸（闸一到闸七都在比字节；闸八比的是两本账）。
    pub fn is_byte_gate(&self) -> bool {
        match self {
            Stage::Sign => false,
            _ => true,
        }
    }

    /// 闸段总数。
    pub const fn count() -> usize {
        STAGE_COUNT
    }
}

// ---------------------------------------------------------------------------
// 注解（这一行到底是什么）
// ---------------------------------------------------------------------------

/// 判定注解：本卡对整行的最终说法。
///
/// 前四档是**规范行**（本卡语法表收下的四种写法），后九档各钉一种违约。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextNote {
    /// 规范：ash/dash 的 `<词>: not found`。
    PlainNotFound,
    /// 规范：GNU bash 的 `<词>: command not found`。
    CommandNotFound,
    /// 规范：GNU bash 的 `<词>: <strerror>`。
    SysError,
    /// 规范：busybox ash 的 `can't open '<词>': <strerror>`。
    CantOpen,
    /// 行首归属错：前缀不是 `argv[0]` 的 basename，或冒号/空格缺位。
    PrefixWrong,
    /// 行号段错：该有 `line 1: ` 的没有、不该有的多了、或形态串了方言。
    SegmentWrong,
    /// 回显错：词被 basename/大小写/截断/丢弃/加了料。
    EchoMangled,
    /// 包装错：引号档或 `can't open ` 中段与句族不符。
    QuoteWrong,
    /// 流错：行没走 stderr。
    StreamLeak,
    /// 尾句错：句族与方言不符，或尾句字节不符。
    PhraseWrong,
    /// locale 错：尾句被翻译（默认套件必须 C）。
    LocaleWrong,
    /// 内部背离：分段与整行两种形式不是同一份字节。
    ChanSplit,
    /// 与 B30 联签不上。
    SignBreak,
    /// 这一行不是 shell 说的（不认识的方言、applet 行、子进程自己的字节）。
    NotOurLine,
}

impl TextNote {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            TextNote::PlainNotFound => "not-found-line",
            TextNote::CommandNotFound => "command-not-found-line",
            TextNote::SysError => "system-error-line",
            TextNote::CantOpen => "cant-open-line",
            TextNote::PrefixWrong => "prefix-wrong",
            TextNote::SegmentWrong => "segment-wrong",
            TextNote::EchoMangled => "echo-mangled",
            TextNote::QuoteWrong => "wrap-wrong",
            TextNote::StreamLeak => "stream-leak",
            TextNote::PhraseWrong => "phrase-wrong",
            TextNote::LocaleWrong => "locale-wrong",
            TextNote::ChanSplit => "line-split",
            TextNote::SignBreak => "sign-break",
            TextNote::NotOurLine => "not-our-line",
        }
    }

    /// 是否规范行（本卡语法表收下的四档）。
    pub fn in_grammar(&self) -> bool {
        match self {
            TextNote::PlainNotFound
            | TextNote::CommandNotFound
            | TextNote::SysError
            | TextNote::CantOpen => true,
            _ => false,
        }
    }

    /// 是否违约注解。
    pub fn is_wrong(&self) -> bool {
        !self.in_grammar()
    }

    /// 违约属于结构面还是族属面。
    pub fn is_structure(&self) -> bool {
        match self {
            TextNote::PrefixWrong
            | TextNote::SegmentWrong
            | TextNote::EchoMangled
            | TextNote::QuoteWrong
            | TextNote::StreamLeak => true,
            _ => false,
        }
    }

    /// 违约属于族属/一致性面。
    pub fn is_family(&self) -> bool {
        match self {
            TextNote::PhraseWrong | TextNote::LocaleWrong | TextNote::ChanSplit | TextNote::SignBreak => {
                true
            }
            _ => false,
        }
    }

    /// 规范行对应的句族（违约档返回 `None`）。
    pub fn phrase(&self) -> Option<Phrase> {
        match self {
            TextNote::PlainNotFound => Some(Phrase::NotFound),
            TextNote::CommandNotFound => Some(Phrase::CommandNotFound),
            TextNote::SysError => Some(Phrase::SysError),
            TextNote::CantOpen => Some(Phrase::CantOpen),
            _ => None,
        }
    }

    /// 注解数。
    pub const fn count() -> usize {
        14usize
    }
}

// ---------------------------------------------------------------------------
// 四档统计面
// ---------------------------------------------------------------------------

/// 按实际结果投影的四个档。
///
/// 与 F11664 的分档理由不同：那张按**数是谁给的**分（run/notfound/notexec/
/// foreign），这张按**行歪在哪一面**分（语法/结构/族属/归属）。同一句
/// `sh: foo: not found` 在 F11664 落 `notfound` 档，在本卡落 `grammar` 档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 语法面：本卡表内的规范行。
    Grammar,
    /// 结构面：前缀/段/回显/包装/流歪了。
    Structure,
    /// 族属面：尾句族、locale、两种形式的自洽、联签歪了。
    Family,
    /// 归属面：这行压根不是这台 shell 说的。
    Foreign,
}

impl Tier {
    /// 由实际结果投影。
    ///
    /// 先按**规范行的句族**分两档（notfound 两族是语法面、strerror 两族是
    /// 族属面），再按违约注解分结构面与族属面，剩下只有"这行不是这台 shell
    /// 说的"一种可能。
    pub fn of(o: &Outcome) -> Tier {
        match o.note {
            TextNote::PlainNotFound | TextNote::CommandNotFound => Tier::Grammar,
            TextNote::CantOpen | TextNote::SysError => Tier::Family,
            _ => {
                if o.note.is_structure() {
                    Tier::Structure
                } else if o.note.is_family() {
                    Tier::Family
                } else {
                    Tier::Foreign
                }
            }
        }
    }

    /// 0 基档号。
    pub fn index(&self) -> usize {
        match self {
            Tier::Grammar => 0usize,
            Tier::Structure => 1usize,
            Tier::Family => 2usize,
            Tier::Foreign => 3usize,
        }
    }

    /// 由档号回档。
    pub fn at(i: usize) -> Tier {
        match i {
            0usize => Tier::Grammar,
            1usize => Tier::Structure,
            2usize => Tier::Family,
            _ => Tier::Foreign,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Grammar => "grammar",
            Tier::Structure => "structure",
            Tier::Family => "family",
            Tier::Foreign => "foreign",
        }
    }

    /// 是否规范档。
    pub fn success(&self) -> bool {
        matches!(self, Tier::Grammar)
    }

    /// 档数。
    pub const fn count() -> usize {
        4usize
    }
}

// ---------------------------------------------------------------------------
// 申报缺陷
// ---------------------------------------------------------------------------

/// 六维申报缺陷 + 七种系统缺陷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 一致。
    Ok,
    /// 闸段错。
    Stage,
    /// 蕴含 errno 通道错。
    Code,
    /// 字节数错。
    Value,
    /// 注解错。
    Note,
    /// 联签位错。
    Default,
    /// 行内两种形式背离。
    Channel,
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
            Fault::Stage => TX_ESTAGE,
            Fault::Code => TX_ECODE,
            Fault::Value => TX_EVALUE,
            Fault::Note => TX_ENOTE,
            Fault::Default => TX_EDEFAULT,
            Fault::Channel => TX_ECHANNEL,
            Fault::Estate => TX_ESTATE,
            Fault::Count => TX_ECOUNT,
            Fault::Slot => TX_ESLOT,
            Fault::Seal => TX_ESEAL,
            Fault::Nobs => TX_ENOBS,
            Fault::Matrix => TX_EMATRIX,
            Fault::Bal => TX_EBAL,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Fault::Ok => "ok",
            Fault::Stage => "gate",
            Fault::Code => "errno",
            Fault::Value => "bytes",
            Fault::Note => "note",
            Fault::Default => "sign",
            Fault::Channel => "split",
            Fault::Estate => "estate",
            Fault::Count => "count",
            Fault::Slot => "slot",
            Fault::Seal => "seal",
            Fault::Nobs => "nobs",
            Fault::Matrix => "matrix",
            Fault::Bal => "bal",
        }
    }

    /// 是否非零缺陷。
    pub fn alive(&self) -> bool {
        !matches!(self, Fault::Ok)
    }

    /// 是否六维申报缺陷之一。
    pub fn is_claim_fault(&self) -> bool {
        match self {
            Fault::Estate | Fault::Stage | Fault::Code | Fault::Value | Fault::Note | Fault::Default => {
                true
            }
            _ => false,
        }
    }
}

/// 缺陷短名。
pub fn fault_short(f: Fault) -> &'static str {
    f.name()
}

// ---------------------------------------------------------------------------
// 现场（一行报错的全部可观测事实）
// ---------------------------------------------------------------------------

/// 一槽现场：一行 shell 报错的全部**观测**事实。
///
/// 与 F11664 的 `Shape` 最大的不同在字段构成：那张的字段是"这台 shell 做了
/// 什么"（扫了几个目录、命中了什么档、exec 返了什么 errno），本张的字段是
/// "这一行长什么样"——`prefix`/`segkind`/`echo`/`quote`/`tok`/`tail` 六组
/// 字节位，外加一份**整行原文** `body`。
///
/// 为什么 `word`（命令字）与 `echo`（回显）要分成两个字段：真机上这两处
/// **可以不等**，而这一等不等正是本卡的一道闸。shell 打印的是它当时记住的
/// 那份字符串；`./missing` 被 basename 化成 `missing`、`Foo` 被小写化成
/// `foo`、超长的词被截到行宽——都发生在这一位上。合成一个字段就等于宣布
/// "回显永远等于命令字"，把第五闸直接删掉了。
///
/// 为什么 `body` 要独立存一份：本卡要求"分段拼出来的行"与"整行字节"两种
/// 形式同时在场并互相对判（见文件头"行级两两自洽"）。只存一份的话，
/// `ChanSplit` 这一类现场内部背离的缺陷根本无从表达。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 语料标签。
    pub tag: &'static str,
    /// `argv[0]`：程序名前缀的来源（真机常见 `/bin/sh` 这类全路径）。
    pub argv0: &'static str,
    /// 实际写进 stderr 的行首前缀（含冒号，不含后面的空格）。
    pub prefix: &'static str,
    /// 命令字（shell 当时准备执行的那串字符串）。
    pub word: &'static str,
    /// 实际回显的字节——与 `word` 不等就是第五闸的事。
    pub echo: &'static str,
    /// 方言档。
    pub flavor: Flavor,
    /// 是否交互态（只影响 GNU bash 的行号段档）。
    pub interactive: bool,
    /// 行号：bash 的 `line N` 与 dash 的 `N:` 用的是 shell 内部的命令行号。
    pub line_no: u32,
    /// 这一行落在哪条流。
    pub stream: Stream,
    /// 句族。
    pub phrase: Phrase,
    /// 实际带没带行号段、带的是哪一种。
    pub segkind: SegKind,
    /// 词的包裹档。
    pub quote: Quote,
    /// 尾句里的 errno 词（不带 strerror 的族恒为 `Absent`）。
    pub tok: ErrnoTok,
    /// 尾句里的 strerror 来自哪张表。
    pub tail: ErrnoTail,
    /// locale 档。
    pub locale: Locale,
    /// B30（UNX-F11664）对本事件的联签结论。
    pub sign: Sign,
    /// 整行原文（含行末换行符）——第二形式的锚。
    pub body: &'static str,
}

impl Shape {
    /// 空白现场。
    ///
    /// 刻意选一个**两形式不符**的合法形态组合做兜底：`body` 是空串，任何
    /// 拼装结果都不等于它，于是空白现场必然死在联签之后的自洽闸上。这样
    /// 兜底不会被误当成绿色——同 F11664 用 `EINVAL` 兜底的理由一致。
    pub const fn blank() -> Shape {
        Shape {
            tag: "blank",
            argv0: "sh",
            prefix: "sh:",
            word: "foo",
            echo: "foo",
            flavor: Flavor::Ash,
            interactive: false,
            line_no: 1,
            stream: Stream::Stderr,
            phrase: Phrase::NotFound,
            segkind: SegKind::None,
            quote: Quote::Bare,
            tok: ErrnoTok::Absent,
            tail: ErrnoTail::None,
            locale: Locale::C,
            sign: Sign::Agree,
            body: "",
        }
    }

    /// 语料标签。
    pub fn tag(&self) -> &'static str {
        self.tag
    }

    /// 该行应有的行号段档（方言 + 交互态决定，不看现场自己写了什么）。
    pub fn want_seg(&self) -> SegKind {
        match self.flavor {
            Flavor::Bash => {
                if self.interactive {
                    SegKind::None
                } else {
                    SegKind::LineWord
                }
            }
            Flavor::Dash => SegKind::BareLine,
            _ => SegKind::None,
        }
    }

    /// 该句族是否要求尾句带 errno 词。
    pub fn tok_required(&self) -> bool {
        self.phrase.uses_strerror()
    }

    /// 命令字的字节数（回显位与 `word` 不等时，行的长度按回显算）。
    pub fn echo_len(&self) -> usize {
        text_len(self.echo)
    }

    /// 行首归属是否由 `argv[0]` 推得——单独成函数，供第一闸与释义共用。
    pub fn prefix_is_mine(&self) -> bool {
        same_text(msg_prefix(self.argv0).as_str(), self.prefix)
    }

    /// 回显形态。
    pub fn echo_form(&self) -> EchoForm {
        EchoForm::of(self.word, self.echo)
    }

    /// 包裹档与句族要求是否一致。
    pub fn quote_matches(&self) -> bool {
        if self.quote == Quote::Double {
            return false;
        }
        self.quote == self.phrase.quote()
    }

    /// 第一形式：由语法部件拼出整行。
    pub fn line(&self) -> String {
        line_of(self)
    }

    /// 第二形式：整行原文去掉行末换行。
    pub fn line_trim(&self) -> &'static str {
        self.body
    }

    /// 两种形式是否同一份字节。
    pub fn forms_agree(&self) -> bool {
        match self.locale {
            Locale::Localized => false,
            _ => same_text(self.line().as_str(), self.body),
        }
    }

    /// 整行字节数（含行末换行）——本卡的 `value` 通道。
    pub fn bytes(&self) -> u32 {
        text_len(self.body) as u32
    }

    /// 回显位在整行里的起始字节偏移。
    ///
    /// 本卡的第二张表写的就是这个数（`WANT_NDX`）：语法位对了、回显内容也
    /// 对了，但词在行里的**位置**错了（例如少打了一个空格），偏移会跟着动，
    /// 而字节总数可能不变——位置维能抓到长度维抓不到的那类手滑。
    pub fn echo_index(&self) -> u32 {
        let head = self.line_head();
        let mut n = text_len(head.as_str()) as u32;
        // head 已含前缀、空格、行号段、中段；再加引号档的前引号
        if self.quote == Quote::Single || self.quote == Quote::Double {
            n = n.saturating_add(1);
        }
        n
    }

    /// 回显之前的所有字节（前缀 + 空格 + 行号段 + 中段）。
    pub fn line_head(&self) -> String {
        let mut s = String::new();
        s.push_str(self.prefix);
        s.push(' ');
        s.push_str(self.segkind.span(self.line_no).as_str());
        s.push_str(self.phrase.mid());
        s
    }
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_str(h, self.argv0);
        h = fold_str(h, self.prefix);
        h = fold_str(h, self.word);
        h = fold_str(h, self.echo);
        h = fold_str(h, self.flavor.name());
        h = fold_word(h, self.interactive as u64);
        h = fold_word(h, self.line_no as u64);
        h = fold_str(h, self.stream.name());
        h = fold_str(h, self.phrase.name());
        h = fold_str(h, self.segkind.name());
        h = fold_str(h, self.quote.name());
        h = fold_str(h, self.tok.name());
        h = fold_str(h, self.tail.name());
        h = fold_str(h, self.locale.name());
        h = fold_str(h, self.sign.name());
        fold_str(h, self.body)
    }
}

// ---------------------------------------------------------------------------
// 实际结果
// ---------------------------------------------------------------------------

/// 现场走完八闸后的实际结果。
///
/// 六个通道里 `value` 是**本卡专属**的一维：它记的是整行字节数，不是退出码。
/// 与 F11664 同名通道的语义差别正在这里——那张的 `value` 是 `$?`，本张的
/// `value` 是这一行有多少个字节。两本账都要，是因为 127 与"文案对不对"
/// 之间没有函数关系：本地化过的 bash 退 127、文案整行都不是英文。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 判死在哪道闸（走完八闸为 `Sign`）。
    pub stage: Stage,
    /// 内部 errno 通道（文案蕴含的那个数）。
    pub code: i32,
    /// 观测到的整行字节数（含行末换行）。
    pub value: u32,
    /// 本卡对整行的说法。
    pub note: TextNote,
    /// 联签位：这一行的事件在 B30 侧是不是 127 档。
    pub default: bool,
}

impl Outcome {
    /// 空白结果（死在第一闸、无观测）。
    pub const fn blank() -> Outcome {
        Outcome {
            stage: Stage::Prefix,
            code: CH_EINVAL,
            value: 0,
            note: TextNote::NotOurLine,
            default: false,
        }
    }

    /// 是否规范行（八闸走满、注解属于语法表四档）。
    pub fn ok(&self) -> bool {
        self.stage == Stage::Sign && self.note.in_grammar()
    }

    /// 是否判红。
    pub fn failed(&self) -> bool {
        !self.ok()
    }

    /// 是否是"本卡不收的行"（归属面）。
    pub fn is_foreign(&self) -> bool {
        matches!(self.note, TextNote::NotOurLine)
    }

    /// 该行蕴含的退出数（只在联签闸引用，比对 B30；本卡不生产它）。
    pub fn implied_exit(&self) -> u16 {
        if self.default {
            EXIT_NOT_FOUND
        } else {
            match self.code {
                CH_EACCES | CH_EISDIR => EXIT_NOT_EXEC,
                _ => EXIT_NONE,
            }
        }
    }

    /// 投影档。
    pub fn tier(&self) -> Tier {
        Tier::of(self)
    }

    /// 结果指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.stage.name());
        h = fold_i32(h, self.code);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        fold_word(h, self.default as u64)
    }
}

/// 负数通道的 FNV 折叠（`code` 是 `i32`，先转符号位再折数值）。
fn fold_i32(h: u64, v: i32) -> u64 {
    let mut o = fold_word(h, if v < 0 { 1u64 } else { 0u64 });
    let mut a = v;
    if a < 0 {
        a = a.saturating_add(1);
        a = a.saturating_neg();
    }
    let mut i = 0usize;
    while i < 4usize {
        let b = ((a >> (8 * i)) as u32) & 0xffu32;
        o = fold_byte(o, b as u8);
        i += 1;
    }
    o
}

/// 注解查表：句族 → 规范注解（语法表四档）。
pub fn note_of_phrase(p: Phrase) -> TextNote {
    match p {
        Phrase::NotFound => TextNote::PlainNotFound,
        Phrase::CommandNotFound => TextNote::CommandNotFound,
        Phrase::CantOpen => TextNote::CantOpen,
        Phrase::SysError => TextNote::SysError,
        _ => TextNote::NotOurLine,
    }
}

/// strerror 拼写表（按方言选表；busybox 短表只覆盖 ENOENT 一族）。
pub fn strerror_text(tok: ErrnoTok, tail: ErrnoTail) -> &'static str {
    match tail {
        ErrnoTail::None => "",
        ErrnoTail::Glibc => tok.libc_text(),
        ErrnoTail::BusyShort => tok.busy_text(),
        ErrnoTail::Translated => match tok {
            ErrnoTok::Enoent => L10N_ENOENT,
            ErrnoTok::Eisdir => L10N_EISDIR,
            ErrnoTok::Eacces => L10N_EACCES,
            _ => "",
        },
    }
}

/// 第一形式的逐字拼装：前缀 + 空格 + 行号段 + 中段 + 包裹词 + 尾句 + 换行。
///
/// 这一段是本卡**唯一**产生整行字节的地方；语料表里的 `body` 是第二形式，
/// 两处由不同的人手写定（见文件头），`define` 在八闸走满之后把它们对齐。
pub fn line_of(s: &Shape) -> String {
    let mut out = String::new();
    out.push_str(s.prefix);
    out.push(' ');
    out.push_str(s.segkind.span(s.line_no).as_str());
    out.push_str(s.phrase.mid());
    out.push_str(s.quote.wrap(s.echo).as_str());
    out.push_str(s.phrase.static_tail());
    if s.phrase.uses_strerror() {
        out.push_str(strerror_text(s.tok, s.tail));
    }
    out.push('\n');
    out
}

/// 短探针现场（只服务于判据自检，不进语料、不进账本）。
fn probe(tag: &'static str, body: &'static str) -> Shape {
    let mut s = Shape::blank();
    s.tag = tag;
    s.body = body;
    s
}

// ---------------------------------------------------------------------------
// 八闸纵线
// ---------------------------------------------------------------------------

/// 由现场推演实际结果：本卡的心脏。
///
/// 八道闸单调向前，每道只问自己那一句：
///
/// 1. `prefix`：行首是不是 `argv[0]` 的 basename 加冒号；
/// 2. `flavor`：这台机器在本卡语法表里有没有；
/// 3. `stream`：字节是不是只落在 stderr；
/// 4. `segment`：行号段是不是该方言在该形态下该有的那一档；
/// 5. `echo`：词有没有逐字回显；
/// 6. `quote`：包裹档与句族要求是否一致；
/// 7. `phrase`：句族是否属于本方言、locale 是否英文、strerror 是否用了对的表；
/// 8. `sign`：与 B30 联没联上；随后补一次两种形式的字节对齐。
///
/// 任一闸判死即返回，`code`/`value`/`default` 仍按观测填——**不清零**。
/// 清零会让"死在第二闸的行"在数值面上长得像"没有行"，与 `Nowhere` 混淆。
pub fn define(s: &Shape) -> Outcome {
    let code = s.phrase.implied_errno(s.tok);
    let value = s.bytes();
    let default = s.phrase.implied_exit(s.tok) == EXIT_NOT_FOUND;

    // 闸一：行首归属。空 body 视作无观测，归 `NotOurLine`，让无观测与
    // "有行但前缀错"分家——前者是 TX_ENOBS 的事，后者是 TX_ECHANNEL 的事。
    if value == 0 || s.body.is_empty() {
        return Outcome {
            stage: Stage::Prefix,
            code: CH_EINVAL,
            value: 0,
            note: TextNote::NotOurLine,
            default: false,
        };
    }
    if !s.prefix_is_mine() || newline_count(s.body) != 1 || !ends_newline(s.body) {
        return Outcome {
            stage: Stage::Prefix,
            code: s.phrase.implied_errno(s.tok),
            value,
            note: TextNote::PrefixWrong,
            default,
        };
    }

    // 闸二：方言。表外方言（含 busybox 本体的 applet 行）在这一闸出局。
    if !s.flavor.in_table() {
        return Outcome {
            stage: Stage::Flavor,
            code,
            value,
            note: TextNote::NotOurLine,
            default,
        };
    }

    // 闸三：流。
    if !s.stream.is_ok() {
        return Outcome {
            stage: Stage::Stream,
            code,
            value,
            note: TextNote::StreamLeak,
            default,
        };
    }

    // 闸四：行号段。
    if s.segkind != s.want_seg() {
        return Outcome {
            stage: Stage::Segment,
            code,
            value,
            note: TextNote::SegmentWrong,
            default,
        };
    }

    // 闸五：回显逐字。空词、含控制字符、超上限、非逐字都在这一闸。
    if word_is_empty(s.word)
        || text_len(s.word) > WORD_MAX
        || has_control(s.word)
        || !s.echo_form().is_verbatim()
    {
        return Outcome {
            stage: Stage::Echo,
            code,
            value,
            note: TextNote::EchoMangled,
            default,
        };
    }

    // 闸六：包裹档。
    if !s.quote_matches() {
        return Outcome {
            stage: Stage::Quote,
            code,
            value,
            note: TextNote::QuoteWrong,
            default,
        };
    }

    // 闸七：句族、locale、strerror 表。
    if !s.phrase.in_table() || !s.flavor.allows(s.phrase) {
        return Outcome {
            stage: Stage::Phrase,
            code,
            value,
            note: TextNote::PhraseWrong,
            default,
        };
    }
    if !s.locale.is_english() || !s.tail.is_table() {
        return Outcome {
            stage: Stage::Phrase,
            code,
            value,
            note: TextNote::LocaleWrong,
            default,
        };
    }
    if s.tok_required() {
        if !s.tok.in_table() || !s.flavor.allows_tail(s.tail) {
            return Outcome {
                stage: Stage::Phrase,
                code,
                value,
                note: TextNote::PhraseWrong,
                default,
            };
        }
    } else if s.tok != ErrnoTok::Absent || s.tail != ErrnoTail::None {
        return Outcome {
            stage: Stage::Phrase,
            code,
            value,
            note: TextNote::PhraseWrong,
            default,
        };
    }

    // 闸八：联签。
    if !s.sign.is_agree() {
        return Outcome {
            stage: Stage::Sign,
            code,
            value,
            note: TextNote::SignBreak,
            default,
        };
    }

    // 收尾：两种形式必须同一份字节。
    if !same_text(line_of(s).as_str(), s.body) {
        return Outcome {
            stage: Stage::Sign,
            code,
            value,
            note: TextNote::ChanSplit,
            default,
        };
    }

    Outcome {
        stage: Stage::Sign,
        code,
        value,
        note: note_of_phrase(s.phrase),
        default,
    }
}
// ---------------------------------------------------------------------------
// 文案构造子（逐字复现真机这一行）
// ---------------------------------------------------------------------------

/// 行首归属：`argv[0]` 的 basename 加冒号。
///
/// 真机上 `argv[0]` 常是全路径（`/bin/sh`、`/sbin/init` 起的 ash 是 `sh`），
/// shell 打的是 basename。这一位是全行的第一串字节，写错整行就错——**但它是
/// 派生量**，不是自由量：`argv[0]` 定了它就只能定，这里不许"看起来像"。
pub fn msg_prefix(argv0: &str) -> String {
    let mut s = basename_of(argv0);
    s.push(':');
    s
}

/// 中段（只有 `can't open ` 一族有）。
pub fn msg_mid(p: Phrase) -> &'static str {
    p.mid()
}

/// 行号段字节。
pub fn msg_segment(k: SegKind, line_no: u32) -> String {
    k.span(line_no)
}

/// 包裹后的回显字节。
pub fn msg_wrap(q: Quote, echo: &str) -> String {
    q.wrap(echo)
}

/// 尾句字节：静态尾句 +(需要时) strerror 原文。
pub fn msg_tail_of(p: Phrase, tok: ErrnoTok, tail: ErrnoTail) -> String {
    let mut s = String::new();
    s.push_str(p.static_tail());
    if p.uses_strerror() {
        s.push_str(strerror_text(tok, tail));
    }
    s
}

/// 第三形式：把五段字节**逐段**接起来再加换行。
///
/// 与 `line_of` 的区别在输入形状：`line_of` 吃 `Shape`（字段自取），本函数
/// 吃五段已经算好的字符串。两条独立路线加上语料表里的字面量，一共三种形式，
/// 任一处笔误都会在交叉校验里露出来。
pub fn msg_line(prefix: &str, seg: &str, mid: &str, wrapped: &str, tail: &str) -> String {
    let mut s = String::new();
    s.push_str(prefix);
    s.push(' ');
    s.push_str(seg);
    s.push_str(mid);
    s.push_str(wrapped);
    s.push_str(tail);
    s.push('\n');
    s
}

/// 去掉行末换行（`\n` 与 `\r\n` 都按一行的收尾处理）。
pub fn trim_eol(s: &str) -> &str {
    let b = s.as_bytes();
    if b.is_empty() {
        return s;
    }
    let mut end = b.len();
    while end > 0usize {
        match b.get(end - 1usize) {
            Some(c) => {
                if *c == b'\n' || *c == b'\r' {
                    end = end.saturating_sub(1usize);
                } else {
                    break;
                }
            }
            None => break,
        }
    }
    match s.get(..end) {
        Some(x) => x,
        None => s,
    }
}

/// 现场的第二形式（整行原文，去行末换行）。
pub fn msg_body_trim(s: &Shape) -> &'static str {
    trim_eol(s.body)
}

/// 现场的第一形式（语法部件拼装，去行末换行）。
pub fn msg_line_trim(s: &Shape) -> String {
    let mut out = String::from(msg_body_scratch(s.line().as_str()));
    out.push_str("");
    out
}

/// 内部用的去换行中转（`trim_eol` 吃 `String` 时借用要落地一次）。
fn msg_body_scratch(s: &str) -> &str {
    trim_eol(s)
}

/// 两个字符串首个不同字节的下标；等长前缀相同则返回较短者的长度。
pub fn msg_diff_pos(a: &str, b: &str) -> u32 {
    let x = a.as_bytes();
    let y = b.as_bytes();
    let mut i = 0usize;
    while i < x.len() && i < y.len() {
        let ca = match x.get(i) { Some(v) => *v, None => break };
        let cb = match y.get(i) { Some(v) => *v, None => break };
        if ca != cb {
            return i as u32;
        }
        i += 1;
    }
    if x.len() == y.len() {
        return 0;
    }
    i as u32
}

/// 行的位图：`头/回显/尾/总` 四段字节数。
pub fn msg_bytes_map(s: &Shape) -> String {
    let head = s.line_head();
    let line = s.line();
    let headlen = text_len(head.as_str());
    let echolen = s.quote.wrap(s.echo).len();
    let total = text_len(line.as_str());
    let mut out = dec_text(headlen as u32);
    out.push('/');
    out.push_str(dec_text(echolen as u32).as_str());
    out.push('/');
    let tailpart = total.saturating_sub(headlen).saturating_sub(echolen);
    out.push_str(dec_text(tailpart as u32).as_str());
    out.push('/');
    out.push_str(dec_text(total as u32).as_str());
    out
}

/// 三种形式是否两两相同（第一拼装 / 逐段拼装 / 字面原文）。
pub fn forms_triple(s: &Shape) -> [bool; 3] {
    let route_a = s.line();
    let route_b = msg_line(
        s.prefix,
        msg_segment(s.segkind, s.line_no).as_str(),
        msg_mid(s.phrase),
        msg_wrap(s.quote, s.echo).as_str(),
        msg_tail_of(s.phrase, s.tok, s.tail).as_str(),
    );
    let literal = s.body;
    [
        same_text(route_a.as_str(), literal),
        same_text(route_b.as_str(), literal),
        same_text(route_a.as_str(), route_b.as_str()),
    ]
}

/// 状态面文本：整行原文 + 四件判定事实。
pub fn message(s: &Shape, o: &Outcome) -> String {
    let mut t = String::from(msg_body_trim(s));
    t.push_str(" note=");
    t.push_str(o.note.name());
    t.push_str(" gate=");
    t.push_str(o.stage.name());
    t.push_str(" bytes=");
    t.push_str(dec_text(o.value).as_str());
    t.push_str(" errno=");
    t.push_str(ErrnoTok::of(o.code).name());
    t.push_str(" sign=");
    t.push_str(s.sign.name());
    t.push_str(" ok=");
    t.push_str(bool_text(o.ok()));
    t
}

/// 状态面尾行：第一形式 + locale/strerror 表档。
pub fn message_tail(s: &Shape) -> String {
    let mut t = msg_line_trim(s);
    t.push_str(" | ");
    t.push_str(s.locale.name());
    t.push(':');
    t.push_str(s.tail.name());
    t
}

// ---------------------------------------------------------------------------
// 申报与用例
// ---------------------------------------------------------------------------

/// 一槽申报。
///
/// 五个字段与 `Outcome` 逐位同名：这样"申报"才是"对同一组事实的另一种说法"，
/// 而不是另一套判据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 申报的闸段（规范行恒 `Sign`）。
    pub stage: Stage,
    /// 申报的内部 errno。
    pub code: i32,
    /// 申报的整行字节数。
    pub value: u32,
    /// 申报的注解。
    pub note: TextNote,
    /// 申报的联签位。
    pub default: bool,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim {
            stage: Stage::Sign,
            code: CH_ENOENT,
            value: 0,
            note: TextNote::PlainNotFound,
            default: true,
        }
    }

    /// 是否申报"这是一行本卡表内的规范文案"。
    ///
    /// 与 `Outcome::ok()` 同构：闸走满 **且** 注解属于语法表四档。
    /// 只看注解会把"注解写对但归因停在第四闸"的自相矛盾当成成功。
    pub fn success(&self) -> bool {
        self.stage == Stage::Sign && self.note.in_grammar()
    }

    /// 是否申报 127 联签。
    pub fn claims_signed(&self) -> bool {
        self.default
    }

    /// 注解蕴含的句族（违约注解返回 `None`）。
    pub fn phrase(&self) -> Option<Phrase> {
        self.note.phrase()
    }

    /// 申报自身是否自洽。
    ///
    /// 规范申报要求：闸走满、内部 errno 与句族蕴含一致、联签位与句族蕴含一致。
    /// 违约申报要求：不许把闸段写到 `Sign`（`ChanSplit`/`SignBreak` 两类除外——
    /// 它们本来就在最后一道闸上判死）。
    pub fn self_consistent(&self) -> bool {
        match self.phrase() {
            Some(p) => {
                if self.stage != Stage::Sign {
                    return false;
                }
                let tok = ErrnoTok::of(self.code);
                if p.implied_errno(tok) != self.code {
                    return false;
                }
                if p.uses_strerror() {
                    if !tok.in_table() {
                        return false;
                    }
                } else if self.code != CH_ENOENT {
                    // 不带 strerror 的行只有一个合法的内部蕴含数：ENOENT。
                    // 报 EACCES 的 `not found` 行申报属于自相矛盾。
                    return false;
                }
                self.default == (p.implied_exit(tok) == EXIT_NOT_FOUND)
            }
            None => {
                if self.stage == Stage::Sign {
                    return matches!(
                        self.note,
                        TextNote::ChanSplit | TextNote::SignBreak
                    );
                }
                true
            }
        }
    }

    /// 申报指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.stage.name());
        h = fold_i32(h, self.code);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        fold_word(h, self.default as u64)
    }

    /// 申报文本。
    pub fn text(&self) -> String {
        let mut s = String::from(self.stage.name());
        s.push(':');
        s.push_str(self.note.name());
        s.push(':');
        s.push_str(dec_text(self.value).as_str());
        s.push(':');
        s.push_str(bool_text(self.default));
        s
    }
}

/// 一槽用例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 现场。
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
// 语料（14 槽 = 8 活 + 6 死）
// ---------------------------------------------------------------------------

/// 8 行真机规范文案 + 6 种"同一行字、六种误报"。
///
/// 后六槽的**整行原文互不相同**（applet 行、小写化行、缺行号段行、未知方言
/// 行、规范 ash 行、规范 dash 行），缺陷却分别是六种申报维——这正是本卡的红档
/// 显式：一行字可以六种说法错法，行本身没有区别。
pub const SHAPES: [Shape; CORPUS] = [
    // 0 busybox ash 裸名 notfound：本卡主锚。前缀取自 argv[0] 的 basename，
    //   无行号段、无引号、无 strerror，内部蕴含 ENOENT，联签 127。
    Shape {
        tag: "ash-notfound-bare",
        argv0: "/bin/sh",
        prefix: "sh:",
        word: "foo",
        echo: "foo",
        flavor: Flavor::Ash,
        interactive: false,
        line_no: 1,
        stream: Stream::Stderr,
        phrase: Phrase::NotFound,
        segkind: SegKind::None,
        quote: Quote::Bare,
        tok: ErrnoTok::Absent,
        tail: ErrnoTail::None,
        locale: Locale::C,
        sign: Sign::Agree,
        body: ASH_FOO_LINE,
    },
    // 1 GNU bash 非交互（`bash -c`）：`line 1: ` 关键字号段 + `command not found`。
    Shape {
        tag: "bash-c-command-not-found",
        argv0: "bash",
        prefix: "bash:",
        word: "foo",
        echo: "foo",
        flavor: Flavor::Bash,
        interactive: false,
        line_no: 1,
        stream: Stream::Stderr,
        phrase: Phrase::CommandNotFound,
        segkind: SegKind::LineWord,
        quote: Quote::Bare,
        tok: ErrnoTok::Absent,
        tail: ErrnoTail::None,
        locale: Locale::C,
        sign: Sign::Agree,
        body: BASH_C_LINE,
    },
    // 2 GNU bash 交互态：同一句族、行号段整体消失。这是"同一个 shell 两种形态
    //    两种字节"的唯一一档，本卡的第四闸靠它才立得住。
    Shape {
        tag: "bash-i-command-not-found",
        argv0: "/bin/bash",
        prefix: "bash:",
        word: "foo",
        echo: "foo",
        flavor: Flavor::Bash,
        interactive: true,
        line_no: 1,
        stream: Stream::Stderr,
        phrase: Phrase::CommandNotFound,
        segkind: SegKind::None,
        quote: Quote::Bare,
        tok: ErrnoTok::Absent,
        tail: ErrnoTail::None,
        locale: Locale::C,
        sign: Sign::Agree,
        body: BASH_I_LINE,
    },
    // 3 dash：裸行号段（永远带，与是否交互无关）。
    Shape {
        tag: "dash-numbered-notfound",
        argv0: "/bin/dash",
        prefix: "dash:",
        word: "foo",
        echo: "foo",
        flavor: Flavor::Dash,
        interactive: false,
        line_no: 1,
        stream: Stream::Stderr,
        phrase: Phrase::NotFound,
        segkind: SegKind::BareLine,
        quote: Quote::Bare,
        tok: ErrnoTok::Absent,
        tail: ErrnoTail::None,
        locale: Locale::C,
        sign: Sign::Agree,
        body: DASH_FOO_LINE,
    },
    // 4 busybox ash 打不开脚本：唯一带 `can't open ` 中段 + 单引号 + strerror
    //    的规范行；蕴含 ENOENT 且退 127。
    Shape {
        tag: "ash-cant-open-glibc",
        argv0: "ash",
        prefix: "ash:",
        word: "nosuch",
        echo: "nosuch",
        flavor: Flavor::Ash,
        interactive: false,
        line_no: 1,
        stream: Stream::Stderr,
        phrase: Phrase::CantOpen,
        segkind: SegKind::None,
        quote: Quote::Single,
        tok: ErrnoTok::Enoent,
        tail: ErrnoTail::Glibc,
        locale: Locale::C,
        sign: Sign::Agree,
        body: "ash: can't open 'nosuch': No such file or directory\n",
    },
    // 5 GNU bash 对含斜杠的词走 C 库原文：尾句不是 command not found，但
    //    蕴含的退出数仍是 127——文案族属与数字族属**不同轴**的活样本。
    Shape {
        tag: "bash-slash-strerror",
        argv0: "/bin/bash",
        prefix: "bash:",
        word: "/tmp/nosuch",
        echo: "/tmp/nosuch",
        flavor: Flavor::Bash,
        interactive: false,
        line_no: 1,
        stream: Stream::Stderr,
        phrase: Phrase::SysError,
        segkind: SegKind::LineWord,
        quote: Quote::Bare,
        tok: ErrnoTok::Enoent,
        tail: ErrnoTail::Glibc,
        locale: Locale::C,
        sign: Sign::Agree,
        body: "bash: line 1: /tmp/nosuch: No such file or directory\n",
    },
    // 6 GNU bash 对目录报 EISDIR：同一句族、不同 errno 词，蕴含 126 档，
    //    于是联签位为假——本卡唯一一条"规范行但不联签"的槽。
    Shape {
        tag: "bash-slash-isdir-126",
        argv0: "bash",
        prefix: "bash:",
        word: "/tmp",
        echo: "/tmp",
        flavor: Flavor::Bash,
        interactive: true,
        line_no: 1,
        stream: Stream::Stderr,
        phrase: Phrase::SysError,
        segkind: SegKind::None,
        quote: Quote::Bare,
        tok: ErrnoTok::Eisdir,
        tail: ErrnoTail::Glibc,
        locale: Locale::C,
        sign: Sign::Agree,
        body: "bash: /tmp: Is a directory\n",
    },
    // 7 大小写混合的命令字逐字回显：这一槽专门用来钉住"回显不是归一化"。
    //    把词小写化在这里只改尾句里的字节数，数字通道完全看不出来。
    Shape {
        tag: "ash-mixedcase-echo",
        argv0: "/bin/sh",
        prefix: "sh:",
        word: "FooBAR",
        echo: "FooBAR",
        flavor: Flavor::Ash,
        interactive: false,
        line_no: 1,
        stream: Stream::Stderr,
        phrase: Phrase::NotFound,
        segkind: SegKind::None,
        quote: Quote::Bare,
        tok: ErrnoTok::Absent,
        tail: ErrnoTail::None,
        locale: Locale::C,
        sign: Sign::Agree,
        body: "sh: FooBAR: not found\n",
    },
    // 8 归属缺陷：busybox 本体的 applet 行（UNX-F11661/F11663 的现场本体）。
    //    申报把它当成 shell 的规范 notfound 行——成败互报。
    Shape {
        tag: "estate-swap-applet-line",
        argv0: "/usr/bin/busybox",
        prefix: "busybox:",
        word: "foo",
        echo: "foo",
        flavor: Flavor::BusyboxApplet,
        interactive: false,
        line_no: 1,
        stream: Stream::Stderr,
        phrase: Phrase::AppletNotFound,
        segkind: SegKind::None,
        quote: Quote::Single,
        tok: ErrnoTok::Absent,
        tail: ErrnoTail::None,
        locale: Locale::C,
        sign: Sign::Absent,
        body: "busybox: 'foo' applet not found\n",
    },
    // 9 闸段缺陷：词被小写化（`Foo → foo`）。申报说死在行号段闸上——
    //    那一闸这行是干净的，真正歪的是第五闸。
    Shape {
        tag: "stage-swap-lowered-echo",
        argv0: "/bin/sh",
        prefix: "sh:",
        word: "Foo",
        echo: "foo",
        flavor: Flavor::Ash,
        interactive: false,
        line_no: 1,
        stream: Stream::Stderr,
        phrase: Phrase::NotFound,
        segkind: SegKind::None,
        quote: Quote::Bare,
        tok: ErrnoTok::Absent,
        tail: ErrnoTail::None,
        locale: Locale::C,
        sign: Sign::Agree,
        body: "sh: foo: not found\n",
    },
    // 10 通道缺陷：dash 少打了行号段。行本身还是规范形状的 notfound 行，
    //    但 errno 通道被申报写成 EACCES——蕴含数错。
    Shape {
        tag: "code-swap-dash-no-seg",
        argv0: "dash",
        prefix: "dash:",
        word: "foo",
        echo: "foo",
        flavor: Flavor::Dash,
        interactive: false,
        line_no: 1,
        stream: Stream::Stderr,
        phrase: Phrase::NotFound,
        segkind: SegKind::None,
        quote: Quote::Bare,
        tok: ErrnoTok::Absent,
        tail: ErrnoTail::None,
        locale: Locale::C,
        sign: Sign::Agree,
        body: "dash: foo: not found\n",
    },
    // 11 归属缺陷：不认识的方言（zsh）。字节很像、族属名字也像，本卡拒绝对判；
    //    申报却给了一行少 3 个字节的长度。
    Shape {
        tag: "value-swap-unknown-flavor",
        argv0: "zsh",
        prefix: "zsh:",
        word: "foo",
        echo: "foo",
        flavor: Flavor::Unknown,
        interactive: false,
        line_no: 1,
        stream: Stream::Stderr,
        phrase: Phrase::NotFound,
        segkind: SegKind::None,
        quote: Quote::Bare,
        tok: ErrnoTok::Absent,
        tail: ErrnoTail::None,
        locale: Locale::C,
        sign: Sign::Agree,
        body: "zsh: command not found: foo\n",
    },
    // 12 注解缺陷：规范 ash 行被报成 bash 的 `command not found` 族。
    Shape {
        tag: "note-swap-ash-as-bash",
        argv0: "/bin/sh",
        prefix: "sh:",
        word: "nosuchtool",
        echo: "nosuchtool",
        flavor: Flavor::Ash,
        interactive: false,
        line_no: 1,
        stream: Stream::Stderr,
        phrase: Phrase::NotFound,
        segkind: SegKind::None,
        quote: Quote::Bare,
        tok: ErrnoTok::Absent,
        tail: ErrnoTail::None,
        locale: Locale::C,
        sign: Sign::Agree,
        body: "sh: nosuchtool: not found\n",
    },
    // 13 联签缺陷：规范 dash 行、第三行号，B30 那边记的是 127，申报把联签位抹掉。
    Shape {
        tag: "default-swap-dash-unsigned",
        argv0: "/bin/dash",
        prefix: "dash:",
        word: "missing-thing",
        echo: "missing-thing",
        flavor: Flavor::Dash,
        interactive: false,
        line_no: 3,
        stream: Stream::Stderr,
        phrase: Phrase::NotFound,
        segkind: SegKind::BareLine,
        quote: Quote::Bare,
        tok: ErrnoTok::Absent,
        tail: ErrnoTail::None,
        locale: Locale::C,
        sign: Sign::Agree,
        body: "dash: 3: missing-thing: not found\n",
    },
];

/// 与 `SHAPES` 一一对应的申报。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim { stage: Stage::Sign, code: CH_ENOENT, value: 19, note: TextNote::PlainNotFound, default: true },
    Claim { stage: Stage::Sign, code: CH_ENOENT, value: 37, note: TextNote::CommandNotFound, default: true },
    Claim { stage: Stage::Sign, code: CH_ENOENT, value: 29, note: TextNote::CommandNotFound, default: true },
    Claim { stage: Stage::Sign, code: CH_ENOENT, value: 24, note: TextNote::PlainNotFound, default: true },
    Claim { stage: Stage::Sign, code: CH_ENOENT, value: 53, note: TextNote::CantOpen, default: true },
    Claim { stage: Stage::Sign, code: CH_ENOENT, value: 53, note: TextNote::SysError, default: true },
    Claim { stage: Stage::Sign, code: CH_EISDIR, value: 27, note: TextNote::SysError, default: false },
    Claim { stage: Stage::Sign, code: CH_ENOENT, value: 22, note: TextNote::PlainNotFound, default: true },
    Claim { stage: Stage::Sign, code: CH_ENOENT, value: 32, note: TextNote::PlainNotFound, default: true },
    Claim { stage: Stage::Segment, code: CH_ENOENT, value: 19, note: TextNote::SegmentWrong, default: true },
    Claim { stage: Stage::Segment, code: CH_EACCES, value: 21, note: TextNote::SegmentWrong, default: true },
    Claim { stage: Stage::Flavor, code: CH_ENOENT, value: 25, note: TextNote::NotOurLine, default: true },
    Claim { stage: Stage::Sign, code: CH_ENOENT, value: 26, note: TextNote::CommandNotFound, default: true },
    Claim { stage: Stage::Sign, code: CH_ENOENT, value: 34, note: TextNote::PlainNotFound, default: false },
];

/// 后 6 槽的期望缺陷（次序固定：Estate/Stage/Code/Value/Note/Default）。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

/// 后 6 槽的缺陷码（六个互不相同的 `E*`）。
pub const REJECT_CODES: [u32; REJECT] = [
    TX_ESTATE,
    TX_ESTAGE,
    TX_ECODE,
    TX_EVALUE,
    TX_ENOTE,
    TX_EDEFAULT,
];

/// 账本 8 个成格的值：总数、成、败、语法面槽、结构面槽、族属面槽、归属面槽、红档率‰。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 7, 2, 3, 2, 929];

// ---------------------------------------------------------------------------
// 期望表（实际结果的第二种形式——任一处笔误都要露出来）
// ---------------------------------------------------------------------------

/// 每槽期望闸段（形式二：直接写 `Stage`，与 `define` 的产物交叉校验）。
const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Sign,
    Stage::Sign,
    Stage::Sign,
    Stage::Sign,
    Stage::Sign,
    Stage::Sign,
    Stage::Sign,
    Stage::Sign,
    Stage::Flavor,
    Stage::Echo,
    Stage::Segment,
    Stage::Flavor,
    Stage::Sign,
    Stage::Sign,
];

/// 每槽期望内部 errno 通道。
const WANT_CODE: [i32; CORPUS] = [
    CH_ENOENT,
    CH_ENOENT,
    CH_ENOENT,
    CH_ENOENT,
    CH_ENOENT,
    CH_ENOENT,
    CH_EISDIR,
    CH_ENOENT,
    CH_ENONE,
    CH_ENOENT,
    CH_ENOENT,
    CH_ENOENT,
    CH_ENOENT,
    CH_ENOENT,
];

/// 每槽期望整行字节数（含行末换行）。
const WANT_BYTES: [u32; CORPUS] = [19, 37, 29, 24, 53, 53, 27, 22, 32, 19, 21, 28, 26, 34];

/// 每槽期望回显偏移。
///
/// 记的是**第一形式**里回显位的起始字节下标（含前引号之后）。槽 11 那条不
/// 认识的方言行，第一形式与原文本就不是同一句话——偏移对表照记不误，因为
/// 该槽的缺陷正落在"方言不认识"这一闸上，位置维只是复写现场。
const WANT_NDX: [u32; CORPUS] = [4, 14, 6, 9, 17, 14, 6, 4, 10, 4, 6, 5, 4, 9];

/// 每槽期望注解。
const WANT_NOTE: [TextNote; CORPUS] = [
    TextNote::PlainNotFound,
    TextNote::CommandNotFound,
    TextNote::CommandNotFound,
    TextNote::PlainNotFound,
    TextNote::CantOpen,
    TextNote::SysError,
    TextNote::SysError,
    TextNote::PlainNotFound,
    TextNote::NotOurLine,
    TextNote::EchoMangled,
    TextNote::SegmentWrong,
    TextNote::NotOurLine,
    TextNote::PlainNotFound,
    TextNote::PlainNotFound,
];

/// 每槽期望联签位。
const WANT_DEFAULT: [bool; CORPUS] = [
    true, true, true, true, true, true, false, true, false, true, true, true, true, true,
];

/// 每槽标签。
const WANT_TAGS: [&'static str; CORPUS] = [
    "ash-notfound-bare",
    "bash-c-command-not-found",
    "bash-i-command-not-found",
    "dash-numbered-notfound",
    "ash-cant-open-glibc",
    "bash-slash-strerror",
    "bash-slash-isdir-126",
    "ash-mixedcase-echo",
    "estate-swap-applet-line",
    "stage-swap-lowered-echo",
    "code-swap-dash-no-seg",
    "value-swap-unknown-flavor",
    "note-swap-ash-as-bash",
    "default-swap-dash-unsigned",
];

/// 每槽期望缺陷维序（活槽恒 `Ok`，死槽按 `REJECT_FAULTS` 次序）。
pub fn expected(i: usize) -> Option<Fault> {
    if i >= CORPUS {
        return None;
    }
    if i < ACCEPT {
        return Some(Fault::Ok);
    }
    REJECT_FAULTS.get(i - ACCEPT).copied()
}

/// 语料表。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case::blank(); CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        if let (Some(s), Some(c)) = (SHAPES.get(i), CLAIMS.get(i)) {
            if let Some(slot) = out.get_mut(i) {
                *slot = Case { shape: *s, claim: *c };
            }
        }
        i += 1;
    }
    out
}

/// 取一槽现场。
pub fn shape_at(i: usize) -> Option<Shape> {
    SHAPES.get(i).copied()
}

/// 取一槽实际结果。
pub fn outcome_at(i: usize) -> Option<Outcome> {
    match SHAPES.get(i) {
        Some(s) => Some(define(s)),
        None => None,
    }
}

/// 取一槽申报。
pub fn claim_at(i: usize) -> Option<Claim> {
    CLAIMS.get(i).copied()
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
        return Err(TX_ENOBS);
    }
    Ok(())
}

/// 对判。
///
/// 判序固定为 Estate → Stage → Code → Value → Note → Default → Ok。
/// Estate 最先的理由与本卡的邻卡 F11664 相同但成因不同：这里最贵的一类误报
/// 是**把不是 shell 说的行报成 shell 的规范行**（槽 8）。如果先比闸段，一条
/// "applet 行报成 `Sign`/`PlainNotFound`" 的申报会因为闸段也错而被降级成
/// "闸段笔误"，把归属越权洗成了观测手滑——两张卡的分工就再也追不回来了。
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

/// 该槽除申报外的**行内自洽**缺陷（与 `judge` 正交的第二把尺）。
///
/// `judge` 问"申报对不对"，本函数问"这一行本身能不能被本卡读成一句话"：
/// 归属面（不是这台 shell 的行）与现场两形式背离、申报自相矛盾，都记 Channel。
/// 有了这把尺，"错得自洽"就再也不能冒充"对"。
pub fn channel_fault(c: &Case) -> Fault {
    let a = c.actual();
    if a.is_foreign() {
        return Fault::Channel;
    }
    if !c.shape.forms_agree() {
        return Fault::Channel;
    }
    if !c.claim.self_consistent() {
        return Fault::Channel;
    }
    Fault::Ok
}

/// 逐槽释义。
pub fn detail(c: &Case) -> String {
    let o = c.actual();
    let mut s = String::from(c.shape.tag);
    s.push_str(" ");
    s.push_str(message(&c.shape, &o).as_str());
    s.push_str(" claim=");
    s.push_str(c.claim.stage.name());
    s.push(':');
    s.push_str(c.claim.note.name());
    s.push(':');
    s.push_str(dec_text(c.claim.value).as_str());
    s.push_str(" fault=");
    s.push_str(fault_short(judge(c)));
    s
}

/// 释义尾行（位置通道的官方形态）。
pub fn detail_tail(c: &Case) -> String {
    let o = c.actual();
    let mut s = String::from("map ");
    s.push_str(msg_bytes_map(&c.shape).as_str());
    s.push_str(" echo@");
    s.push_str(dec_text(c.shape.echo_index()).as_str());
    s.push_str(" signed ");
    s.push_str(bool_text(o.default));
    s
}
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
/// 分档只看**实际结果**（这一行歪在哪一面），申报正确与否单独计。本卡六格
/// 误报分散在三个面上：归属面两条（applet 行、未知方言）、结构面一条（小写
/// 化回显）、族属面一条（缺行号段的 errno 写错）……这种"红档摊开"的形状与
/// F11664 恰好相反——那张的六条误报全挤在 127 一档里。两卡的矩阵因此不可能
/// 用同一段渲染代码糊过去。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行（grammar/structure/family/foreign）。
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
            let t = c.actual().tier().index();
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
                n = n.saturating_add(r.total);
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
                n = n.saturating_add(r.good);
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

    /// 红档总数。
    pub fn reds(&self) -> usize {
        self.slots().saturating_sub(self.goods())
    }

    /// 语法面（本卡表内的两种 notfound 行）红档数。
    pub fn grammar_reds(&self) -> usize {
        match self.rows.get(Tier::Grammar.index()) {
            Some(r) => r.red(),
            None => 0,
        }
    }

    /// 归属面红档数（本卡拒绝对判的行）。
    pub fn foreign_reds(&self) -> usize {
        match self.rows.get(Tier::Foreign.index()) {
            Some(r) => r.red(),
            None => 0,
        }
    }

    /// 渲染全矩阵。
    pub fn render(&self) -> String {
        let tiers = [Tier::Grammar, Tier::Structure, Tier::Family, Tier::Foreign];
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

/// 全矩阵逐字文本（供判据对表）。
pub const MATRIX_LIT: &str = "grammar total=7 good=5 red=2\nstructure total=2 good=0 red=2\nfamily total=3 good=3 red=0\nforeign total=2 good=0 red=2\n";

// ---------------------------------------------------------------------------
// 汇总报表
// ---------------------------------------------------------------------------

/// 语料汇总。
///
/// 四个档字段记的是**覆盖槽数**而不是成格数，与 F11664 有意不同：本卡的红档
/// 全部落在结构面与归属面，那两档的成格数恒为 0，若照抄那张的口径，账本第
/// 5~7 格就失去鉴别力（注入任何缺陷都看不出差别）。
#[derive(Clone, Copy, Debug)]
pub struct Report {
    /// 总槽数。
    pub total: u32,
    /// 申报正确数。
    pub accept: u32,
    /// 申报错误数。
    pub reject: u32,
    /// 语法面槽数。
    pub grammar: u32,
    /// 结构面槽数。
    pub structure: u32,
    /// 族属面槽数。
    pub family: u32,
    /// 归属面槽数。
    pub foreign: u32,
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
            Err(TX_ECOUNT)
        }
    }

    /// 四档槽数合计（必须回到总槽数——覆盖不平就是矩阵不平）。
    pub fn tier_sum(&self) -> u32 {
        self.grammar
            .saturating_add(self.structure)
            .saturating_add(self.family)
            .saturating_add(self.foreign)
    }

    /// 本卡语法表收下的行数（语法面 + 族属面）。
    pub fn table_cells(&self) -> u32 {
        self.grammar.saturating_add(self.family)
    }

    /// 表内行占比（百分数，整型）。
    pub fn table_share(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.table_cells() * 100) / self.total
    }

    /// 成功格数（申报正确数）。
    pub fn success_cells(&self) -> u32 {
        self.accept
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("notfound-text corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" grammar=");
        s.push_str(u32_text(self.grammar).as_str());
        s.push_str(" structure=");
        s.push_str(u32_text(self.structure).as_str());
        s.push_str(" family=");
        s.push_str(u32_text(self.family).as_str());
        s.push_str(" foreign=");
        s.push_str(u32_text(self.foreign).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 全绿渲染文本（逐字钉住）。
const RENDER_OK: &str = "notfound-text corpus=14 accept=8 reject=6 grammar=7 structure=2 family=3 foreign=2 match=14 rate=1000ppm\n";

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 可注入一槽缺陷的全跑。
///
/// 注入把"期望表"本身也放进被测面：强行宣称某槽犯了另一种缺陷，matched
/// 必须掉、rate 必须离开 1000。若注入后仍报全绿，说明 matched 是硬编码的。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(TX_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut grammar = 0u32;
    let mut structure = 0u32;
    let mut family = 0u32;
    let mut foreign = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(TX_ENOBS),
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
            None => return Err(TX_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
        } else {
            reject += 1;
        }
        // 档字段按实际结果计覆盖，与注入无关：注入只动申报面，不动现场面。
        match a.tier() {
            Tier::Grammar => grammar += 1,
            Tier::Structure => structure += 1,
            Tier::Family => family += 1,
            Tier::Foreign => foreign += 1,
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, grammar, structure, family, foreign, matched })
}

/// 注入第 0 槽缺陷后的红档率（账本第 8 格的来源）。
pub fn red_rate() -> u32 {
    match run_with(Some((0usize, Fault::Code))) {
        Ok(r) => r.rate(),
        Err(_) => 0,
    }
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门。
///
/// 八道闸 `prefix → flavor → stream → segment → echo → quote → phrase → sign`
/// 只许向前。回退（已经判死在尾句闸，又声称回到行号段闸复核）说明申报者不是
/// 按行从左到右读的，而是**先看结论再补过程**——这类申报在双机对照里最危险：
/// 整行字节与 errno 都完全合理，只有归因链是假的。
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
            return Err(TX_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(TX_ESTAGE);
        }
        self.last = o;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }

    /// 关闭，返回段数。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(TX_ESTATE);
        }
        self.closed = true;
        Ok(self.steps)
    }

    /// 段数。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// 上一段次序。
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
    /// 槽位（必须严格等于当前长度才可登记）。
    pub slot: u8,
    /// 操作码（八闸各一值，本卡的"操作"就是"验哪道闸"）。
    pub op: u8,
    /// 是否成功格。
    pub ok: bool,
    /// 缺陷码（成功格恒零）。
    pub code: u32,
    /// 观测值（失败格恒零）。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }

    /// 格指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.slot as u64);
        h = fold_word(h, self.op as u64);
        h = fold_word(h, self.ok as u64);
        h = fold_word(h, self.code as u64);
        fold_word(h, self.value as u64)
    }

    /// 格文本。
    pub fn text(&self) -> String {
        let mut s = String::from("slot ");
        s.push_str(dec_text(self.slot as u32).as_str());
        s.push_str(" gate ");
        s.push_str(match Stage::from_order((self.op as u32).saturating_sub(1) as usize) {
            Some(v) => v.name(),
            None => Stage::Sign.name(),
        });
        s.push_str(" ok ");
        s.push_str(bool_text(self.ok));
        s.push_str(" code ");
        s.push_str(hex_text(self.code, 5).as_str());
        s.push_str(" bytes ");
        s.push_str(dec_text(self.value).as_str());
        s
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

    /// 登记一格；槽位不等于当前长度、越界或已冻结一律拒绝并计数。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TX_ESLOT);
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
        self.len.saturating_sub(self.ok_count())
    }

    /// 逐格复核：槽位连续、成格值逐格等于 `VALS`、败格码逐格等于
    /// `REJECT_CODES` 且败格值必须为 0。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(TX_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(TX_EBAL),
            };
            if s.slot as usize != i {
                return Err(TX_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(TX_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(TX_EVALUE);
                        }
                    }
                    None => return Err(TX_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(TX_EVALUE);
                }
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(TX_EVALUE);
                        }
                    }
                    None => return Err(TX_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(TX_EBAL);
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
            return Err(TX_EBAL);
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
            Err(TX_ESEAL)
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

    /// 全账文本。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(g) = self.rows.get(i) {
                s.push_str(g.text().as_str());
                s.push('\n');
            }
            i += 1;
        }
        s
    }
}

/// 计数结果 → 样本。
fn count_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 建账本：7 格覆盖统计 + 1 格红档率（929）+ 6 格缺陷码。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let _ = l.register(count_cell(0, OP_PREFIX, r.map(|x| x.total)));
    let _ = l.register(count_cell(1, OP_STREAM, r.map(|x| x.accept)));
    let _ = l.register(count_cell(2, OP_FLAVOR, r.map(|x| x.reject)));
    let _ = l.register(count_cell(3, OP_ECHO, r.map(|x| x.grammar)));
    let _ = l.register(count_cell(4, OP_SEGMENT, r.map(|x| x.structure)));
    let _ = l.register(count_cell(5, OP_QUOTE, r.map(|x| x.family)));
    let _ = l.register(count_cell(6, OP_PHRASE, r.map(|x| x.foreign)));

    let red = run_with(Some((0usize, Fault::Code)));
    let _ = l.register(count_cell(7, OP_SIGN, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_PHRASE,
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
// 判据基元（13 条判据的私有谓词）
// ---------------------------------------------------------------------------

/// 码名回查（状态面文本用）。
pub fn code_name(code: u32) -> &'static str {
    match code {
        0 => "none",
        x if x == TX_ESTAGE => "estage",
        x if x == TX_ECODE => "ecode",
        x if x == TX_EVALUE => "evalue",
        x if x == TX_ENOTE => "enote",
        x if x == TX_EDEFAULT => "edefault",
        x if x == TX_ECHANNEL => "echannel",
        x if x == TX_ESTATE => "estate",
        x if x == TX_ECOUNT => "ecount",
        x if x == TX_ESLOT => "eslot",
        x if x == TX_ESEAL => "eseal",
        x if x == TX_ENOBS => "enobs",
        x if x == TX_EMATRIX => "ematrix",
        x if x == TX_EBAL => "ebal",
        _ => "unknown",
    }
}

/// 码释义（状态面文本用）。
pub fn code_desc(code: u32) -> &'static str {
    if code == 0 {
        return "一致";
    }
    match CODE_TABLE.get(0usize) {
        Some(_) => {
            let mut i = 0usize;
            while i < CODE_TABLE.len() {
                if let Some(w) = CODE_TABLE.get(i) {
                    if *w == code {
                        return match i {
                            0usize => "闸段判据与八闸纵线不符",
                            1usize => "文案蕴含的内部 errno 与申报不符",
                            2usize => "整行字节数与申报不符",
                            3usize => "注解（这一行是什么）与申报不符",
                            4usize => "联签位与 B30 的账不符",
                            5usize => "行首归属或行内两种形式背离",
                            6usize => "规范行与违约行互报",
                            7usize => "语料计数或覆盖不闭合",
                            8usize => "槽位越界或注入点不可达",
                            9usize => "指纹错",
                            10usize => "无观测或期望表缺项",
                            11usize => "矩阵不平",
                            12usize => "账本不平",
                            _ => "未知码",
                        };
                    }
                }
                i += 1;
            }
            "非本卡码"
        }
        None => "空码表",
    }
}

/// 第四形式：由 `argv[0]` 重新派生前缀再拼整行。
///
/// 与 `line_of` 的差别只在**前缀的来源**：`line_of` 用现场记录的 `prefix`
/// 字段（观测到的行首），本函数用 `argv[0]` 派生（应当出现的行首）。两者不
/// 等就说明这一行的归属写歪了——第一闸的判据正是这条式子。
pub fn line_text(s: &Shape) -> String {
    let mut out = msg_prefix(s.argv0);
    out.push(' ');
    out.push_str(msg_segment(s.segkind, s.line_no).as_str());
    out.push_str(msg_mid(s.phrase));
    out.push_str(msg_wrap(s.quote, s.echo).as_str());
    out.push_str(msg_tail_of(s.phrase, s.tok, s.tail).as_str());
    out.push('\n');
    out
}

/// 一行文案的结构体检（不进判定环，只用于语料自检）。
///
/// 规则全部从本卡的行形状来：以换行收尾且只有一个换行、有行首归属冒号、
/// 含本卡四族尾句之一、不含制表符与控制字符、长度在上限内。
pub fn lint_line(t: &str) -> bool {
    if t.is_empty() {
        return false;
    }
    if text_len(t) > LINE_MAX {
        return false;
    }
    if !ends_newline(t) {
        return false;
    }
    if newline_count(t) != 1usize {
        return false;
    }
    if has_control(t) {
        return false;
    }
    if colon_count(t) < 2usize {
        return false;
    }
    let tails = [
        ASH_TAIL,
        BASH_TAIL,
        APPLET_TAIL,
        STR_ENOENT,
        STR_EISDIR,
        STR_EACCES,
        BUSY_ENOENT,
        L10N_ZH,
    ];
    let mut i = 0usize;
    let mut hit = false;
    while i < tails.len() {
        if let Some(i) = tails.get(i) {
            if i != &"" && contains_text(t, i) {
                hit = true;
            }
        }
        // 循环自增：i 已被遮蔽，这里按计数推进
        if hit {
            break;
        }
    }
    hit
}

/// 缺陷维序。
pub fn fault_ndx(f: Fault) -> usize {
    match f {
        Fault::Ok => 0usize,
        Fault::Channel => 1usize,
        Fault::Estate => 2usize,
        Fault::Stage => 3usize,
        Fault::Code => 4usize,
        Fault::Value => 5usize,
        Fault::Note => 6usize,
        Fault::Default => 7usize,
        _ => 8usize,
    }
}

/// 维名。
pub fn dim_name(ndx: usize) -> &'static str {
    match ndx {
        0usize => "estate",
        1usize => "stage",
        2usize => "code",
        3usize => "value",
        4usize => "note",
        5usize => "default",
        _ => "none",
    }
}

/// 实际结果 → 同构申报（六维逐位相等，`judge` 必返回 `Ok`）。
fn mk_claim(o: &Outcome) -> Claim {
    Claim { stage: o.stage, code: o.code, value: o.value, note: o.note, default: o.default }
}

/// 注一维缺陷（只动指定通道）。
fn claim_of(c: &Case, f: Fault) -> Claim {
    let a = c.actual();
    let mut k = mk_claim(&a);
    match f {
        Fault::Estate => {
            if a.ok() {
                k.stage = Stage::Segment;
                k.note = TextNote::SegmentWrong;
            } else {
                k.stage = Stage::Sign;
                k.note = TextNote::PlainNotFound;
                if !a.default {
                    k.default = false;
                }
            }
        }
        Fault::Stage => k.stage = next_stage(a.stage),
        Fault::Code => k.code = other_code(a.code),
        Fault::Value => k.value = a.value.saturating_add(1u32),
        Fault::Note => k.note = flip_note(a.note),
        Fault::Default => k.default = !a.default,
        _ => {}
    }
    k
}

/// 闸段推进一位（注入"归因写歪"用）。
fn next_stage(s: Stage) -> Stage {
    match s {
        Stage::Prefix => Stage::Flavor,
        Stage::Flavor => Stage::Stream,
        Stage::Stream => Stage::Segment,
        Stage::Segment => Stage::Echo,
        Stage::Echo => Stage::Quote,
        Stage::Quote => Stage::Phrase,
        Stage::Phrase => Stage::Sign,
        Stage::Sign => Stage::Prefix,
    }
}

/// errno 通道换位（注入"蕴含数写歪"用）。
fn other_code(v: i32) -> i32 {
    match v {
        CH_ENOENT => CH_EACCES,
        CH_EACCES => CH_EISDIR,
        CH_EISDIR => CH_ENOENT,
        _ => CH_EINVAL,
    }
}

/// 注解换位（注入"这一行说成另一种行"用）。
fn flip_note(n: TextNote) -> TextNote {
    match n {
        TextNote::PlainNotFound => TextNote::CommandNotFound,
        TextNote::CommandNotFound => TextNote::PlainNotFound,
        TextNote::SysError => TextNote::CantOpen,
        TextNote::CantOpen => TextNote::SysError,
        _ => TextNote::NotOurLine,
    }
}

/// 申报与实际结果不同的维数。
fn dims_differ(c: &Case) -> usize {
    let a = c.actual();
    let k = c.claim;
    let mut n = 0usize;
    if a.ok() != k.success() {
        n += 1;
    }
    if a.stage != k.stage {
        n += 1;
    }
    if a.code != k.code {
        n += 1;
    }
    if a.value != k.value {
        n += 1;
    }
    if a.note != k.note {
        n += 1;
    }
    if a.default != k.default {
        n += 1;
    }
    n
}

/// 首个不同的维。
fn first_diff(c: &Case) -> usize {
    let a = c.actual();
    let k = c.claim;
    if a.ok() != k.success() {
        return 0usize;
    }
    if a.stage != k.stage {
        return 1usize;
    }
    if a.code != k.code {
        return 2usize;
    }
    if a.value != k.value {
        return 3usize;
    }
    if a.note != k.note {
        return 4usize;
    }
    if a.default != k.default {
        return 5usize;
    }
    6usize
}

/// 布尔数组是否全真。
fn all_true(v: [bool; 3]) -> bool {
    let mut i = 0usize;
    while i < v.len() {
        if let Some(x) = v.get(i) {
            if !x {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 四路注入是否全红。
fn four_true(f: [bool; 4]) -> bool {
    let mut i = 0usize;
    while i < f.len() {
        if let Some(x) = f.get(i) {
            if !x {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 造一条只改了若干字段的探针现场。
fn ash_probe(tag: &'static str, body: &'static str, phrase: Phrase) -> Shape {
    let mut s = Shape::blank();
    s.tag = tag;
    s.argv0 = "sh";
    s.prefix = "sh:";
    s.word = "foo";
    s.echo = "foo";
    s.flavor = Flavor::Ash;
    s.phrase = phrase;
    s.quote = phrase.quote();
    s.segkind = SegKind::None;
    s.tok = if phrase.uses_strerror() { ErrnoTok::Enoent } else { ErrnoTok::Absent };
    s.tail = if phrase.uses_strerror() { ErrnoTail::Glibc } else { ErrnoTail::None };
    s.body = body;
    s
}

// ---------------------------------------------------------------------------
// 四路注入
// ---------------------------------------------------------------------------

/// 四路注入：表路 / 判路 / 渲染路 / 账本路。
///
/// 1. 表路——段基址、码档表、闸段次序、语法表（方言 × 句族 × 拼写表）；
/// 2. 判路——三形式逐字、八闸纵线、回显与包裹、联签、对判、期望表；
/// 3. 渲染路——语料结构、矩阵覆盖与红档显式、状态面文本、释义；
/// 4. 账本路——阶段闸门单调、逐格对账、冻结重放。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && codes_ok() && stages_ok() && grammar_ok();
    out[1] = line_ok() && define_ok() && echo_ok() && sign_ok() && judge_ok();
    out[2] = corpus_ok() && slots_ok() && matrix_ok() && message_ok();
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
// 十三条判据
// ---------------------------------------------------------------------------

/// TX01 名称与段基址。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d1b0 {
        return false;
    }
    if TX_ESTAGE != SEG_BASE + 0x01
        || TX_ECODE != SEG_BASE + 0x02
        || TX_EVALUE != SEG_BASE + 0x03
        || TX_ENOTE != SEG_BASE + 0x04
        || TX_EDEFAULT != SEG_BASE + 0x05
        || TX_ECHANNEL != SEG_BASE + 0x06
        || TX_ESTATE != SEG_BASE + 0x07
        || TX_ECOUNT != SEG_BASE + 0x08
    {
        return false;
    }
    if TX_ESLOT != SEG_BASE + 0x09
        || TX_ESEAL != SEG_BASE + 0x0a
        || TX_ENOBS != SEG_BASE + 0x0b
        || TX_EMATRIX != SEG_BASE + 0x0c
        || TX_EBAL != SEG_BASE + 0x0d
    {
        return false;
    }
    if CODE_TABLE.len() != 13usize {
        return false;
    }
    // 尺值
    if CORPUS != 14usize
        || SAMPLES != 14usize
        || ACCEPT != 8usize
        || REJECT != 6usize
        || TIER_COUNT != 4usize
        || STAGE_COUNT != 8usize
        || CAP != 6usize
    {
        return false;
    }
    if LINE_MAX != 200usize || WORD_MAX != 127usize || PREFIX_MAX != 64usize {
        return false;
    }
    // 数档：本卡的数字通道只有两档（文案蕴含的 127 / 126）
    if EXIT_NOT_FOUND != 127u16 || EXIT_NOT_EXEC != 126u16 || EXIT_NONE != 0u16 {
        return false;
    }
    if CH_ENOENT != 2i32 || CH_EACCES != 13i32 || CH_EISDIR != 21i32 || CH_EINVAL != 22i32 {
        return false;
    }
    if CH_ENONE != 0i32 {
        return false;
    }
    // 操作码与八闸一一对应
    if OP_PREFIX != 0x01
        || OP_FLAVOR != 0x02
        || OP_STREAM != 0x03
        || OP_SEGMENT != 0x04
        || OP_ECHO != 0x05
        || OP_QUOTE != 0x06
        || OP_PHRASE != 0x07
        || OP_SIGN != 0x08
    {
        return false;
    }
    if (OP_SIGN as usize) != STAGE_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < 8usize {
        let op = (i + 1usize) as u8;
        let st = match Stage::from_order(i) {
            Some(s) => s,
            None => return false,
        };
        if st.order() != i || st.code() == 0 {
            return false;
        }
        let mut j = 0usize;
        let mut hit = 0usize;
        while j < CODE_TABLE.len() {
            if let Some(w) = CODE_TABLE.get(j) {
                if *w == st.code() {
                    hit += 1;
                }
            }
            j += 1;
        }
        if hit != 1usize {
            return false;
        }
        if op == 0 {
            return false;
        }
        i += 1;
    }
    // 逐字锚：四条主锚的长度是本卡的状态面契约
    if text_len(ASH_FOO_LINE) != 19usize
        || text_len(BASH_C_LINE) != 37usize
        || text_len(BASH_I_LINE) != 29usize
        || text_len(DASH_FOO_LINE) != 24usize
    {
        return false;
    }
    // 语法表里的静态片段
    if text_len(ASH_TAIL) != 11usize || text_len(BASH_TAIL) != 19usize {
        return false;
    }
    if text_len(ASH_OPEN) != 11usize || text_len(BASH_LINE) != 5usize {
        return false;
    }
    if text_len(STR_ENOENT) != 25usize
        || text_len(STR_EISDIR) != 14usize
        || text_len(STR_EACCES) != 17usize
    {
        return false;
    }
    if text_len(BUSY_ENOENT) != 13usize || text_len(APPLET_TAIL) != 17usize {
        return false;
    }
    if text_len(L10N_ZH) != 15usize {
        return false;
    }
    // 枚举维数
    if Flavor::table_size() != 3usize || Phrase::count() != 6usize {
        return false;
    }
    if Stage::count() != 8usize || TextNote::count() != 14usize {
        return false;
    }
    if Quote::count() != 4usize
        || SegKind::count() != 3usize
        || EchoForm::count() != 7usize
        || Locale::count() != 4usize
        || ErrnoTail::count() != 4usize
        || Sign::count() != 4usize
        || Stream::count() != 4usize
    {
        return false;
    }
    if Tier::at(9usize) != Tier::Foreign || Tier::count() != TIER_COUNT {
        return false;
    }
    true
}

/// TX02 错误码表与缺陷档。
fn codes_ok() -> bool {
    if CODE_TABLE.len() != 13usize {
        return false;
    }
    let mut i = 0usize;
    while i < CODE_TABLE.len() {
        let code = match CODE_TABLE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if code == 0 {
            return false;
        }
        if code != SEG_BASE.saturating_add(i as u32).saturating_add(1u32) {
            return false;
        }
        if code_name(code) == "unknown" || code_name(code).is_empty() {
            return false;
        }
        if code_desc(code).is_empty() || code_desc(code) == "非本卡码" {
            return false;
        }
        let mut j = i.saturating_add(1usize);
        while j < CODE_TABLE.len() {
            if let Some(w) = CODE_TABLE.get(j) {
                if *w == code {
                    return false;
                }
            }
            j = j.saturating_add(1usize);
        }
        i += 1;
    }
    if code_name(0) != "none" || code_desc(0) != "一致" {
        return false;
    }
    if code_name(SEG_BASE) != "unknown" || code_desc(0x9999) != "非本卡码" {
        return false;
    }
    // Fault 的码闭合：每一档的 code() 要么零、要么在码表里且只出现一次
    let faults = [
        Fault::Ok,
        Fault::Stage,
        Fault::Code,
        Fault::Value,
        Fault::Note,
        Fault::Default,
        Fault::Channel,
        Fault::Estate,
        Fault::Count,
        Fault::Slot,
        Fault::Seal,
        Fault::Nobs,
        Fault::Matrix,
        Fault::Bal,
    ];
    if faults.len() != 14usize {
        return false;
    }
    let mut k = 0usize;
    while k < faults.len() {
        let f = match faults.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let code = f.code();
        if k == 0usize {
            if code != 0 || f.alive() || f.name() != "ok" {
                return false;
            }
        } else if code == 0 || !f.alive() || f.name() == "ok" {
            return false;
        }
        let mut m = 0usize;
        let mut hits = 0usize;
        while m < CODE_TABLE.len() {
            if let Some(w) = CODE_TABLE.get(m) {
                if *w == code {
                    hits += 1;
                }
            }
            m += 1;
        }
        if f.is_claim_fault() {
            if hits != 1usize {
                return false;
            }
        } else if matches!(f, Fault::Channel | Fault::Count | Fault::Slot | Fault::Seal | Fault::Nobs | Fault::Matrix | Fault::Bal) && hits != 1usize {
            return false;
        }
        let mut n = k.saturating_add(1usize);
        while n < faults.len() {
            if let Some(g) = faults.get(n) {
                if g.name() == f.name() || fault_short(*g) != g.name() {
                    return false;
                }
            }
            n = n.saturating_add(1usize);
        }
        if fault_short(f) != f.name() {
            return false;
        }
        k += 1;
    }
    // 六维申报缺陷闭合
    let mut claim_hits = 0usize;
    let mut p = 0usize;
    while p < faults.len() {
        if let Some(f) = faults.get(p) {
            if f.is_claim_fault() {
                claim_hits += 1;
            }
        }
        p += 1;
    }
    if claim_hits != REJECT {
        return false;
    }
    if REJECT_FAULTS.len() != REJECT || REJECT_CODES.len() != REJECT {
        return false;
    }
    let mut q = 0usize;
    while q < REJECT {
        let f = match REJECT_FAULTS.get(q) {
            Some(v) => *v,
            None => return false,
        };
        let c = match REJECT_CODES.get(q) {
            Some(v) => *v,
            None => return false,
        };
        if !f.is_claim_fault() || f.code() != c {
            return false;
        }
        let mut r = q.saturating_add(1usize);
        while r < REJECT {
            if let Some(w) = REJECT_CODES.get(r) {
                if *w == c {
                    return false;
                }
            }
            if let Some(w) = REJECT_FAULTS.get(r) {
                if *w == f {
                    return false;
                }
            }
            r = r.saturating_add(1usize);
        }
        q += 1;
    }
    if VALS.len() != ACCEPT {
        return false;
    }
    let mut v = 0usize;
    while v < VALS.len() {
        if let Some(x) = VALS.get(v) {
            if *x == 0 {
                return false;
            }
        }
        v += 1;
    }
    match VALS.get(7usize) {
        Some(x) => *x == red_rate(),
        None => false,
    }
}

/// TX03 八闸次序与专属码。
fn stages_ok() -> bool {
    let all = [
        Stage::Prefix,
        Stage::Flavor,
        Stage::Stream,
        Stage::Segment,
        Stage::Echo,
        Stage::Quote,
        Stage::Phrase,
        Stage::Sign,
    ];
    if all.len() != STAGE_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < all.len() {
        let s = match all.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.order() != i {
            return false;
        }
        if Stage::from_order(i) != Some(s) {
            return false;
        }
        if s.name().is_empty() {
            return false;
        }
        if s.is_byte_gate() != (i < 7usize) {
            return false;
        }
        let mut j = i.saturating_add(1usize);
        while j < all.len() {
            if let Some(t) = all.get(j) {
                if t.code() == s.code() || t.name() == s.name() {
                    return false;
                }
            }
            j = j.saturating_add(1usize);
        }
        i += 1;
    }
    if Stage::from_order(99usize) != None {
        return false;
    }
    if next_stage(Stage::Sign) != Stage::Prefix {
        return false;
    }
    // 闸次序与注解面一致：结构面注解只可能落在前六道字节闸，族属面落在后两道
    let struct_notes = [
        TextNote::PrefixWrong,
        TextNote::SegmentWrong,
        TextNote::EchoMangled,
        TextNote::QuoteWrong,
        TextNote::StreamLeak,
    ];
    let mut m = 0usize;
    while m < struct_notes.len() {
        if let Some(n) = struct_notes.get(m) {
            if !n.is_structure() || n.is_family() || n.in_grammar() {
                return false;
            }
        }
        m += 1;
    }
    let fam_notes = [
        TextNote::PhraseWrong,
        TextNote::LocaleWrong,
        TextNote::ChanSplit,
        TextNote::SignBreak,
    ];
    let mut f = 0usize;
    while f < fam_notes.len() {
        if let Some(n) = fam_notes.get(f) {
            if !n.is_family() || n.is_structure() || n.in_grammar() {
                return false;
            }
        }
        f += 1;
    }
    // 语法四档与归属档互斥
    let grammar_notes = [
        TextNote::PlainNotFound,
        TextNote::CommandNotFound,
        TextNote::CantOpen,
        TextNote::SysError,
    ];
    let mut g = 0usize;
    while g < grammar_notes.len() {
        let n = match grammar_notes.get(g) {
            Some(v) => *v,
            None => return false,
        };
        if !n.in_grammar() || n.is_wrong() || n.phrase() == None {
            return false;
        }
        g += 1;
    }
    if TextNote::NotOurLine.in_grammar()
        || TextNote::NotOurLine.is_structure()
        || TextNote::NotOurLine.is_family()
        || TextNote::NotOurLine.phrase() != None
    {
        return false;
    }
    // 注解的句族回查闭合：四档各指一个不同句族
    let mut seen = [false; 6];
    let mut h = 0usize;
    while h < grammar_notes.len() {
        let n = match grammar_notes.get(h) {
            Some(v) => *v,
            None => return false,
        };
        let ndx = match n.phrase() {
            Some(p) => phrase_ndx(p),
            None => return false,
        };
        if ndx >= 6 {
            return false;
        }
        match seen.get_mut(ndx) {
            Some(x) => {
                if *x {
                    return false;
                }
                *x = true;
            }
            None => return false,
        }
        h += 1;
    }
    // 每个句族都至少被一条闸负责（句族闸负责四个表内族，归属闸收另外两族）
    let phrases = [
        Phrase::NotFound,
        Phrase::CommandNotFound,
        Phrase::CantOpen,
        Phrase::SysError,
        Phrase::AppletNotFound,
        Phrase::Foreign,
    ];
    let mut pi = 0usize;
    while pi < phrases.len() {
        let p = match phrases.get(pi) {
            Some(v) => *v,
            None => return false,
        };
        if p.name().is_empty() || p.quote().name().is_empty() {
            return false;
        }
        pi += 1;
    }
    true
}

/// 句族下标（注解回查用）。
fn phrase_ndx(p: Phrase) -> usize {
    match p {
        Phrase::NotFound => 0usize,
        Phrase::CommandNotFound => 1usize,
        Phrase::CantOpen => 2usize,
        Phrase::SysError => 3usize,
        Phrase::AppletNotFound => 4usize,
        Phrase::Foreign => 5usize,
    }
}

/// TX04 方言 × 句族 × 拼写表的语法表闭合。
fn grammar_ok() -> bool {
    let flavors = [
        Flavor::Ash,
        Flavor::Bash,
        Flavor::Dash,
        Flavor::BusyboxApplet,
        Flavor::Unknown,
    ];
    if flavors.len() != 5usize {
        return false;
    }
    let mut i = 0usize;
    let mut in_table = 0usize;
    let mut fam_total = 0usize;
    while i < flavors.len() {
        let fl = match flavors.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if fl.name().is_empty() {
            return false;
        }
        if fl.in_table() {
            in_table += 1;
        }
        if fl.family_count() == 0 && fl.in_table() {
            return false;
        }
        fam_total = fam_total.saturating_add(fl.family_count());
        // 表外方言一概不允任何句族
        if !fl.in_table() && fl.allows(Phrase::NotFound) {
            return false;
        }
        if !fl.in_table() && fl.allows_tail(ErrnoTail::Glibc) {
            return false;
        }
        i += 1;
    }
    if in_table != Flavor::table_size() {
        return false;
    }
    if fam_total != 5usize {
        return false;
    }
    // 三方言的句族列逐格核（本卡语法表的第一列）
    if !Flavor::Ash.allows(Phrase::NotFound)
        || !Flavor::Ash.allows(Phrase::CantOpen)
        || Flavor::Ash.allows(Phrase::CommandNotFound)
        || Flavor::Ash.allows(Phrase::SysError)
    {
        return false;
    }
    if !Flavor::Bash.allows(Phrase::CommandNotFound)
        || !Flavor::Bash.allows(Phrase::SysError)
        || Flavor::Bash.allows(Phrase::NotFound)
        || Flavor::Bash.allows(Phrase::CantOpen)
    {
        return false;
    }
    if !Flavor::Dash.allows(Phrase::NotFound) || Flavor::Dash.allows(Phrase::CantOpen) {
        return false;
    }
    if Flavor::Ash.family_count() != 2usize
        || Flavor::Bash.family_count() != 2usize
        || Flavor::Dash.family_count() != 1usize
    {
        return false;
    }
    // 行号段档：同一条句族在 bash 的两种形态下行号段互斥，且永远只有一个字节形状
    if SegKind::None.span(1) != "" {
        return false;
    }
    if SegKind::BareLine.span(1) != "1: " || SegKind::BareLine.span(12) != "12: " {
        return false;
    }
    if SegKind::LineWord.span(1) != "line 1: " || SegKind::LineWord.span(40) != "line 40: " {
        return false;
    }
    if SegKind::count() != 3usize {
        return false;
    }
    // 引号档：全仓只有两族加单引号，双引号一档真机不存在
    if Phrase::NotFound.quote() != Quote::Bare
        || Phrase::CommandNotFound.quote() != Quote::Bare
        || Phrase::SysError.quote() != Quote::Bare
        || Phrase::CantOpen.quote() != Quote::Single
        || Phrase::AppletNotFound.quote() != Quote::Single
        || Phrase::Foreign.quote() != Quote::Absent
    {
        return false;
    }
    if Quote::wrap(&Quote::Bare, "foo") != "foo"
        || Quote::wrap(&Quote::Single, "foo") != "'foo'"
        || Quote::wrap(&Quote::Double, "foo") != "\"foo\""
        || Quote::wrap(&Quote::Absent, "foo") != ""
    {
        return false;
    }
    if msg_mid(Phrase::CantOpen) != ASH_OPEN || msg_mid(Phrase::NotFound) != "" {
        return false;
    }
    // 尾句静态片段逐字
    if Phrase::NotFound.static_tail() != ASH_TAIL
        || Phrase::CommandNotFound.static_tail() != BASH_TAIL
        || Phrase::AppletNotFound.static_tail() != APPLET_TAIL
    {
        return false;
    }
    if Phrase::CantOpen.static_tail() != ": " || Phrase::SysError.static_tail() != ": " {
        return false;
    }
    if Phrase::NotFound.colon_before_tail() != true
        || Phrase::AppletNotFound.colon_before_tail() != false
    {
        return false;
    }
    // 中段只有 can't open 一族有；两族用 strerror
    let mut strerr = 0usize;
    let ps = [
        Phrase::NotFound,
        Phrase::CommandNotFound,
        Phrase::CantOpen,
        Phrase::SysError,
        Phrase::AppletNotFound,
        Phrase::Foreign,
    ];
    let mut q = 0usize;
    while q < ps.len() {
        let p = match ps.get(q) {
            Some(v) => *v,
            None => return false,
        };
        if p.uses_strerror() {
            strerr += 1;
            if !p.in_table() {
                return false;
            }
        }
        if p.mid() != "" && p != Phrase::CantOpen {
            return false;
        }
        q += 1;
    }
    if strerr != 2usize {
        return false;
    }
    if Phrase::AppletNotFound.in_table() || Phrase::Foreign.in_table() {
        return false;
    }
    // errno 词与拼写表
    if ErrnoTok::Enoent.value() != CH_ENOENT
        || ErrnoTok::Eacces.value() != CH_EACCES
        || ErrnoTok::Eisdir.value() != CH_EISDIR
        || ErrnoTok::Absent.value() != CH_ENONE
        || ErrnoTok::Other.value() != CH_EINVAL
    {
        return false;
    }
    let vals = [CH_ENONE, CH_ENOENT, CH_EACCES, CH_EISDIR, CH_EINVAL, 99];
    let mut t = 0usize;
    while t < vals.len() {
        let v = match vals.get(t) {
            Some(x) => *x,
            None => return false,
        };
        let tok = ErrnoTok::of(v);
        if tok.name().is_empty() {
            return false;
        }
        if v == CH_EINVAL || v == 99 {
            if tok.in_table() {
                return false;
            }
        } else if v == CH_ENONE {
            if tok.in_table() || tok.libc_text() != "" || tok.busy_text() != "" {
                return false;
            }
        } else if !tok.in_table() {
            return false;
        }
        t += 1;
    }
    if ErrnoTok::Enoent.libc_text() != STR_ENOENT
        || ErrnoTok::Eisdir.libc_text() != STR_EISDIR
        || ErrnoTok::Eacces.libc_text() != STR_EACCES
        || ErrnoTok::Enoent.busy_text() != BUSY_ENOENT
        || ErrnoTok::Eisdir.busy_text() != ""
    {
        return false;
    }
    // strerror_text 的三张表
    if strerror_text(ErrnoTok::Enoent, ErrnoTail::None) != "" {
        return false;
    }
    if strerror_text(ErrnoTok::Enoent, ErrnoTail::Glibc) != STR_ENOENT {
        return false;
    }
    if strerror_text(ErrnoTok::Enoent, ErrnoTail::BusyShort) != BUSY_ENOENT {
        return false;
    }
    if strerror_text(ErrnoTok::Enoent, ErrnoTail::Translated) != L10N_ENOENT {
        return false;
    }
    if strerror_text(ErrnoTok::Absent, ErrnoTail::Glibc) != "" {
        return false;
    }
    if !ErrnoTail::None.is_table() || !ErrnoTail::Glibc.is_table() || !ErrnoTail::BusyShort.is_table() {
        return false;
    }
    if ErrnoTail::Translated.is_table() {
        return false;
    }
    if !Flavor::Ash.allows_tail(ErrnoTail::BusyShort)
        || !Flavor::Bash.allows_tail(ErrnoTail::Glibc)
        || Flavor::Bash.allows_tail(ErrnoTail::BusyShort)
        || Flavor::Dash.allows_tail(ErrnoTail::BusyShort)
    {
        return false;
    }
    // 句族蕴含的 errno / 退出数
    if Phrase::NotFound.implied_errno(ErrnoTok::Absent) != CH_ENOENT
        || Phrase::CommandNotFound.implied_errno(ErrnoTok::Enoent) != CH_ENOENT
        || Phrase::SysError.implied_errno(ErrnoTok::Eisdir) != CH_EISDIR
        || Phrase::AppletNotFound.implied_errno(ErrnoTok::Absent) != CH_ENONE
    {
        return false;
    }
    if Phrase::NotFound.implied_exit(ErrnoTok::Absent) != EXIT_NOT_FOUND
        || Phrase::CommandNotFound.implied_exit(ErrnoTok::Absent) != EXIT_NOT_FOUND
        || Phrase::SysError.implied_exit(ErrnoTok::Enoent) != EXIT_NOT_FOUND
        || Phrase::CantOpen.implied_exit(ErrnoTok::Enoent) != EXIT_NOT_FOUND
        || Phrase::SysError.implied_exit(ErrnoTok::Eisdir) != EXIT_NOT_EXEC
        || Phrase::SysError.implied_exit(ErrnoTok::Eacces) != EXIT_NOT_EXEC
        || Phrase::AppletNotFound.implied_exit(ErrnoTok::Absent) != EXIT_NONE
    {
        return false;
    }
    // locale 四档：只有 Localized 会说非英文尾句
    if !Locale::C.is_english() || !Locale::Unset.is_english() || !Locale::Posix.is_english() {
        return false;
    }
    if Locale::Localized.is_english() {
        return false;
    }
    // 蕴含退出数只有三档：0 / 126 / 127
    let ex = [Phrase::NotFound, Phrase::CommandNotFound, Phrase::CantOpen, Phrase::SysError];
    let mut e = 0usize;
    while e < ex.len() {
        let p = match ex.get(e) {
            Some(v) => *v,
            None => return false,
        };
        let tokv = [
            ErrnoTok::Absent,
            ErrnoTok::Enoent,
            ErrnoTok::Eacces,
            ErrnoTok::Eisdir,
            ErrnoTok::Other,
        ];
        let mut m = 0usize;
        while m < tokv.len() {
            let tk = match tokv.get(m) {
                Some(v) => *v,
                None => return false,
            };
            let x = p.implied_exit(tk);
            if x != EXIT_NOT_FOUND && x != EXIT_NOT_EXEC && x != EXIT_NONE {
                return false;
            }
            m += 1;
        }
        e += 1;
    }
    // 流档：只有 stderr 合法
    if !Stream::Stderr.is_ok() || Stream::Stdout.is_ok() || Stream::Both.is_ok() {
        return false;
    }
    if !Stream::Nowhere.is_void() || Stream::Stderr.is_void() {
        return false;
    }
    // 联签四档：只有 Agree 可签
    if !Sign::Agree.is_agree() || Sign::Disagree.is_agree() || Sign::Absent.is_agree() {
        return false;
    }
    if Sign::ChildOwn.is_agree() {
        return false;
    }
    if Sign::Agree.name() != "signed"
        || Sign::Disagree.name() != "disagreed"
        || Sign::Absent.name() != "unsigned"
        || Sign::ChildOwn.name() != "child-owned"
    {
        return false;
    }
    true
}

/// TX05 四种形式的逐字闭包与行级自检。
fn line_ok() -> bool {
    // 主锚：由语法部件拼装，必须与语料表里的逐字原文相等
    let mut p0 = Shape::blank();
    p0.argv0 = "/bin/sh";
    p0.prefix = "sh:";
    p0.body = ASH_FOO_LINE;
    if line_of(&p0) != ASH_FOO_LINE || line_text(&p0) != ASH_FOO_LINE {
        return false;
    }
    let mut p1 = p0;
    p1.argv0 = "bash";
    p1.prefix = "bash:";
    p1.flavor = Flavor::Bash;
    p1.phrase = Phrase::CommandNotFound;
    p1.quote = Phrase::CommandNotFound.quote();
    p1.segkind = SegKind::LineWord;
    p1.body = BASH_C_LINE;
    if line_of(&p1) != BASH_C_LINE || msg_prefix(p1.argv0) != "bash:" {
        return false;
    }
    let mut p2 = p1;
    p2.interactive = true;
    p2.segkind = SegKind::None;
    p2.body = BASH_I_LINE;
    if line_of(&p2) != BASH_I_LINE {
        return false;
    }
    let mut p3 = p0;
    p3.argv0 = "/bin/dash";
    p3.prefix = "dash:";
    p3.flavor = Flavor::Dash;
    p3.segkind = SegKind::BareLine;
    p3.body = DASH_FOO_LINE;
    if line_of(&p3) != DASH_FOO_LINE || want_seg_of(&p3) != SegKind::BareLine {
        return false;
    }
    // 逐段拼装路线（msg_* 构造子）与整行路线一致
    let parts = msg_line(
        msg_prefix(p3.argv0).as_str(),
        msg_segment(SegKind::BareLine, 1).as_str(),
        msg_mid(Phrase::NotFound),
        msg_wrap(Quote::Bare, "foo").as_str(),
        msg_tail_of(Phrase::NotFound, ErrnoTok::Absent, ErrnoTail::None).as_str(),
    );
    if parts != DASH_FOO_LINE {
        return false;
    }
    // strerror 行的两段式尾句
    let tail = msg_tail_of(Phrase::SysError, ErrnoTok::Enoent, ErrnoTail::Glibc);
    if tail != ": No such file or directory" {
        return false;
    }
    if msg_tail_of(Phrase::SysError, ErrnoTok::Enoent, ErrnoTail::BusyShort) != ": no such file" {
        return false;
    }
    if msg_tail_of(Phrase::NotFound, ErrnoTok::Absent, ErrnoTail::None) != ASH_TAIL {
        return false;
    }
    // 语料四形式闭合：活槽三形式全等、回显偏移与位图相符
    let mut i = 0usize;
    let mut split = 0usize;
    while i < CORPUS {
        let s = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let triple = forms_triple(&s);
        if all_true(triple) {
            split += 1;
        }
        if i < ACCEPT {
            if !all_true(triple) {
                return false;
            }
            if text_len(s.line().as_str()) != WANT_BYTES[i] as usize {
                return false;
            }
            if s.echo_index() != WANT_NDX[i] {
                return false;
            }
            if !lint_line(s.body) {
                return false;
            }
            if line_text(&s) != s.line() {
                return false;
            }
        }
        i += 1;
    }
    // 归属面两槽（applet 行与未知方言行）刻意让整行原文与拼装不一致
    if split != CORPUS.saturating_sub(2usize) {
        return false;
    }
    // 首字节差与位图
    if msg_diff_pos(ASH_FOO_LINE, ASH_FOO_LINE) != 0 {
        return false;
    }
    if msg_diff_pos("sh: foo: not found\n", "sh: fo: not found\n") != 5 {
        return false;
    }
    if msg_diff_pos("abc", "abcdef") != 3 {
        return false;
    }
    if msg_bytes_map(&p0) != "4/3/12/19" {
        return false;
    }
    if trim_eol(ASH_FOO_LINE) != "sh: foo: not found" {
        return false;
    }
    if msg_body_trim(&p0) != "sh: foo: not found" {
        return false;
    }
    if msg_line_trim(&p0) != "sh: foo: not found" {
        return false;
    }
    // lint_line 的正反判据
    if lint_line("") || lint_line("sh: foo: not found") {
        return false;
    }
    if lint_line("sh: foo: not found\n\n") {
        return false;
    }
    if lint_line("no colon and no tail\n") {
        return false;
    }
    if !lint_line(BASH_C_LINE) || !lint_line("sh: x: Permission denied\n") {
        return false;
    }
    // 行号段在整行里只能出现一次
    if newline_count(BASH_C_LINE) != 1 || newline_count(ASH_FOO_LINE) != 1 {
        return false;
    }
    if colon_count(ASH_FOO_LINE) != 2usize || colon_count(BASH_C_LINE) != 3usize {
        return false;
    }
    true
}

/// 方言 + 交互态应有的行号段档（`Shape::want_seg` 的自由函数形态，供判据独立复算）。
fn want_seg_of(s: &Shape) -> SegKind {
    match s.flavor {
        Flavor::Bash => {
            if s.interactive {
                SegKind::None
            } else {
                SegKind::LineWord
            }
        }
        Flavor::Dash => SegKind::BareLine,
        Flavor::Ash | Flavor::BusyboxApplet | Flavor::Unknown => SegKind::None,
    }
}

/// TX06 八闸纵线逐道判死。
fn define_ok() -> bool {
    if CORPUS != 14usize {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        if case_at(i).is_none() {
            return false;
        }
        i += 1;
    }
    if case_at(CORPUS).is_some() {
        return false;
    }
    // 空白现场：词为空，死在回显闸，三个通道填不可达哨兵
    let o = define(&Shape::blank());
    if o.stage != Stage::Echo || o.note != TextNote::EchoMangled || o.value != 0 {
        return false;
    }
    if o.ok() || !o.failed() {
        return false;
    }
    // 闸一：前缀不是 argv[0] 的 basename
    let mut g1 = ash_probe("g1", ASH_FOO_LINE, Phrase::NotFound);
    g1.prefix = "/bin/sh:";
    let a1 = define(&g1);
    if a1.stage != Stage::Prefix || a1.note != TextNote::PrefixWrong || a1.tier() != Tier::Structure {
        return false;
    }
    if a1.code != CH_ENOENT || a1.value != 19 || a1.default != true {
        return false;
    }
    // 闸一另一型：整行没有收尾换行
    let mut g1b = ash_probe("g1b", "sh: foo: not found", Phrase::NotFound);
    let a1b = define(&g1b);
    if a1b.stage != Stage::Prefix || a1b.note != TextNote::PrefixWrong {
        return false;
    }
    // 闸二：表外方言（busybox 本体的 applet 行）
    let mut g2 = ash_probe("g2", "busybox: 'foo' applet not found\n", Phrase::AppletNotFound);
    g2.argv0 = "busybox";
    g2.prefix = "busybox:";
    g2.flavor = Flavor::BusyboxApplet;
    g2.quote = Quote::Single;
    let a2 = define(&g2);
    if a2.stage != Stage::Flavor || a2.note != TextNote::NotOurLine || a2.code != CH_ENONE {
        return false;
    }
    if a2.default || a2.tier() != Tier::Foreign {
        return false;
    }
    // 闸二另一型：未知方言
    let mut g2b = ash_probe("g2b", "zsh: command not found: foo\n", Phrase::NotFound);
    g2b.argv0 = "zsh";
    g2b.prefix = "zsh:";
    g2b.flavor = Flavor::Unknown;
    let a2b = define(&g2b);
    if a2b.stage != Stage::Flavor || a2b.note != TextNote::NotOurLine || !a2b.default {
        return false;
    }
    // 闸三：行漏进 stdout
    let mut g3 = ash_probe("g3", ASH_FOO_LINE, Phrase::NotFound);
    g3.stream = Stream::Stdout;
    let a3 = define(&g3);
    if a3.stage != Stage::Stream || a3.note != TextNote::StreamLeak || a3.tier() != Tier::Structure {
        return false;
    }
    // 闸三另一型：一个字节都没落（无观测）
    let mut g3b = ash_probe("g3b", ASH_FOO_LINE, Phrase::NotFound);
    g3b.stream = Stream::Nowhere;
    if define(&g3b).note != TextNote::StreamLeak {
        return false;
    }
    // 闸四：bash 交互态却带了行号段
    let mut g4 = ash_probe("g4", BASH_C_LINE, Phrase::CommandNotFound);
    g4.argv0 = "bash";
    g4.prefix = "bash:";
    g4.flavor = Flavor::Bash;
    g4.interactive = true;
    g4.segkind = SegKind::LineWord;
    g4.phrase = Phrase::CommandNotFound;
    let a4 = define(&g4);
    if a4.stage != Stage::Segment || a4.note != TextNote::SegmentWrong || a4.value != 37 {
        return false;
    }
    // 闸四另一型：dash 少了裸行号段
    let mut g4b = ash_probe("g4b", "dash: foo: not found\n", Phrase::NotFound);
    g4b.argv0 = "dash";
    g4b.prefix = "dash:";
    g4b.flavor = Flavor::Dash;
    let a4b = define(&g4b);
    if a4b.stage != Stage::Segment || a4b.note != TextNote::SegmentWrong {
        return false;
    }
    if want_seg_of(&g4b) != g4b.want_seg() {
        return false;
    }
    // 闸五：回显被小写化
    let mut g5 = ash_probe("g5", ASH_FOO_LINE, Phrase::NotFound);
    g5.word = "Foo";
    let a5 = define(&g5);
    if a5.stage != Stage::Echo || a5.note != TextNote::EchoMangled {
        return false;
    }
    // 闸五另一型：回显整体丢失
    let mut g5b = ash_probe("g5b", "sh: : not found\n", Phrase::NotFound);
    g5b.echo = "";
    let a5b = define(&g5b);
    if a5b.stage != Stage::Echo || a5b.note != TextNote::EchoMangled {
        return false;
    }
    // 闸五另一型：词为空
    let mut g5c = ash_probe("g5c", ASH_FOO_LINE, Phrase::NotFound);
    g5c.word = "";
    g5c.echo = "";
    if define(&g5c).stage != Stage::Echo {
        return false;
    }
    // 闸六：ash 的 not found 行好心加了单引号
    let mut g6 = ash_probe("g6", "sh: 'foo': not found\n", Phrase::NotFound);
    g6.quote = Quote::Single;
    let a6 = define(&g6);
    if a6.stage != Stage::Quote || a6.note != TextNote::QuoteWrong || a6.tier() != Tier::Structure {
        return false;
    }
    // 闸六另一型：双引号档真机不存在
    let mut g6b = ash_probe("g6b", "sh: foo: not found\n", Phrase::NotFound);
    g6b.quote = Quote::Double;
    if define(&g6b).note != TextNote::QuoteWrong {
        return false;
    }
    // 闸七：ash 说 bash 的句子
    let mut g7 = ash_probe("g7", BASH_I_LINE, Phrase::CommandNotFound);
    g7.phrase = Phrase::CommandNotFound;
    g7.body = BASH_I_LINE;
    let a7 = define(&g7);
    if a7.stage != Stage::Phrase || a7.note != TextNote::PhraseWrong || a7.tier() != Tier::Family {
        return false;
    }
    // 闸七另一型：locale 翻过尾句
    let mut g7b = ash_probe("g7b", "bash: foo: 未找到命令\n", Phrase::CommandNotFound);
    g7b.argv0 = "bash";
    g7b.prefix = "bash:";
    g7b.flavor = Flavor::Bash;
    g7b.phrase = Phrase::CommandNotFound;
    g7b.locale = Locale::Localized;
    let a7b = define(&g7b);
    if a7b.note == TextNote::PhraseWrong {
        return false;
    }
    if a7b.stage != Stage::Phrase || a7b.note != TextNote::LocaleWrong {
        return false;
    }
    // 闸七另一型：不含 strerror 的族却带 errno 词
    let mut g7c = ash_probe("g7c", ASH_FOO_LINE, Phrase::NotFound);
    g7c.tok = ErrnoTok::Enoent;
    g7c.tail = ErrnoTail::Glibc;
    g7c.body = "sh: foo: No such file or directory\n";
    if define(&g7c).note != TextNote::PhraseWrong {
        return false;
    }
    // 闸七另一型：bash 用 busybox 的短表
    let mut g7d = ash_probe("g7d", "bash: x: no such file\n", Phrase::SysError);
    g7d.argv0 = "bash";
    g7d.prefix = "bash:";
    g7d.flavor = Flavor::Bash;
    g7d.phrase = Phrase::SysError;
    g7d.word = "x";
    g7d.echo = "x";
    g7d.tok = ErrnoTok::Enoent;
    g7d.tail = ErrnoTail::BusyShort;
    if define(&g7d).note != TextNote::PhraseWrong {
        return false;
    }
    // 闸七另一型：表里没有的 errno 词
    let mut g7e = ash_probe("g7e", "sh: can't open 'x': \n", Phrase::CantOpen);
    g7e.phrase = Phrase::CantOpen;
    g7e.quote = Quote::Single;
    g7e.word = "x";
    g7e.echo = "x";
    g7e.tok = ErrnoTok::Other;
    g7e.tail = ErrnoTail::Glibc;
    let a7e = define(&g7e);
    if a7e.stage != Stage::Phrase || a7e.note != TextNote::PhraseWrong {
        return false;
    }
    if a7e.code != CH_EINVAL {
        return false;
    }
    // 闸八：联签不上
    let mut g8 = ash_probe("g8", ASH_FOO_LINE, Phrase::NotFound);
    g8.sign = Sign::Disagree;
    let a8 = define(&g8);
    if a8.stage != Stage::Sign || a8.note != TextNote::SignBreak || a8.tier() != Tier::Family {
        return false;
    }
    if !a8.default || a8.implied_exit() != EXIT_NOT_FOUND {
        return false;
    }
    let mut g8b = ash_probe("g8b", ASH_FOO_LINE, Phrase::NotFound);
    g8b.sign = Sign::ChildOwn;
    if define(&g8b).note != TextNote::SignBreak {
        return false;
    }
    let mut g8c = ash_probe("g8c", ASH_FOO_LINE, Phrase::NotFound);
    g8c.sign = Sign::Absent;
    if define(&g8c).note != TextNote::SignBreak {
        return false;
    }
    // 收尾：两种形式背离
    let mut g9 = ash_probe("g9", ASH_FOO_LINE, Phrase::NotFound);
    g9.body = "sh: foo: not  found\n";
    let a9 = define(&g9);
    if a9.stage != Stage::Sign || a9.note != TextNote::ChanSplit || a9.value != 20 {
        return false;
    }
    if a9.ok() || !g9.locale.is_english() {
        return false;
    }
    // 现场本身自洽时三形式必须闭合
    let s0 = match shape_at(0usize) {
        Some(v) => v,
        None => return false,
    };
    if !s0.forms_agree() || !all_true(forms_triple(&s0)) {
        return false;
    }
    // 活槽八闸走满
    let mut w = 0usize;
    while w < ACCEPT {
        let s = match shape_at(w) {
            Some(v) => v,
            None => return false,
        };
        let a = define(&s);
        if a.stage != Stage::Sign || !a.ok() || !a.note.in_grammar() {
            return false;
        }
        if a.value != text_len(s.body) as u32 || a.value == 0 {
            return false;
        }
        if a.code != s.phrase.implied_errno(s.tok) {
            return false;
        }
        if a.default != (s.phrase.implied_exit(s.tok) == EXIT_NOT_FOUND) {
            return false;
        }
        if a.implied_exit() == EXIT_NONE {
            return false;
        }
        if a.tier() == Tier::Foreign || a.tier() == Tier::Structure {
            return false;
        }
        w += 1;
    }
    // 死槽六档各归其面
    let want_stage = [
        Stage::Flavor,
        Stage::Echo,
        Stage::Segment,
        Stage::Flavor,
        Stage::Sign,
        Stage::Sign,
    ];
    let want_note = [
        TextNote::NotOurLine,
        TextNote::EchoMangled,
        TextNote::SegmentWrong,
        TextNote::NotOurLine,
        TextNote::PlainNotFound,
        TextNote::PlainNotFound,
    ];
    let mut d = 0usize;
    while d < REJECT {
        let s = match shape_at(ACCEPT + d) {
            Some(v) => v,
            None => return false,
        };
        let a = define(&s);
        let st = match want_stage.get(d) {
            Some(v) => *v,
            None => return false,
        };
        let nt = match want_note.get(d) {
            Some(v) => *v,
            None => return false,
        };
        if a.stage != st || a.note != nt {
            return false;
        }
        if d < 2usize {
            if a.tier() != Tier::Foreign {
                return false;
            }
        } else if d == 2usize {
            if a.tier() != Tier::Structure {
                return false;
            }
        } else if a.tier() != Tier::Foreign && a.tier() != Tier::Grammar {
            return false;
        }
        // 实际结果为假的槽，注解必不属于语法表；为真的槽反之
        if nt.in_grammar() != a.ok() {
            return false;
        }
        d += 1;
    }
    // 哨兵只属于空行：本卡的闸判死一律保留观测三通道
    let mut sentinel_hits = 0usize;
    let mut e = 0usize;
    while e < CORPUS {
        let s = match shape_at(e) {
            Some(v) => v,
            None => return false,
        };
        let a = define(&s);
        if a.code == CH_EINVAL {
            sentinel_hits += 1;
        }
        e += 1;
    }
    if sentinel_hits != 1usize {
        return false;
    }
    // Outcome 的指纹只由五维决定，tag 不参与
    let x0 = match outcome_at(0usize) {
        Some(v) => v,
        None => return false,
    };
    let x7 = match outcome_at(7usize) {
        Some(v) => v,
        None => return false,
    };
    if x0.seal() == x7.seal() {
        return false;
    }
    true
}

/// TX07 回显、包裹与行号段的字节关系。
fn echo_ok() -> bool {
    let cases = [
        ("foo", "foo", EchoForm::Verbatim),
        ("/bin/foo", "foo", EchoForm::Basename),
        ("./missing", "missing", EchoForm::Basename),
        ("Foo", "foo", EchoForm::Lowered),
        ("foo", "FOO", EchoForm::Uppered),
        ("foobar", "foo", EchoForm::Truncated),
        ("foo", "", EchoForm::Dropped),
        ("", "", EchoForm::Verbatim),
        ("foo", "foo ", EchoForm::Extra),
        ("foo", "foobar", EchoForm::Extra),
        ("foo", "fooo", EchoForm::Extra),
    ];
    if cases.len() != 11usize {
        return false;
    }
    let mut i = 0usize;
    let mut verbatim = 0usize;
    while i < cases.len() {
        let (w, e, want) = match cases.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let got = EchoForm::of(w, e);
        if got != want {
            return false;
        }
        if got.is_verbatim() {
            verbatim += 1;
        }
        if got.name().is_empty() {
            return false;
        }
        i += 1;
    }
    if verbatim != 2usize {
        return false;
    }
    if EchoForm::count() != 7usize {
        return false;
    }
    // 七档互不相同
    let forms = [
        EchoForm::Verbatim,
        EchoForm::Basename,
        EchoForm::Lowered,
        EchoForm::Uppered,
        EchoForm::Truncated,
        EchoForm::Dropped,
        EchoForm::Extra,
    ];
    let mut a = 0usize;
    while a < forms.len() {
        let x = match forms.get(a) {
            Some(v) => *v,
            None => return false,
        };
        let mut b = a.saturating_add(1usize);
        while b < forms.len() {
            if let Some(y) = forms.get(b) {
                if y.name() == x.name() || *y == x {
                    return false;
                }
            }
            b = b.saturating_add(1usize);
        }
        a += 1;
    }
    // basename / 大小写的字节基元
    if basename_of("/bin/sh") != "sh" || basename_of("sh") != "sh" || basename_of("") != "" {
        return false;
    }
    if basename_of("/usr/bin/busybox") != "busybox" {
        return false;
    }
    if basename_of("a/b/c") != "c" {
        return false;
    }
    if !basename_eq("/bin/dash", "dash") || basename_eq("dash", "sh") {
        return false;
    }
    if ascii_lower("FooBAR") != "foobar" || ascii_lower("已") != "已" {
        return false;
    }
    if !has_upper("FooBAR") || has_upper("foo") {
        return false;
    }
    if !word_has_slash("/tmp/nosuch") || word_has_slash("nosuch") {
        return false;
    }
    if !word_is_empty("") || word_is_empty("x") {
        return false;
    }
    if !has_control("a\tb") || has_control("a b") {
        return false;
    }
    if has_non_ascii("sh: foo") != false || !has_non_ascii(L10N_ZH) {
        return false;
    }
    if msg_wrap(Quote::Single, "x") != "'x'" || msg_wrap(Quote::Bare, "x") != "x" {
        return false;
    }
    // 语料里的回显：活槽逐字、槽 9 被小写化、其余逐字
    let mut w = 0usize;
    let mut mangled = 0usize;
    while w < CORPUS {
        let s = match shape_at(w) {
            Some(v) => v,
            None => return false,
        };
        if s.echo_form().is_verbatim() {
            if s.tag() == "stage-swap-lowered-echo" {
                return false;
            }
        } else {
            mangled += 1;
            if s.tag() != "stage-swap-lowered-echo" {
                return false;
            }
        }
        if text_len(s.word) > WORD_MAX {
            return false;
        }
        if has_control(s.word) && w < ACCEPT {
            return false;
        }
        w += 1;
    }
    if mangled != 1usize {
        return false;
    }
    // 行号段：整行里 `line ` 只能出现在 bash 非交互行，裸行号只能在 dash 行
    let mut ln = 0usize;
    let mut bare = 0usize;
    let mut j = 0usize;
    while j < CORPUS {
        let s = match shape_at(j) {
            Some(v) => v,
            None => return false,
        };
        if contains_text(s.body, BASH_LINE) {
            ln += 1;
            if s.flavor != Flavor::Bash || s.segkind != SegKind::LineWord {
                return false;
            }
        }
        if s.segkind == SegKind::BareLine {
            bare += 1;
            if s.flavor != Flavor::Dash {
                return false;
            }
            let head = s.line_head();
            if !contains_text(head.as_str(), ": ") {
                return false;
            }
        }
        j += 1;
    }
    if ln != 2usize || bare != 2usize {
        return false;
    }
    // 期望段档与现场段档的复算一致
    let mut k = 0usize;
    while k < CORPUS {
        let s = match shape_at(k) {
            Some(v) => v,
            None => return false,
        };
        if s.want_seg() != want_seg_of(&s) {
            return false;
        }
        k += 1;
    }
    true
}

/// TX08 与 B30 的联签（本卡不生产数字，只引用数字的结论）。
fn sign_ok() -> bool {
    // 规范行的联签位与句族蕴含一致
    let mut i = 0usize;
    let mut signed = 0usize;
    let mut unsigned = 0usize;
    while i < ACCEPT {
        let o = match outcome_at(i) {
            Some(v) => v,
            None => return false,
        };
        let s = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        if !o.ok() {
            return false;
        }
        if o.default {
            signed += 1;
            if o.implied_exit() != EXIT_NOT_FOUND {
                return false;
            }
            if !s.phrase.in_table() {
                return false;
            }
        } else {
            unsigned += 1;
            if o.implied_exit() == EXIT_NOT_FOUND {
                return false;
            }
        }
        if s.sign != Sign::Agree {
            return false;
        }
        i += 1;
    }
    if signed != 7usize || unsigned != 1usize {
        return false;
    }
    // 申报的联签位与 success() 各自独立：抹掉联签位不得翻成败面
    let mut j = 0usize;
    while j < REJECT {
        let c = match case_at(ACCEPT + j) {
            Some(v) => v,
            None => return false,
        };
        let a = c.actual();
        let k = mk_claim(&a);
        if judge(&Case { shape: c.shape, claim: k }) != Fault::Ok {
            return false;
        }
        if k.claims_signed() != a.default {
            return false;
        }
        if !k.self_consistent() && j != 5usize {
            // 死槽 6（联签维）的申报按定义不自洽；其余同构申报必须自洽
            return false;
        }
        let flipped = claim_of(&c, Fault::Default);
        if flipped.default == a.default {
            return false;
        }
        if flipped.success() != a.ok() {
            return false;
        }
        j += 1;
    }
    // 四种联签档都要能点名，且只有 Agree 放行
    let signs = [Sign::Agree, Sign::Disagree, Sign::Absent, Sign::ChildOwn];
    let mut m = 0usize;
    while m < signs.len() {
        let sg = match signs.get(m) {
            Some(v) => *v,
            None => return false,
        };
        let mut s = match shape_at(0usize) {
            Some(v) => v,
            None => return false,
        };
        s.sign = sg;
        let a = define(&s);
        if sg.is_agree() {
            if a.note != TextNote::PlainNotFound || !a.ok() {
                return false;
            }
        } else {
            if a.note != TextNote::SignBreak || a.ok() {
                return false;
            }
            if a.stage != Stage::Sign {
                return false;
            }
            if a.tier() != Tier::Family {
                return false;
            }
            // 联签断裂不许改数：现场三通道保持观测值
            if a.value != 19 || a.code != CH_ENOENT || !a.default {
                return false;
            }
        }
        m += 1;
    }
    // 126 档的行不联签：蕴含数决定联签位，不看注解
    let mut n = 0usize;
    while n < CORPUS {
        let s = match shape_at(n) {
            Some(v) => v,
            None => return false,
        };
        let o = match outcome_at(n) {
            Some(v) => v,
            None => return false,
        };
        let want = s.phrase.implied_exit(s.tok) == EXIT_NOT_FOUND;
        if o.default != want {
            return false;
        }
        n += 1;
    }
    // 联签位不能倒推文案：同一数字面可以是四种句族
    if CH_EISDIR != 21i32 || CH_EACCES != 13i32 {
        return false;
    }
    true
}

/// TX09 六维对判与注入方向。
fn judge_ok() -> bool {
    if CORPUS != 14usize || ACCEPT != 8usize || REJECT != 6usize {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(v) => v,
            None => return false,
        };
        if c.tag() != c.shape.tag || c.tag().is_empty() {
            return false;
        }
        let want = match expected(i) {
            Some(v) => v,
            None => return false,
        };
        let got = c.judge();
        if got != want || judge(&c) != got {
            return false;
        }
        if got == Fault::Ok {
            if i >= ACCEPT || !c.ok() {
                return false;
            }
        } else {
            if i < ACCEPT {
                return false;
            }
            if !got.is_claim_fault() || !got.alive() {
                return false;
            }
            if c.ok() {
                return false;
            }
        }
        // 六维必须逐维可达：注哪一维就红哪一维
        let dims = [
            Fault::Estate,
            Fault::Stage,
            Fault::Code,
            Fault::Value,
            Fault::Note,
            Fault::Default,
        ];
        let mut d = 0usize;
        while d < dims.len() {
            let f = match dims.get(d) {
                Some(v) => *v,
                None => return false,
            };
            if c.actual().note == TextNote::NotOurLine && f == Fault::Note {
                d = d.saturating_add(1usize);
                continue;
            }
            let k = claim_of(&c, f);
            let t = Case { shape: c.shape, claim: k };
            if judge(&t) != f {
                return false;
            }
            if first_diff(&t) != fault_ndx(f).saturating_sub(2usize) {
                return false;
            }
            if fault_ndx(f) < 2usize {
                return false;
            }
            d = d.saturating_add(1usize);
        }
        i += 1;
    }
    // Estate 压过其余五维：槽 8 的申报同时错四问，只许报 ESTATE
    let s8 = match case_at(8usize) {
        Some(v) => v,
        None => return false,
    };
    if s8.actual().ok() || s8.claim.success() {
        return false;
    }
    if first_diff(&s8) != 0usize || dims_differ(&s8) != 3usize {
        return false;
    }
    if judge(&s8) != Fault::Estate || dim_name(0) != "estate" {
        return false;
    }
    // 六槽各歪一维；每槽的缺陷码互不相同
    let mut seen = [0u32; REJECT];
    let mut k = 0usize;
    while k < REJECT {
        let c = match case_at(ACCEPT + k) {
            Some(v) => v,
            None => return false,
        };
        if dims_differ(&c) != 1usize {
            return false;
        }
        let f = match expected(ACCEPT + k) {
            Some(v) => v,
            None => return false,
        };
        if first_diff(&c) != fault_ndx(f).saturating_sub(2usize) {
            return false;
        }
        if dim_name(first_diff(&c)) != f.name() {
            return false;
        }
        let code = f.code();
        let mut m = 0usize;
        while m < k {
            if let Some(prev) = seen.get(m) {
                if *prev == code {
                    return false;
                }
            }
            m += 1;
        }
        if let Some(slot) = seen.get_mut(k) {
            *slot = code;
        }
        k += 1;
    }
    // 六维的名字与 REJECT_CODES 次序互异
    let dimnames = ["estate", "gate", "errno", "bytes", "note", "sign"];
    let mut dn = 0usize;
    while dn < dimnames.len() {
        let mut di = dn.saturating_add(1usize);
        while di < dimnames.len() {
            if let (Some(x), Some(y)) = (dimnames.get(dn), dimnames.get(di)) {
                if x == y {
                    return false;
                }
            }
            di += 1;
        }
        dn += 1;
    }
    // 第二把尺：现场自洽 + 申报自洽，与 judge 正交
    let mut rf = 0usize;
    let mut p = 0usize;
    while p < CORPUS {
        let c = match case_at(p) {
            Some(v) => v,
            None => return false,
        };
        let cf = channel_fault(&c);
        if c.claim.self_consistent() && c.shape.forms_agree() && !c.actual().is_foreign() {
            if cf != Fault::Ok {
                return false;
            }
        } else if cf != Fault::Channel {
            return false;
        }
        if cf.alive() {
            rf += 1;
            if cf != Fault::Channel || cf.code() != TX_ECHANNEL {
                return false;
            }
        }
        if cf == Fault::Ok && !c.claim.self_consistent() {
            return false;
        }
        p += 1;
    }
    if rf != 3usize {
        return false;
    }
    // 对判方向：全绿 + 逐槽注入
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if !r.green() || r.matched != CORPUS as u32 || r.rate() != 1000 {
        return false;
    }
    if r.accept != ACCEPT as u32 || r.reject != REJECT as u32 {
        return false;
    }
    let mut q = 0usize;
    while q < CORPUS {
        match run_with(Some((q, Fault::Ok))) {
            Ok(x) => {
                if x.matched != (CORPUS as u32 - 1u32) || x.green() || x.rate() >= 1000 {
                    return false;
                }
                let flat = match expected(q) {
                    Some(v) => v == Fault::Ok,
                    None => true,
                };
                if q < ACCEPT {
                    if x.reject != REJECT as u32 {
                        return false;
                    }
                } else if flat {
                    return false;
                } else if x.reject != REJECT.saturating_add(1) as u32 || x.accept != ACCEPT.saturating_sub(1) as u32 {
                    return false;
                }
                if x.total != CORPUS as u32 || x.tier_sum() != x.total {
                    return false;
                }
            }
            Err(_) => return false,
        }
        q += 1;
    }
    if let Err(c) = run_with(Some((CORPUS, Fault::Ok))) {
        if c != TX_ESLOT {
            return false;
        }
    } else {
        return false;
    }
    if let Err(c) = run_with(Some((99usize, Fault::Stage))) {
        if c != TX_ESLOT {
            return false;
        }
    } else {
        return false;
    }
    true
}

/// TX10 语料结构自检（14 槽的形态分布）。
fn corpus_ok() -> bool {
    if CORPUS != 14usize || SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS {
        return false;
    }
    if WANT_TAGS.len() != CORPUS || WANT_BYTES.len() != CORPUS || WANT_NDX.len() != CORPUS {
        return false;
    }
    if WANT_STAGE.len() != CORPUS || WANT_CODE.len() != CORPUS || WANT_NOTE.len() != CORPUS {
        return false;
    }
    if WANT_DEFAULT.len() != CORPUS {
        return false;
    }
    let mut i = 0usize;
    let mut flavors = [0usize; 5];
    let mut phrases = [0usize; 6];
    let mut streams = [0usize; 4];
    let mut toks = [0usize; 5];
    let mut segs = [0usize; 3];
    let mut quotes = [0usize; 4];
    let mut interactive = 0usize;
    let mut bash_non = 0usize;
    let mut strerr = 0usize;
    let mut slashed = 0usize;
    let mut upper = 0usize;
    while i < CORPUS {
        let s = match SHAPES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.tag.is_empty() || s.body.is_empty() {
            return false;
        }
        if s.locale != Locale::C {
            return false;
        }
        if s.argv0.is_empty() || s.prefix.is_empty() {
            return false;
        }
        if text_len(s.argv0) > LINE_MAX || text_len(s.prefix) > PREFIX_MAX {
            return false;
        }
        if text_len(s.body) > LINE_MAX {
            return false;
        }
        if text_len(s.word) > WORD_MAX {
            return false;
        }
        if newline_count(s.body) != 1usize {
            return false;
        }
        if s.line_no != 1 && s.line_no != 3 {
            return false;
        }
        // 标签不许重名
        let mut j = i.saturating_add(1usize);
        while j < CORPUS {
            if let Some(t) = SHAPES.get(j) {
                if t.tag == s.tag || t.body == s.body && t.tag != s.tag && j > ACCEPT {
                    if t.tag == s.tag {
                        return false;
                    }
                }
            }
            j += 1;
        }
        // 期望标签表与本表逐位一致
        if let Some(w) = WANT_TAGS.get(i) {
            if *w != s.tag {
                return false;
            }
        } else {
            return false;
        }
        // 分布计数
        bump(&mut flavors, flavor_ndx(s.flavor));
        bump(&mut phrases, phrase_ndx(s.phrase));
        bump(&mut streams, stream_ndx(s.stream));
        bump(&mut toks, tok_ndx(s.tok));
        bump(&mut segs, seg_ndx(s.segkind));
        bump(&mut quotes, quote_ndx(s.quote));
        if s.interactive {
            interactive += 1;
        }
        if s.flavor == Flavor::Bash && !s.interactive {
            bash_non += 1;
        }
        if s.phrase.uses_strerror() {
            strerr += 1;
        }
        if word_has_slash(s.word) {
            slashed += 1;
        }
        if has_upper(s.word) {
            upper += 1;
        }
        i += 1;
    }
    // 逐维分布：这些数就是本卡的语料形状，改动语料必须同步这里的判据
    if cell(&flavors, flavor_ndx(Flavor::Ash)) != 5usize {
        return false;
    }
    if cell(&flavors, flavor_ndx(Flavor::Bash)) != 5usize {
        return false;
    }
    if cell(&flavors, flavor_ndx(Flavor::Dash)) != 3usize {
        return false;
    }
    if cell(&flavors, flavor_ndx(Flavor::BusyboxApplet)) != 1usize {
        return false;
    }
    if cell(&flavors, flavor_ndx(Flavor::Unknown)) != 1usize {
        return false;
    }
    if cell(&phrases, phrase_ndx(Phrase::NotFound)) != 8usize {
        return false;
    }
    if cell(&phrases, phrase_ndx(Phrase::CommandNotFound)) != 3usize {
        return false;
    }
    if cell(&phrases, phrase_ndx(Phrase::CantOpen)) != 1usize {
        return false;
    }
    if cell(&phrases, phrase_ndx(Phrase::SysError)) != 2usize {
        return false;
    }
    if cell(&phrases, phrase_ndx(Phrase::AppletNotFound)) != 1usize {
        return false;
    }
    if cell(&phrases, phrase_ndx(Phrase::Foreign)) != 0usize {
        return false;
    }
    if cell(&streams, stream_ndx(Stream::Stderr)) != 13usize
        || cell(&streams, stream_ndx(Stream::Stdout)) != 1usize
    {
        return false;
    }
    if cell(&toks, tok_ndx(ErrnoTok::Absent)) != 10usize
        || cell(&toks, tok_ndx(ErrnoTok::Enoent)) != 2usize
        || cell(&toks, tok_ndx(ErrnoTok::Eisdir)) != 1usize
        || cell(&toks, tok_ndx(ErrnoTok::Other)) != 1usize
    {
        return false;
    }
    if cell(&segs, seg_ndx(SegKind::None)) != 11usize
        || cell(&segs, seg_ndx(SegKind::LineWord)) != 2usize
        || cell(&segs, seg_ndx(SegKind::BareLine)) != 2usize
    {
        return false;
    }
    if cell(&quotes, quote_ndx(Quote::Bare)) != 12usize
        || cell(&quotes, quote_ndx(Quote::Single)) != 2usize
        || cell(&quotes, quote_ndx(Quote::Double)) != 0usize
        || cell(&quotes, quote_ndx(Quote::Absent)) != 0usize
    {
        return false;
    }
    if interactive != 2usize {
        return false;
    }
    if bash_non != 3usize {
        return false;
    }
    if strerr != 4usize {
        return false;
    }
    if slashed != 2usize {
        return false;
    }
    if upper != 2usize {
        return false;
    }
    // 流档：全仓只有一条漏进 stdout 的行（归属槽之外）
    let mut stderr_all = true;
    let mut m = 0usize;
    while m < ACCEPT {
        if let Some(s) = SHAPES.get(m) {
            if s.stream != Stream::Stderr {
                stderr_all = false;
            }
            // 活槽的前缀必须由 argv[0] 派生
            if msg_prefix(s.argv0) != s.prefix {
                return false;
            }
        }
        m += 1;
    }
    if !stderr_all {
        return false;
    }
    true
}

/// 分布计数辅助。
fn bump(a: &mut [usize], i: usize) {
    if let Some(x) = a.get_mut(i) {
        *x += 1;
    }
}

/// 分布读取辅助。
fn cell(a: &[usize], i: usize) -> usize {
    match a.get(i) {
        Some(x) => *x,
        None => 999,
    }
}

fn flavor_ndx(f: Flavor) -> usize {
    match f {
        Flavor::Ash => 0usize,
        Flavor::Bash => 1usize,
        Flavor::Dash => 2usize,
        Flavor::BusyboxApplet => 3usize,
        Flavor::Unknown => 4usize,
    }
}

fn stream_ndx(s: Stream) -> usize {
    match s {
        Stream::Stderr => 0usize,
        Stream::Stdout => 1usize,
        Stream::Both => 2usize,
        Stream::Nowhere => 3usize,
    }
}

fn seg_ndx(s: SegKind) -> usize {
    match s {
        SegKind::None => 0usize,
        SegKind::BareLine => 1usize,
        SegKind::LineWord => 2usize,
    }
}

fn quote_ndx(q: Quote) -> usize {
    match q {
        Quote::Bare => 0usize,
        Quote::Single => 1usize,
        Quote::Double => 2usize,
        Quote::Absent => 3usize,
    }
}

fn tok_ndx(t: ErrnoTok) -> usize {
    match t {
        ErrnoTok::Absent => 0usize,
        ErrnoTok::Enoent => 1usize,
        ErrnoTok::Eacces => 2usize,
        ErrnoTok::Eisdir => 3usize,
        ErrnoTok::Other => 4usize,
    }
}

/// TX11 期望表与申报表的交叉校验。
fn slots_ok() -> bool {
    if CORPUS != 14usize {
        return false;
    }
    let mut i = 0usize;
    let mut signed = 0usize;
    while i < CORPUS {
        let s = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let a = match outcome_at(i) {
            Some(v) => v,
            None => return false,
        };
        let k = match claim_at(i) {
            Some(v) => v,
            None => return false,
        };
        let st = match WANT_STAGE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let cd = match WANT_CODE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let by = match WANT_BYTES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let dx = match WANT_NDX.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let nt = match WANT_NOTE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let df = match WANT_DEFAULT.get(i) {
            Some(v) => *v,
            None => return false,
        };
        // 期望表对实际结果逐维校验
        if a.stage != st || a.code != cd || a.note != nt || a.default != df {
            return false;
        }
        if a.value != by || text_len(s.body) != by as usize {
            return false;
        }
        if s.echo_index() != dx {
            return false;
        }
        if by == 0 || by > LINE_MAX as u32 {
            return false;
        }
        if dx >= by {
            return false;
        }
        if a.tier() != Tier::of(&a) {
            return false;
        }
        if a.default {
            signed += 1;
        }
        // 活槽：申报表与实际结果逐位相同；死槽：只许差一维
        if i < ACCEPT {
            if k.stage != a.stage || k.code != a.code || k.value != a.value {
                return false;
            }
            if k.note != a.note || k.default != a.default || !a.ok() {
                return false;
            }
            if !k.self_consistent() {
                return false;
            }
        } else {
            if dims_differ(&Case { shape: s, claim: k }) != 1usize {
                return false;
            }
            if k == mk_claim(&a) {
                return false;
            }
        }
        i += 1;
    }
    // 联签位数：活槽 7 真 1 假（126 档那条），死槽 4 真（槽 8 蕴含 0 数）
    if signed != 11usize {
        return false;
    }
    // 期望缺陷表本身
    if REJECT_FAULTS.len() != REJECT || VALS.len() != ACCEPT {
        return false;
    }
    let mut j = 0usize;
    while j < REJECT {
        if let Some(f) = REJECT_FAULTS.get(j) {
            if !f.is_claim_fault() || fault_ndx(*f) < 2usize {
                return false;
            }
            if expected(ACCEPT + j) != Some(*f) {
                return false;
            }
        }
        if let Some(c) = REJECT_CODES.get(j) {
            if *c == 0 || code_name(*c) == "unknown" {
                return false;
            }
        }
        j += 1;
    }
    if expected(0usize) != Some(Fault::Ok) || expected(CORPUS) != None {
        return false;
    }
    if expected(ACCEPT.saturating_sub(1usize)) != Some(Fault::Ok) {
        return false;
    }
    if expected(ACCEPT) != Some(Fault::Estate) || expected(CORPUS.saturating_sub(1usize)) != Some(Fault::Default) {
        return false;
    }
    // 尺值：VALS 第 8 格是注入后的红档率，其余七格取自全绿报表
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if VALS.get(0) != Some(&r.total)
        || VALS.get(1) != Some(&r.accept)
        || VALS.get(2) != Some(&r.reject)
        || VALS.get(3) != Some(&r.grammar)
        || VALS.get(4) != Some(&r.structure)
        || VALS.get(5) != Some(&r.family)
        || VALS.get(6) != Some(&r.foreign)
    {
        return false;
    }
    if let Some(x) = VALS.get(7usize) {
        if *x != red_rate() || *x >= 1000 {
            return false;
        }
    } else {
        return false;
    }
    if r.tier_sum() != r.total {
        return false;
    }
    true
}

/// TX12 矩阵覆盖、报表文本与红档显式。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    if m.slots() != CORPUS {
        return false;
    }
    if m.goods() != ACCEPT || m.reds() != REJECT {
        return false;
    }
    if !m.covered() {
        return false;
    }
    if m.perfect_rows() != 1usize {
        return false;
    }
    let g = match m.row(Tier::Grammar) {
        Some(v) => v,
        None => return false,
    };
    if g.total != 7usize || g.good != 5usize || g.red() != 2usize || g.perfect() {
        return false;
    }
    let st = match m.row(Tier::Structure) {
        Some(v) => v,
        None => return false,
    };
    if st.total != 2usize || st.good != 0usize || st.red() != 2usize || st.perfect() {
        return false;
    }
    let fa = match m.row(Tier::Family) {
        Some(v) => v,
        None => return false,
    };
    if fa.total != 3usize || fa.good != 3usize || fa.red() != 0usize || !fa.perfect() {
        return false;
    }
    let fo = match m.row(Tier::Foreign) {
        Some(v) => v,
        None => return false,
    };
    if fo.total != 2usize || fo.good != 0usize || fo.red() != 2usize {
        return false;
    }
    if m.grammar_reds() != 2usize || m.foreign_reds() != 2usize {
        return false;
    }
    if m.rows.len() != TIER_COUNT {
        return false;
    }
    if m.row(Tier::at(9usize)).is_some() {
        return false;
    }
    if m.render() != MATRIX_LIT {
        return false;
    }
    if text_len(MATRIX_LIT) != 117usize {
        return false;
    }
    // 全绿报表与逐字文本
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if r.render() != RENDER_OK {
        return false;
    }
    if text_len(RENDER_OK) != 105usize {
        return false;
    }
    if r.total != CORPUS as u32
        || r.accept != ACCEPT as u32
        || r.reject != REJECT as u32
        || r.matched != CORPUS as u32
    {
        return false;
    }
    if r.grammar != 7 || r.structure != 2 || r.family != 3 || r.foreign != 2 {
        return false;
    }
    if r.tier_sum() != r.total || r.table_cells() != 10 || r.table_share() != 71 {
        return false;
    }
    if !r.green() || r.gate() != Ok(()) || r.rate() != 1000 {
        return false;
    }
    if r.success_cells() != ACCEPT as u32 {
        return false;
    }
    // 注入后的报表：现场面四格不动，申报面全动
    let bad = match run_with(Some((0usize, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if bad.matched != 13 || bad.accept != 7 || bad.reject != 7 || bad.rate() != 929 {
        return false;
    }
    if bad.green() || bad.gate() != Err(TX_ECOUNT) {
        return false;
    }
    if bad.grammar != r.grammar || bad.structure != r.structure {
        return false;
    }
    if bad.family != r.family || bad.foreign != r.foreign || bad.total != r.total {
        return false;
    }
    if red_rate() != 929 || red_rate() != bad.rate() {
        return false;
    }
    // 空报表的比率兜底与闸门
    let z = Report {
        total: 0,
        accept: 0,
        reject: 0,
        grammar: 0,
        structure: 0,
        family: 0,
        foreign: 0,
        matched: 0,
    };
    if z.rate() != 0 || z.green() || z.gate() != Err(TX_ECOUNT) || z.table_share() != 0 {
        return false;
    }
    if z.tier_sum() != 0 || z.table_cells() != 0 {
        return false;
    }
    // 半绿报表不过闸
    let half = Report {
        total: CORPUS as u32,
        accept: ACCEPT as u32,
        reject: REJECT as u32,
        grammar: 7,
        structure: 2,
        family: 3,
        foreign: 2,
        matched: (CORPUS as u32) - 1u32,
    };
    if half.green() || half.gate() != Err(TX_ECOUNT) || half.rate() >= 1000 {
        return false;
    }
    // 覆盖计数只对 CORPUS 放行
    if cover(CORPUS) != Ok(()) || cover(13usize) != Err(TX_ENOBS) || cover(15usize) != Err(TX_ENOBS) {
        return false;
    }
    // 行渲染的字节形状
    let one = Row { total: 3, good: 1 };
    if one.render(Tier::Family) != "family total=3 good=1 red=2\n" {
        return false;
    }
    if one.red() != 2usize || one.perfect() {
        return false;
    }
    let inv = Row { total: 1, good: 2 };
    if inv.red() != 1usize || inv.perfect() {
        return false;
    }
    if Row::blank().total != 0 || Row::blank().perfect() || Row::blank().red() != 0 {
        return false;
    }
    if Matrix::blank().covered() || Matrix::blank().slots() != 0 {
        return false;
    }
    if Matrix::blank().goods() != 0 || Matrix::blank().perfect_rows() != 0 {
        return false;
    }
    if Matrix::blank().grammar_reds() != 0 || Matrix::blank().foreign_reds() != 0 {
        return false;
    }
    // 四个档名互不相同且按 index 回查闭合
    let mut t = 0usize;
    while t < TIER_COUNT {
        let ti = Tier::at(t);
        if ti.index() != t || ti.name().is_empty() {
            return false;
        }
        if ti.success() != (t == 0usize) {
            return false;
        }
        let mut u = t.saturating_add(1usize);
        while u < TIER_COUNT {
            if let Some(w) = Tier::at(u).name().get(0..) {
                if w == ti.name() {
                    return false;
                }
            }
            u = u.saturating_add(1usize);
        }
        t += 1;
    }
    if Tier::at(99usize) != Tier::Foreign || Tier::count() != TIER_COUNT {
        return false;
    }
    true
}

/// TX13 状态面文本与逐字锚。
fn message_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(v) => v,
            None => return false,
        };
        let a = c.actual();
        let m = message(&c.shape, &a);
        if m.is_empty() || text_len(m.as_str()) > 600usize {
            return false;
        }
        // 状态面必须以观测整行（去行末换行）起头——"这一行是什么"的自述
        if !starts_with(m.as_str(), msg_body_trim(&c.shape)) {
            return false;
        }
        if newline_count(m.as_str()) != 0usize {
            return false;
        }
        let keys = ["note=", "gate=", "bytes=", "errno=", "sign=", "ok="];
        let mut k = 0usize;
        while k < keys.len() {
            if let Some(w) = keys.get(k) {
                if !contains_text(m.as_str(), w) {
                    return false;
                }
            }
            k += 1;
        }
        if !contains_text(m.as_str(), a.note.name()) || !contains_text(m.as_str(), a.stage.name()) {
            return false;
        }
        if a.ok() != contains_text(m.as_str(), "ok=yes") {
            return false;
        }
        let tail = message_tail(&c.shape);
        if !starts_with(tail.as_str(), msg_line_trim(&c.shape).as_str()) {
            return false;
        }
        if !contains_text(tail.as_str(), " | ") || !contains_text(tail.as_str(), c.shape.locale.name()) {
            return false;
        }
        if text_len(tail.as_str()) > 600usize {
            return false;
        }
        // 释义：标签打头、申报三元、缺陷短名收尾
        let d = detail(&c);
        if !starts_with(d.as_str(), c.shape.tag) {
            return false;
        }
        if !contains_text(d.as_str(), msg_body_trim(&c.shape)) {
            return false;
        }
        if !contains_text(d.as_str(), " claim=") || !contains_text(d.as_str(), " fault=") {
            return false;
        }
        if !contains_text(d.as_str(), c.claim.stage.name()) {
            return false;
        }
        if !contains_text(d.as_str(), fault_short(judge(&c))) {
            return false;
        }
        let want_head = format_head(&c, &m);
        if d != want_head {
            return false;
        }
        // 尾行：位图 + 回显偏移 + 联签位
        let dt = detail_tail(&c);
        if !starts_with(dt.as_str(), "map ") || !contains_text(dt.as_str(), " echo@") {
            return false;
        }
        if !contains_text(dt.as_str(), msg_bytes_map(&c.shape).as_str()) {
            return false;
        }
        if !contains_text(dt.as_str(), dec_text(c.shape.echo_index()).as_str()) {
            return false;
        }
        if !contains_text(dt.as_str(), " signed ") {
            return false;
        }
        if !contains_text(dt.as_str(), bool_text(a.default)) {
            return false;
        }
        if text_len(dt.as_str()) > 200usize || newline_count(dt.as_str()) != 0usize {
            return false;
        }
        // 申报文本与指纹
        if !contains_text(c.claim.text().as_str(), c.claim.stage.name()) {
            return false;
        }
        if !contains_text(c.claim.text().as_str(), c.claim.note.name()) {
            return false;
        }
        i += 1;
    }
    // 逐字主锚：状态面里出现的行必须与语料表原文一致
    let s0 = match shape_at(0usize) {
        Some(v) => v,
        None => return false,
    };
    if s0.body != ASH_FOO_LINE {
        return false;
    }
    if msg_body_trim(&s0) != "sh: foo: not found" {
        return false;
    }
    if !contains_text(message(&s0, &define(&s0)).as_str(), ASH_TAIL) {
        return false;
    }
    let s1 = match shape_at(1usize) {
        Some(v) => v,
        None => return false,
    };
    if s1.body != BASH_C_LINE {
        return false;
    }
    let s3 = match shape_at(3usize) {
        Some(v) => v,
        None => return false,
    };
    if s3.body != DASH_FOO_LINE {
        return false;
    }
    // 指纹：现场全等则指纹等，改一维则指纹变
    let x = match shape_at(0usize) {
        Some(v) => v,
        None => return false,
    };
    let mut y = x;
    if x.seal() != y.seal() {
        return false;
    }
    y.quote = Quote::Single;
    if x.seal() == y.seal() {
        return false;
    }
    let mut z = x;
    z.locale = Locale::Posix;
    if x.seal() == z.seal() {
        return false;
    }
    let o = match outcome_at(0usize) {
        Some(v) => v,
        None => return false,
    };
    if o.seal() != mk_claim(&o).seal() {
        return false;
    }
    if mk_claim(&o).seal() == claim_of(&Case { shape: x, claim: mk_claim(&o) }, Fault::Value).seal() {
        return false;
    }
    // 空白现场的状态面不得自称规范行
    let b = Shape::blank();
    let ob = define(&b);
    let mb = message(&b, &ob);
    if ob.ok() || contains_text(mb.as_str(), "ok=yes") || !contains_text(mb.as_str(), "not-our-line") {
        return false;
    }
    if text_len(mb.as_str()) > 200usize {
        return false;
    }
    true
}

/// 复算一槽释义的首段（tag + 空格 + 状态面），用来钉住 detail 的字节结构。
fn format_head(c: &Case, m: &str) -> String {
    let mut s = String::from(c.shape.tag);
    s.push(' ');
    s.push_str(m);
    s.push_str(" claim=");
    s.push_str(c.claim.stage.name());
    s.push(':');
    s.push_str(c.claim.note.name());
    s.push(':');
    s.push_str(dec_text(c.claim.value).as_str());
    s.push_str(" fault=");
    s.push_str(fault_short(judge(c)));
    s
}

/// TX14 阶段闸门的单调与关闭语义。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    if g.steps() != 0 || g.last() != -1 || g.closed() {
        return false;
    }
    let mut i = 0usize;
    while i < STAGE_COUNT {
        let s = match Stage::from_order(i) {
            Some(v) => v,
            None => return false,
        };
        if g.step(s) != Ok(()) {
            return false;
        }
        if g.steps() != (i + 1usize) as u32 {
            return false;
        }
        if g.last() != i as i32 {
            return false;
        }
        if g.closed() {
            return false;
        }
        i += 1;
    }
    let n = match g.close() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if n != STAGE_COUNT as u32 || !g.closed() {
        return false;
    }
    if g.step(Stage::Sign) != Err(TX_ESTATE) || g.close() != Err(TX_ESTATE) {
        return false;
    }
    // 同段重复不算回退
    let mut h = Gate::new();
    if h.step(Stage::Echo) != Ok(()) || h.step(Stage::Echo) != Ok(()) {
        return false;
    }
    if h.last() != 4 || h.steps() != 2 {
        return false;
    }
    let mut back = Gate::new();
    let _ = back.step(Stage::Phrase);
    if back.step(Stage::Echo) != Err(TX_ESTAGE) {
        return false;
    }
    if back.step(Stage::Sign) != Ok(()) {
        return false;
    }
    let _ = back.close();
    if back.step(Stage::Prefix) != Err(TX_ESTATE) {
        return false;
    }
    // 语料里每一槽的实际闸段都能被单调走到
    let mut k = 0usize;
    while k < CORPUS {
        let o = match outcome_at(k) {
            Some(v) => v,
            None => return false,
        };
        let mut gg = Gate::new();
        let mut m = 0usize;
        while m <= o.stage.order() {
            let s = match Stage::from_order(m) {
                Some(v) => v,
                None => return false,
            };
            if gg.step(s) != Ok(()) {
                return false;
            }
            m += 1;
        }
        if gg.close() != Ok((o.stage.order() + 1usize) as u32) {
            return false;
        }
        k += 1;
    }
    // 八闸的专属码互不相同，且都属于本卡码档
    let mut seen = [0u32; STAGE_COUNT];
    let mut a = 0usize;
    while a < STAGE_COUNT {
        let s = match Stage::from_order(a) {
            Some(v) => v,
            None => return false,
        };
        let c = s.code();
        if c == 0 {
            return false;
        }
        let mut hit = 0usize;
        let mut b = 0usize;
        while b < CODE_TABLE.len() {
            if let Some(w) = CODE_TABLE.get(b) {
                if *w == c {
                    hit += 1;
                }
            }
            b += 1;
        }
        if hit != 1usize {
            return false;
        }
        let mut d = 0usize;
        while d < a {
            if let Some(prev) = seen.get(d) {
                if *prev == c {
                    return false;
                }
            }
            d += 1;
        }
        if let Some(slot) = seen.get_mut(a) {
            *slot = c;
        }
        a += 1;
    }
    // 字节闸七道 + 账闸一道
    let mut bytes = 0usize;
    let mut e = 0usize;
    while e < STAGE_COUNT {
        if let Some(s) = Stage::from_order(e) {
            if s.is_byte_gate() {
                bytes += 1;
            }
        }
        e += 1;
    }
    if bytes != 7usize {
        return false;
    }
    if Stage::count() != STAGE_COUNT {
        return false;
    }
    true
}

/// TX15 账本的登记、对账与冻结重放。
fn ledger_ok() -> bool {
    let mut led = build_ledger();
    if led.len() != SAMPLES || led.is_empty() {
        return false;
    }
    if led.ok_count() != ACCEPT || led.err_count() != REJECT {
        return false;
    }
    if led.audit() != Ok(()) {
        return false;
    }
    if led.rejects() != 0 || led.root() != 0 || led.is_frozen() {
        return false;
    }
    // 槽位错位一律拒绝并计数，且不污染账本
    let junk = [
        Sample { slot: 3, op: OP_QUOTE, ok: true, code: 0, value: 1 },
        Sample { slot: 0, op: OP_QUOTE, ok: true, code: 0, value: 1 },
        Sample { slot: 20, op: OP_QUOTE, ok: true, code: 0, value: 1 },
        Sample { slot: 255, op: OP_QUOTE, ok: false, code: 9, value: 0 },
    ];
    let mut i = 0usize;
    while i < junk.len() {
        if let Some(s) = junk.get(i) {
            if led.register(*s) != Err(TX_ESLOT) {
                return false;
            }
        }
        i += 1;
    }
    if led.rejects() != 4 || led.len() != SAMPLES {
        return false;
    }
    if led.register(Sample { slot: 14, op: OP_QUOTE, ok: true, code: 0, value: 0 }) != Err(TX_ESLOT) {
        return false;
    }
    if led.rejects() != 5 || led.audit() != Ok(()) {
        return false;
    }
    // 逐格：槽位连续、成格值等于 VALS、败格码等于 REJECT_CODES 且败格值为零
    let mut vi = 0usize;
    let mut ci = 0usize;
    let mut k = 0usize;
    while k < SAMPLES {
        let s = match led.sample(k) {
            Some(v) => v,
            None => return false,
        };
        if s.slot as usize != k {
            return false;
        }
        if s.ok {
            if s.code != 0 {
                return false;
            }
            match VALS.get(vi) {
                Some(w) => {
                    if s.value != *w {
                        return false;
                    }
                }
                None => return false,
            }
            vi += 1;
        } else {
            if s.value != 0 {
                return false;
            }
            match REJECT_CODES.get(ci) {
                Some(w) => {
                    if s.code != *w {
                        return false;
                    }
                }
                None => return false,
            }
            if s.code != TX_ESTATE
                && s.code != TX_ESTAGE
                && s.code != TX_ECODE
                && s.code != TX_EVALUE
                && s.code != TX_ENOTE
                && s.code != TX_EDEFAULT
            {
                return false;
            }
            if code_name(s.code) == "unknown" {
                return false;
            }
            ci += 1;
        }
        if led.sample(k.saturating_add(SAMPLES)).is_some() {
            return false;
        }
        k += 1;
    }
    if vi != ACCEPT || ci != REJECT {
        return false;
    }
    if led.sample(SAMPLES).is_some() || led.sample(99usize).is_some() {
        return false;
    }
    if led.sample(0usize).is_none() {
        return false;
    }
    // 账本渲染：每格一行且带 bytes 位
    let text = led.render();
    if newline_count(text.as_str()) != SAMPLES {
        return false;
    }
    if !contains_text(text.as_str(), "slot 0 ") {
        return false;
    }
    if !contains_text(text.as_str(), " bytes 929") || !contains_text(text.as_str(), " bytes 14") {
        return false;
    }
    if !contains_text(text.as_str(), " bytes 0") {
        return false;
    }
    if !contains_text(text.as_str(), "gate ") || !contains_text(text.as_str(), "code ") {
        return false;
    }
    // 冻结后拒登记、拒二次冻结，重放指纹相等
    let root = match led.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if root == 0 || !led.is_frozen() {
        return false;
    }
    if led.root() != root {
        return false;
    }
    if led.register(Sample { slot: 14, op: OP_SIGN, ok: true, code: 0, value: 1 }) != Err(TX_ESLOT) {
        return false;
    }
    if led.freeze() != Err(TX_EBAL) || led.rejects() != 7 {
        return false;
    }
    if led.replay() != Ok(root) {
        return false;
    }
    if build_ledger().seal() != led.seal() {
        return false;
    }
    // 改一格就过不了对账
    let mut t = build_ledger();
    if let Some(row) = t.rows.get_mut(1usize) {
        row.value = row.value.saturating_add(1);
    }
    if t.audit() != Err(TX_EVALUE) {
        return false;
    }
    let mut u = build_ledger();
    if let Some(row) = u.rows.get_mut(9usize) {
        row.code = row.code.saturating_add(1);
    }
    if u.audit() != Err(TX_EVALUE) {
        return false;
    }
    let mut v = build_ledger();
    if let Some(row) = v.rows.get_mut(9usize) {
        row.value = 1;
    }
    if v.audit() != Err(TX_EVALUE) {
        return false;
    }
    let mut w = build_ledger();
    if let Some(row) = w.rows.get_mut(1usize) {
        row.code = 1;
    }
    if w.audit() != Err(TX_EVALUE) {
        return false;
    }
    // 空账本与半账本都不闭合
    let z = Ledger::new();
    if z.audit() != Err(TX_EBAL) || !z.is_empty() || z.ok_count() != 0 {
        return false;
    }
    if z.err_count() != 0 || z.sample(0usize).is_some() || z.seal() == build_ledger().seal() {
        return false;
    }
    if z.render().len() != 0 {
        return false;
    }
    let mut part = Ledger::new();
    let _ = part.register(Sample { slot: 0, op: OP_PREFIX, ok: true, code: 0, value: CORPUS as u32 });
    if part.audit() != Err(TX_EBAL) {
        return false;
    }
    // 样本基元
    let b = Sample::blank();
    if b.ok || b.code != 0 || b.value != 0 || b.slot != 0 || b.op != 0 {
        return false;
    }
    if b.seal() == led.sample(0usize).map(|x| x.seal()).unwrap_or(0) {
        return false;
    }
    if !contains_text(b.text().as_str(), "slot 0 ") {
        return false;
    }
    if contains_text(b.text().as_str(), "ok yes") {
        return false;
    }
    true
}

/// TX16 四路注入的红/绿方向。
fn inject_ok() -> bool {
    let f = inject_four();
    if f.len() != 4usize {
        return false;
    }
    if !four_true(f) {
        return false;
    }
    let mut i = 0usize;
    while i < f.len() {
        if let Some(x) = f.get(i) {
            if !x {
                return false;
            }
        }
        i += 1;
    }
    if all_true([true, true, true]) != true || all_true([true, false, true]) {
        return false;
    }
    if four_true([true, true, true, false]) {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 本卡的十三条判据
// ---------------------------------------------------------------------------

/// 跑本卡全部判据，返回判定集（红项自带详情）。
///
/// 十三条按"表 → 判 → 渲染 → 账"的顺序排：先钉常量与码档，再钉语法表与
/// 八闸，然后钉语料与期望表，最后才是逐字锚与冻结重放。
pub fn run_unx11665_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11665_sh_cmd_notfound_text");
    s.add(
        "TX01-名称与段基址",
        name_ok(),
        "SEG_BASE=0x6d1b0 起十三条码连续互异、四份逐字主锚的字节数与行号段档互斥、八闸各有专属码",
    );
    s.add(
        "TX02-错误码表与缺陷档",
        codes_ok(),
        "十三码互异可回查、十四档缺陷名互异、六维申报缺陷恰对应六个互异码、第 8 格是注入后的红档率",
    );
    s.add(
        "TX03-八闸次序与注解面",
        stages_ok(),
        "闸次序单调可回查、越界回 None、结构面注解只落前六闸、族属面注解先后四闸、语法四档各指一个句族",
    );
    s.add(
        "TX04-方言与句族语法表",
        grammar_ok(),
        "三方言共五格句族、行号段三档字节互斥、只有两族加单引号、busybox 短表只属 ash、蕴含数只有 0/126/127",
    );
    s.add(
        "TX05-四种形式逐字闭包",
        line_ok(),
        "整行拼装 = 逐段拼装 = 由 argv[0] 重派生 = 语料原文，归属面两槽刻意让原文与拼装背离、位图与偏移可复算",
    );
    s.add(
        "TX06-八闸纵线现场推演",
        define_ok(),
        "空行死在回显闸、前缀错与无换行死在归属闸、每道闸各有专属注解、判死仍保留观测三通道、哨兵只属空行",
    );
    s.add(
        "TX07-回显包裹与行号段",
        echo_ok(),
        "七种回显形态由字节复算、basename 与大小写只认逐字、双引号档真机不存在、`line ` 只属 bash 非交互",
    );
    s.add(
        "TX08-与 B30 的联签",
        sign_ok(),
        "联签位由句族蕴含数决定不看注解、四种联签档只有 Agree 放行、断签不许改现场三通道、本卡不生产数字",
    );
    s.add(
        "TX09-六维对判与注入方向",
        judge_ok(),
        "六维逐维可达、Estate 压过其余五维、每死槽恰差一维且缺陷码互异、通道自洽是正交的第二把尺",
    );
    s.add(
        "TX10-语料形态分布",
        corpus_ok(),
        "十四槽的方言/句族/流/errno 词/段档/引号档分布逐档点数、活槽前缀一律由 argv[0] 派生",
    );
    s.add(
        "TX11-期望表交叉校验",
        slots_ok(),
        "两张独立手写的表（期望结果与缺陷维序）互查每槽六维、活槽申报恰等实际、死槽恰差一维、VALS 取报表",
    );
    s.add(
        "TX12-矩阵覆盖与红档显式",
        matrix_ok(),
        "四档覆盖 7/2/3/2 且和回总槽数、族属面全对、红档摊在三个面上、报表与矩阵逐字文本钉住、注入后现场面不动",
    );
    s.add(
        "TX13-状态面锚与冻结重放",
        message_ok() && gate_ok() && ledger_ok() && inject_ok(),
        "状态面以观测整行起头、释义字节可复算、八闸单调可走满、14 格逐格对账后冻结重放指纹一致、四路注入皆红",
    );
    s
}
// __CHAP_G__
