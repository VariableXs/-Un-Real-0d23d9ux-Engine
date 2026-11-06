//! UNX-F11682 cp 磁盘满中止与退出码判据
//!
//! `cp` 是"把一个源件搬完"的工具，它面向用户的三样东西是**目的件留下的字节**、
//! **最后一句诊断文案**、**退出码**。F11681 把总锚钉在系统调用面（`write()`
//! 返回值 + errno），本卡把同一现场向上折算成工具面的三件事，并且必须解释
//! 一件 F11681 解释不了的事：**同样是 ENOSPC，为什么 GNU cp 有时退 1、有时
//! 退 0，而 busybox cp 甚至可能退 0**。
//!
//! 现场形态分成两条独立的轴，本卡把它们交在一起判：
//!
//! * **中止轴**：复制循环在写到一半时收到 `-1/ENOSPC`（GNU 报
//!   `cp: error writing '<dst>': No space left on device`，busybox 报
//!   `cp: write error to '<dst>': No space left on device`），目的件留在磁盘上，
//!   尺寸就是已经落进去的那部分字节——它不是"没复制"，是"复制了一半"。
//! * **延迟轴**：`write()` 全部当场放行（页缓存收下），真相迟到 `close()`
//!   才爆——GNU 从 9.x 起检查 `close` 返回值并打
//!   `cp: failed to close '<dst>': No space left on device` 且退 1；busybox 的
//!   `close()` 返回值被丢弃，于是**满盘丢块但退出码 0**。这两个是同一物理
//!   现场的两种工具结论，只有把 close 单列一道闸才分得开。
//!
//! 还有一条**尾段轴**：数据落定之后 `cp -a` 还要保留 mode/mtime/xattr/sparse。
//! 这一段失败**不改变目的件字节数**，只把退出码推到 1：
//! `cp: preserving times for '<dst>': Operation not permitted`、
//! `cp: preserving permissions for '<dst>': Operation not permitted`、
//! `cp: failed to copy xattrs`。尾段与中止段必须分档——把尾段错算成"半件留档"
//! 会让所有审计脚本误判数据完整性。
//!
//! 警告不是错误：GNU 在目的文件系统不支持稀疏时打
//! `cp: warning: behavior of cp is dependent on the file system's sparse support,
//! but it is not enabled for '/mnt/full', file system does not support sparse files`
//! 之后照常把字节写完，退出码 **0**；busybox 根本没有这条告警（它不支持 `-S`），
//! 于是"有没有告警"本身成了工具面差异，而不是现场差异。
//!
//! 稀疏源还要过 seek-vs-write 这关：`-S auto/always` 把洞 `lseek` 跳过，
//! 洞的字节**不进入写入计数**，目的件的"表观尺寸"等于源尺寸而"实占块数"小于
//! 源实占；一旦目的文件系统不支持稀疏，跳过就退化成写零，满盘时这部分零也要
//! 占额度——本卡把这条区分做成 `Holes` 记账，`stored()` 与 `spread()` 两个数
//! 不许互相顶替。
//!
//! 闸门次序（为什么不能倒）：
//!
//! `open → stat → copy → werr → cerr → pres → exit`
//!
//! 1. `open`：目的件没建成，后面一切计数都是空谈；建不出来时**留档字节必须
//!    为 0**，这条只能在 open 段判，走到 copy 段就丢了这个信息。
//! 2. `stat`：源/目的的类型与稀疏几何在这里定案。`cp logs` 没带 `-r` 时
//!    GNU 在 stat 段就拒了（`cp: -r not specified; omitting directory 'logs'`），
//!    busybox 到打开才红（`cp: 'logs': Is a directory`）——同一件事的两个
//!    落点，段序就是把它们分开的唯一手段。
//! 3. `copy`：循环本身；循环内出现"没有错误码却少了字节"就是**静默截断**，
//!    本卡判死——它正是"中途满盘却继续往下走"那种最危险的形态。
//! 4. `werr`：写错误归因。它必须在 copy 之后：只有先确认循环被中断，才谈得
//!    上"是哪一句文案中断的"；放在 copy 之前就会把静默截断也算成 ENOSPC。
//! 5. `cerr`：close 提交错误。**必须在 werr 之后**——写已经红了就不会再走到
//!    close，反过来 close 红的前提是写全绿；两闸一倒序，延迟分配现场就永远
//!    记不到 close 头上，busybox 丢块也就无从暴露。
//! 6. `pres`：属性尾段。它必须在 cerr 之后：数据没落定之前谈保留属性无意义，
//!    而 GNU 的实现正是先 close 再设属性；反过来会把"数据没写完 + 属性也没
//!    保住"的现场只报出后半截。
//! 7. `exit`：退出码是前面所有闸的聚合，只能最后算。任何"码 0"的申报都要
//!    在这里被回头核对：**只要 werr/cerr/pres/open/stat 任一段红过，码就不许 0**；
//!    只有 warn 一种情形例外。
//!
//! 关键差异（本卡深度所在）：
//!
//! * **F11681 不判的部分**：总锚只给"那一次 `write()`/`fsync()`/`close()` 的
//!   (返回, errno) 对"，它不知道 `cp` 中止后目的件留多大、不知道最后打的是
//!   `error writing` 还是 `failed to close`、更不知道进程退出码是几。这些都
//!   归本卡：本卡把锚的现场折算成"部分件字节 + 文案落点 + 退出码"三元组。
//! * **F11681 判的部分**（本卡不得重复判）：errno 与返回值是否成对、短写与
//!   满失败的区分、EFBIG/EDQUOT/EBADF/EIO 的分档。本卡的 `errno_seen` 字段
//!   只是把总锚的结论**引进来**，不重新裁定它的对错。
//! * **F11683 的边界**：`dd` 的块算术（records in/out、半块字节数、
//!   `N bytes copied`、`conv=noerror/sync/notrunc` 对计数的影响）全归 F11683；
//!   本卡**不数块**，只记"目的件留下多少字节"和"退出码是谁造成的"。cp 没有
//!   `conv=`，也没有块边界，把 dd 的计数搬进本卡会造成两卡对同一次满盘各报
//!   一套数字。
//! * **本卡的排他职责**：① 部分件留档的尺寸记账（`value`）；② 中止文案落点
//!   （`note`：写错/close 错/尾段错/告警）；③ 退出码归因（`code`：1 是文件错、
//!   2 是用法错、0 是纯告警）；④ busybox 丢 close 错误这一工具面差异。
//! * **谁判死谁放行**：现场是 ENOSPC 且工具报了写错或 close 错且退 1 → 放行
//!   （锚位命中）；现场是 ENOSPC 但工具静默续传或退出码 0（busybox 的 close
//!   忽略除外，那一格由本卡显式标注为"未归因"）→ 判死。
//!
//! 判据六维：结束阶段（gate）、退出码（code）、留档字节（value）、文案落点
//! （note）、中止归因（default：是否 ENOSPC 中止且码/文案同族）、成败申报
//! （estate）。判序固定：状态 → 阶段 → 退出码 → 字节 → 注解 → 归因。
//!
//! 语料 14 槽：8 槽申报正确（完成 1、告警 2、尾段 2、中止 3），6 槽复刻同一
//! "递归复制中途满盘"现场、各挂一维错报（estate/stage/code/value/note/default）。
//! 账本 14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码，冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11682）。
pub const SEG_BASE: u32 = 0x6d140;

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
// errno（只引进 F11681 的结论，本卡不重判）
// ---------------------------------------------------------------------------

/// 设备无空间（`No space left on device`），本卡唯一的中止源。
pub const ERR_ENOSPC: i32 = 28;
/// 权限不足（`Permission denied`），目的件建不出来的常见邻居。
pub const ERR_EACCES: i32 = 13;
/// 操作不允许（`Operation not permitted`），保留属主/时间戳的常见邻居。
pub const ERR_EPERM: i32 = 1;
/// 目标是目录（`Is a directory`），不带 `-r` 复制目录的落点。
pub const ERR_EISDIR: i32 = 21;
/// 配额超限（`Quota exceeded`），红但不是 ENOSPC。
pub const ERR_EDQUOT: i32 = 122;
/// 设备错（`Input/output error`），也不是 ENOSPC。
pub const ERR_EIO: i32 = 5;
/// 没有错误。
pub const ERR_NONE: i32 = 0;

/// 退出码：全绿。
pub const EXIT_OK: u32 = 0;
/// 退出码：某个文件出了问题（GNU 的 `Exit_success` 反面）。
pub const EXIT_FILE: u32 = 1;
/// 退出码：用法/选项层面的硬错（GNU 专属，busybox 退 1）。
pub const EXIT_HARD: u32 = 2;

/// 单个尺寸维的合法上界。
pub const SIZE_MAX: u32 = 0x0800_0000;

/// 合法 errno 集合的大小（0/28/13/1/21/122/5 七个）。
pub const ERRNO_SET_SIZE: usize = 7;

// ---------------------------------------------------------------------------
// 保留属性位图（cp -a/-p 的尾段维度）
// ---------------------------------------------------------------------------

/// 不保留任何属性。
pub const PRES_NONE: u8 = 0;
/// 保留权限位（mode）。
pub const PRES_MODE: u8 = 1;
/// 保留时间戳（mtime/atime）。
pub const PRES_TIME: u8 = 2;
/// 保留扩展属性（xattr/SELinux 上下文）。
pub const PRES_XATTR: u8 = 4;
/// 保留稀疏几何（hole）。
pub const PRES_SPARSE: u8 = 8;
/// `-a` 的保留全集（mode|mtime|xattr|sparse）。
pub const PRES_ALL: u8 = PRES_MODE | PRES_TIME | PRES_XATTR | PRES_SPARSE;
/// `-p` 的默认保留集（mode|mtime）。
pub const PRES_P: u8 = PRES_MODE | PRES_TIME;
/// 保留位的个数。
pub const PRES_BIT_COUNT: usize = 4;

// ---------------------------------------------------------------------------
// 错误码（13 个，SEG_BASE + 0x01..0x0d）
// ---------------------------------------------------------------------------

/// 阶段申报不符。
pub const CP_ESTAGE: u32 = SEG_BASE + 0x01;
/// 退出码申报不符。
pub const CP_ECODE: u32 = SEG_BASE + 0x02;
/// 留档字节申报不符。
pub const CP_EVALUE: u32 = SEG_BASE + 0x03;
/// 文案落点申报不符。
pub const CP_ENOTE: u32 = SEG_BASE + 0x04;
/// 中止归因申报不符。
pub const CP_EDEFAULT: u32 = SEG_BASE + 0x05;
/// 归因破断：文案、字节与退出码三者不同族。
pub const CP_EABORT: u32 = SEG_BASE + 0x06;
/// 成败互报。
pub const CP_ESTATE: u32 = SEG_BASE + 0x07;
/// 计数不符。
pub const CP_ECOUNT: u32 = SEG_BASE + 0x08;
/// 槽位非法。
pub const CP_ESLOT: u32 = SEG_BASE + 0x09;
/// 指纹不符。
pub const CP_ESEAL: u32 = SEG_BASE + 0x0a;
/// 无观测。
pub const CP_ENOBS: u32 = SEG_BASE + 0x0b;
/// 矩阵不平。
pub const CP_EMATRIX: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const CP_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码（账本格的操作归类：对应七道闸 + 语料 + 重放）
// ---------------------------------------------------------------------------

/// 目的件创建/选项段。
pub const OP_OPEN: u8 = 0x01;
/// stat 与稀疏几何段。
pub const OP_STAT: u8 = 0x02;
/// 复制循环段。
pub const OP_COPY: u8 = 0x03;
/// 写错误归因段。
pub const OP_WERR: u8 = 0x04;
/// close 提交归因段。
pub const OP_CERR: u8 = 0x05;
/// 属性尾段。
pub const OP_PRES: u8 = 0x06;
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

/// 折入一个有符号数（先展成 u64，保持与 fold_word 同构）。
fn fold_i32(h: u64, v: i32) -> u64 {
    fold_word(h, v as i64 as u64)
}

// ---------------------------------------------------------------------------
// 数字/布尔文本
// ---------------------------------------------------------------------------

/// 十进制文本（u32，逐位从后往前填，不递归）。
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

/// u32 文本。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
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

/// bool 文本。
fn bool_text(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

/// 把 `工具名: ` 前缀贴进文案（GNU/busybox 的 argv[0] 都是 `cp`）。
fn cp_prefix() -> String {
    String::from("cp: ")
}

// ---------------------------------------------------------------------------
// 文案（真实 coreutils / busybox 表面措辞逐字复刻）
// ---------------------------------------------------------------------------

/// strerror(errno) 的 glibc 原文（本卡涉及的七个值）。
pub fn strerror(code: i32) -> &'static str {
    match code {
        ERR_ENOSPC => "No space left on device",
        ERR_EACCES => "Permission denied",
        ERR_EPERM => "Operation not permitted",
        ERR_EISDIR => "Is a directory",
        ERR_EDQUOT => "Quota exceeded",
        ERR_EIO => "Input/output error",
        ERR_NONE => "",
        _ => "Unknown error",
    }
}

/// GNU：复制循环写失败——`cp: error writing '<dst>': <strerror>`。
pub fn msg_gnu_error_writing(path: &str, code: i32) -> String {
    let mut s = cp_prefix();
    s.push_str("error writing '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU：关闭目的件才爆——`cp: failed to close '<dst>': <strerror>`。
pub fn msg_gnu_failed_to_close(path: &str, code: i32) -> String {
    let mut s = cp_prefix();
    s.push_str("failed to close '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU：目的件建不出来——`cp: cannot create regular file '<dst>': <strerror>`。
pub fn msg_gnu_cannot_create(path: &str, code: i32) -> String {
    let mut s = cp_prefix();
    s.push_str("cannot create regular file '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU：不带 `-r` 复制到目录——`cp: -r not specified; omitting directory '<src>'`。
pub fn msg_gnu_omitting_directory(path: &str) -> String {
    let mut s = cp_prefix();
    s.push_str("-r not specified; omitting directory '");
    s.push_str(path);
    s.push('\'');
    s
}

/// GNU：源件打不开——`cp: cannot open '<src>' for reading: <strerror>`。
pub fn msg_gnu_cannot_open(path: &str, code: i32) -> String {
    let mut s = cp_prefix();
    s.push_str("cannot open '");
    s.push_str(path);
    s.push_str("' for reading: ");
    s.push_str(strerror(code));
    s
}

/// GNU：stat 不到源——`cp: cannot stat '<src>': <strerror>`。
pub fn msg_gnu_cannot_stat(path: &str, code: i32) -> String {
    let mut s = cp_prefix();
    s.push_str("cannot stat '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU：稀疏行为依赖文件系统能力的一次性告警。
pub fn msg_gnu_sparse_warning(mount: &str) -> String {
    let mut s = cp_prefix();
    s.push_str("warning: behavior of cp is dependent on the file system's sparse support,");
    s.push_str(" but it is not enabled for '");
    s.push_str(mount);
    s.push_str("', file system does not support sparse files");
    s
}

/// GNU：保留时间戳失败（尾段）。
pub fn msg_gnu_preserving_times(path: &str, code: i32) -> String {
    let mut s = cp_prefix();
    s.push_str("preserving times for '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU：保留权限失败（尾段，ACL/SELinux 模式）。
pub fn msg_gnu_preserving_permissions(path: &str, code: i32) -> String {
    let mut s = cp_prefix();
    s.push_str("preserving permissions for '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU：保留属主失败（尾段，非 root）。
pub fn msg_gnu_failed_preserve_ownership(path: &str, code: i32) -> String {
    let mut s = cp_prefix();
    s.push_str("failed to preserve ownership for '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU：扩展属性拷贝失败（尾段，无路径参数，逐字）。
pub fn msg_gnu_failed_copy_xattrs() -> String {
    String::from("cp: failed to copy xattrs")
}

/// GNU：无法保留稀疏洞的告警（目的端不支持稀疏）。
pub fn msg_gnu_cannot_preserve_sparse(path: &str) -> String {
    let mut s = cp_prefix();
    s.push_str("cannot preserve sparse file state for '");
    s.push_str(path);
    s.push('\'');
    s
}

/// GNU：用法/选项硬错——`cp: unrecognized option '<opt>'`。
pub fn msg_gnu_unrecognized_option(opt: &str) -> String {
    let mut s = cp_prefix();
    s.push_str("unrecognized option '");
    s.push_str(opt);
    s.push('\'');
    s
}

/// busybox：目的件建不出来——`cp: can't create '<dst>': <strerror>`。
pub fn msg_bb_cannot_create(path: &str, code: i32) -> String {
    let mut s = cp_prefix();
    s.push_str("can't create '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// busybox：写失败——`cp: write error to '<dst>': <strerror>`。
pub fn msg_bb_write_error(path: &str, code: i32) -> String {
    let mut s = cp_prefix();
    s.push_str("write error to '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// busybox：源件打不开——`cp: can't open '<src>': <strerror>`。
pub fn msg_bb_cannot_open(path: &str, code: i32) -> String {
    let mut s = cp_prefix();
    s.push_str("can't open '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// busybox：stat 不到源——`cp: can't stat '<src>': <strerror>`。
pub fn msg_bb_cannot_stat(path: &str, code: i32) -> String {
    let mut s = cp_prefix();
    s.push_str("can't stat '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// busybox：目录当普通件复制——`cp: '<src>': Is a directory`。
pub fn msg_bb_is_a_directory(path: &str) -> String {
    let mut s = cp_prefix();
    s.push_str("'");
    s.push_str(path);
    s.push_str("': Is a directory");
    s
}

/// busybox：保留属性只做 chmod，失败时 `cp: can't chmod '<dst>': <strerror>`。
pub fn msg_bb_cannot_chmod(path: &str, code: i32) -> String {
    let mut s = cp_prefix();
    s.push_str("can't chmod '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// busybox：不认的选项——`cp: unrecognized option: <opt>`（随后 usage，退 1）。
pub fn msg_bb_unrecognized_option(opt: &str) -> String {
    let mut s = cp_prefix();
    s.push_str("unrecognized option: ");
    s.push_str(opt);
    s
}

/// 本卡自研审计文案：循环里少了字节却没报错（静默截断）。
pub fn msg_silent_truncate(dst: &str, written: u32, want: u32) -> String {
    let mut s = String::from("silent truncation at '");
    s.push_str(dst);
    s.push_str("' (");
    s.push_str(dec_text(written).as_str());
    s.push_str(" of ");
    s.push_str(dec_text(want).as_str());
    s.push_str(" bytes, no error reported)");
    s
}

/// 本卡自研审计文案：close 报错但被 busybox 丢弃。
pub fn msg_close_ignored(dst: &str, lost: u32) -> String {
    let mut s = String::from("close error dropped by busybox for '");
    s.push_str(dst);
    s.push_str("' (");
    s.push_str(dec_text(lost).as_str());
    s.push_str(" bytes of deferred blocks unaccounted)");
    s
}

/// 归因破断的审计文案：文案说中止、字节却说写全了（或反之）。
pub fn msg_abort_broken(note: &str, value: u32, code: u32) -> String {
    let mut s = String::from("abort attribution broken (note=");
    s.push_str(note);
    s.push_str(", bytes=");
    s.push_str(dec_text(value).as_str());
    s.push_str(", exit=");
    s.push_str(dec_text(code).as_str());
    s.push(')');
    s
}

// ---------------------------------------------------------------------------
// 枚举：工具面
// ---------------------------------------------------------------------------

/// 被测 `cp` 是哪一套实现。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// GNU coreutils 的 `cp`。
    Gnu,
    /// busybox 的 `cp`。
    Busybox,
}

impl Tool {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Tool::Gnu => "gnu-cp",
            Tool::Busybox => "busybox-cp",
        }
    }

    /// 用法/选项硬错的退出码：GNU 走 `usage()` 退 2，busybox 退 1。
    pub fn usage_exit(&self) -> u32 {
        match self {
            Tool::Gnu => EXIT_HARD,
            Tool::Busybox => EXIT_FILE,
        }
    }

    /// 是否检查 `close()` 的返回值（GNU 9.x 起检查，busybox 丢弃）。
    ///
    /// 这一条是本卡最硬的工具面差异：它决定延迟分配的 ENOSPC 会不会被记账。
    pub fn reports_close_error(&self) -> bool {
        match self {
            Tool::Gnu => true,
            Tool::Busybox => false,
        }
    }

    /// 是否支持 `-S/--sparse`（busybox 无此选项）。
    pub fn supports_sparse_flag(&self) -> bool {
        match self {
            Tool::Gnu => true,
            Tool::Busybox => false,
        }
    }

    /// 是否支持 xattr/权限上下文的保留。
    pub fn supports_xattr_preserve(&self) -> bool {
        match self {
            Tool::Gnu => true,
            Tool::Busybox => false,
        }
    }

    /// 目录源的诊断落点：GNU 在 stat 段就拒（`omitting directory`），
    /// busybox 要走到读循环才报 `Is a directory`。
    pub fn diagnoses_dir_at_stat(&self) -> bool {
        match self {
            Tool::Gnu => true,
            Tool::Busybox => false,
        }
    }

    /// 尾段失败是否只报 chmod 一种形态（busybox 的 `-p` 仅 chmod + utime）。
    pub fn tail_is_chmod_only(&self) -> bool {
        match self {
            Tool::Gnu => false,
            Tool::Busybox => true
        }
    }

    /// 该工具能要求的最大保留位集合。
    pub fn preserve_ceiling(&self) -> u8 {
        match self {
            Tool::Gnu => PRES_ALL,
            Tool::Busybox => PRES_MODE | PRES_TIME,
        }
    }
}

/// 复制模式（命令行选项折算出来的语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyMode {
    /// `cp a b`：单件，不保留属性。
    Plain,
    /// `cp -r dir dst`：递归，不保留属性。
    Recursive,
    /// `cp -p a b`：保留 mode/mtime。
    Preserve,
    /// `cp -a src dst`：递归 + 全量保留。
    Archive,
    /// `cp -S always a b`：稀疏感知复制（不递归）。
    Sparse,
}

impl CopyMode {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            CopyMode::Plain => "plain",
            CopyMode::Recursive => "recursive",
            CopyMode::Preserve => "preserve",
            CopyMode::Archive => "archive",
            CopyMode::Sparse => "sparse",
        }
    }

    /// 是否递归遍历（`-r`/`-a`）。
    pub fn recursive(&self) -> bool {
        match self {
            CopyMode::Recursive | CopyMode::Archive => true,
            CopyMode::Plain | CopyMode::Preserve | CopyMode::Sparse => false,
        }
    }

    /// 是否走稀疏感知路径。
    pub fn uses_sparse(&self) -> bool {
        match self {
            CopyMode::Sparse | CopyMode::Archive => true,
            CopyMode::Plain | CopyMode::Recursive | CopyMode::Preserve => false,
        }
    }

    /// 该模式默认要求保留的属性位。
    pub fn preserve_bits(&self) -> u8 {
        match self {
            CopyMode::Archive => PRES_ALL,
            CopyMode::Preserve => PRES_P,
            CopyMode::Sparse => PRES_SPARSE,
            CopyMode::Plain | CopyMode::Recursive => PRES_NONE,
        }
    }
}

/// 保留属性位的位序（尾段扫描次序）。
pub const PRES_BITS: [u8; PRES_BIT_COUNT] = [PRES_MODE, PRES_TIME, PRES_XATTR, PRES_SPARSE];

/// 属性位文本。
pub fn pres_name(bit: u8) -> &'static str {
    match bit {
        PRES_MODE => "mode",
        PRES_TIME => "mtime",
        PRES_XATTR => "xattr",
        PRES_SPARSE => "sparse",
        _ => "none",
    }
}

/// 位集合是否为 `cap` 的子集。
pub fn pres_subset(bits: u8, cap: u8) -> bool {
    bits & !cap == 0
}

/// 位集合里置位的个数。
pub fn pres_popcount(bits: u8) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < PRES_BITS.len() {
        if let Some(b) = PRES_BITS.get(i) {
            if bits & *b != 0 {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// 枚举：闸门阶段与形态
// ---------------------------------------------------------------------------

/// 闸门阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 选项与目的件创建。
    Open,
    /// stat 与稀疏几何定案。
    Stat,
    /// 复制循环（read/write/seek）。
    Copy,
    /// 写错误归因。
    Werr,
    /// close 提交归因。
    ClErr,
    /// 属性尾段。
    Pres,
    /// 退出码聚合。
    Exit,
}

impl Stage {
    /// 次序（纵线不可倒）。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Open => 0,
            Stage::Stat => 1,
            Stage::Copy => 2,
            Stage::Werr => 3,
            Stage::ClErr => 4,
            Stage::Pres => 5,
            Stage::Exit => 6,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Open => "open",
            Stage::Stat => "stat",
            Stage::Copy => "copy",
            Stage::Werr => "werr",
            Stage::ClErr => "cerr",
            Stage::Pres => "pres",
            Stage::Exit => "exit",
        }
    }

    /// 是否失败落点（Exit 之外都算某道闸拦下）。
    pub fn failure(&self) -> bool {
        match self {
            Stage::Exit => false,
            _ => true,
        }
    }

    /// 是否数据面闸门（open/copy/werr/cerr 四段影响留档字节）。
    pub fn data_plane(&self) -> bool {
        match self {
            Stage::Open | Stage::Copy | Stage::Werr | Stage::ClErr => true,
            Stage::Stat | Stage::Pres | Stage::Exit => false,
        }
    }

    /// 是否中止族闸门（只有这两段能把 ENOSPC 记成中止）。
    pub fn abort_plane(&self) -> bool {
        match self {
            Stage::Open | Stage::Werr | Stage::ClErr => true,
            Stage::Stat | Stage::Copy | Stage::Pres | Stage::Exit => false,
        }
    }
}

/// 文案落点（现场形态的分类，对应真实诊断）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 全件落定，无任何告警。
    Complete,
    /// 全件落定，但打了稀疏行为告警（退出码仍 0）。
    WarnSparse,
    /// busybox 丢弃了 close 错误：码 0，延迟块无人记账。
    CloseIgnored,
    /// 目的件建不出来（`cannot create regular file` / `can't create`）。
    CreateFail,
    /// 选项在本工具不认（GNU 退 2，busybox 退 1）。
    BadOption,
    /// GNU 在 stat 段拒目录（`-r not specified; omitting directory`）。
    OmitDir,
    /// busybox 在读循环撞目录（`'x': Is a directory`）。
    IsDir,
    /// 循环少了字节却没有报错——静默截断，判死。
    SilentTrunc,
    /// GNU 写错文案（`error writing`）。
    ErrorWriting,
    /// busybox 写错文案（`write error to`）。
    WriteError,
    /// GNU close 才爆（`failed to close`）。
    CloseEnospc,
    /// 尾段：权限位没保住。
    PresMode,
    /// 尾段：时间戳没保住。
    PresTime,
    /// 尾段：扩展属性没保住。
    PresXattr,
    /// 尾段：稀疏几何没保住。
    PresSparse,
    /// 邻居 errno 造成的中止（红，但不是 ENOSPC）。
    ForeignErr,
    /// 归因破断：文案/字节/退出码三者不同族。
    AbortMismatch,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::Complete => "complete",
            Note::WarnSparse => "warn-sparse",
            Note::CloseIgnored => "close-ignored",
            Note::CreateFail => "create-fail",
            Note::BadOption => "bad-option",
            Note::OmitDir => "omit-dir",
            Note::IsDir => "is-dir",
            Note::SilentTrunc => "silent-trunc",
            Note::ErrorWriting => "error-writing",
            Note::WriteError => "write-error",
            Note::CloseEnospc => "failed-to-close",
            Note::PresMode => "pres-mode",
            Note::PresTime => "pres-time",
            Note::PresXattr => "pres-xattr",
            Note::PresSparse => "pres-sparse",
            Note::ForeignErr => "foreign-err",
            Note::AbortMismatch => "abort-mismatch",
        }
    }

    /// 是否工具自认成功（退出码 0 的那一族）。
    pub fn success(&self) -> bool {
        match self {
            Note::Complete | Note::WarnSparse | Note::CloseIgnored => true,
            _ => false,
        }
    }

    /// 是否 ENOSPC 中止族文案（数据面被满盘截断/拒绝）。
    pub fn abort_family(&self) -> bool {
        match self {
            Note::CreateFail | Note::ErrorWriting | Note::WriteError | Note::CloseEnospc => true,
            Note::Complete
            | Note::WarnSparse
            | Note::CloseIgnored
            | Note::BadOption
            | Note::OmitDir
            | Note::IsDir
            | Note::SilentTrunc
            | Note::PresMode
            | Note::PresTime
            | Note::PresXattr
            | Note::PresSparse
            | Note::ForeignErr
            | Note::AbortMismatch => false,
        }
    }

    /// 是否属性尾段族（数据已到位，只有保留动作失败）。
    pub fn tail_family(&self) -> bool {
        match self {
            Note::PresMode | Note::PresTime | Note::PresXattr | Note::PresSparse => true,
            _ => false,
        }
    }

    /// 是否前置诊断族（还没开始搬字节就停）。
    pub fn prelude_family(&self) -> bool {
        match self {
            Note::BadOption | Note::OmitDir | Note::IsDir => true,
            _ => false,
        }
    }

    /// 是否本卡判死的异常形态（既非成功也非合法中止/尾段）。
    pub fn defect_family(&self) -> bool {
        match self {
            Note::SilentTrunc | Note::ForeignErr | Note::AbortMismatch => true,
            _ => false,
        }
    }

    /// 是否稀疏告警族（码 0 但现场有话说）。
    pub fn noisy_success(&self) -> bool {
        match self {
            Note::WarnSparse | Note::CloseIgnored => true,
            _ => false,
        }
    }

    /// 尾段族对应的保留位。
    pub fn preserve_bit(&self) -> u8 {
        match self {
            Note::PresMode => PRES_MODE,
            Note::PresTime => PRES_TIME,
            Note::PresXattr => PRES_XATTR,
            Note::PresSparse => PRES_SPARSE,
            _ => PRES_NONE,
        }
    }
}

/// 结果档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 全件落定且无声（码 0、无告警）。
    Complete,
    /// 码 0 但现场有话（稀疏告警，或 busybox 丢了 close 错误）。
    Warned,
    /// 码 1 且不是 ENOSPC 中止（用法/类型/尾段/邻居）。
    Tail,
    /// ENOSPC 中止（归因命中）。
    Aborted,
}

impl Tier {
    /// 由结果投影。
    pub fn of(o: &Outcome) -> Tier {
        if o.default {
            Tier::Aborted
        } else if o.ok() {
            match o.note {
                Note::Complete => Tier::Complete,
                _ => Tier::Warned,
            }
        } else {
            Tier::Tail
        }
    }

    /// 次序。
    pub fn index(&self) -> usize {
        match self {
            Tier::Complete => 0,
            Tier::Warned => 1,
            Tier::Tail => 2,
            Tier::Aborted => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Complete => "complete",
            Tier::Warned => "warned",
            Tier::Tail => "tail",
            Tier::Aborted => "aborted",
        }
    }

    /// 是否成功档（退出码 0）。
    pub fn success(&self) -> bool {
        match self {
            Tier::Complete | Tier::Warned => true,
            Tier::Tail | Tier::Aborted => false,
        }
    }

    /// 是否中止档。
    pub fn aborted(&self) -> bool {
        match self {
            Tier::Aborted => true,
            _ => false,
        }
    }
}

/// 申报缺陷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 一致。
    Ok,
    /// 阶段错。
    Stage,
    /// 退出码错。
    Code,
    /// 留档字节错。
    Value,
    /// 文案落点错。
    Note,
    /// 归因错。
    Default,
    /// 归因三元组破断（结构维）。
    Abort,
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
            Fault::Stage => CP_ESTAGE,
            Fault::Code => CP_ECODE,
            Fault::Value => CP_EVALUE,
            Fault::Note => CP_ENOTE,
            Fault::Default => CP_EDEFAULT,
            Fault::Abort => CP_EABORT,
            Fault::Estate => CP_ESTATE,
            Fault::Count => CP_ECOUNT,
            Fault::Slot => CP_ESLOT,
            Fault::Seal => CP_ESEAL,
            Fault::Nobs => CP_ENOBS,
            Fault::Matrix => CP_EMATRIX,
            Fault::Bal => CP_EBAL,
        }
    }

    /// 是否非零。
    pub fn alive(&self) -> bool {
        !matches!(self, Fault::Ok)
    }
}

/// 缺陷短名。
pub fn fault_short(f: Fault) -> &'static str {
    match f {
        Fault::Ok => "ok",
        Fault::Stage => "stage",
        Fault::Code => "code",
        Fault::Value => "value",
        Fault::Note => "note",
        Fault::Default => "default",
        Fault::Abort => "abort",
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
// 目的端文件系统画像
// ---------------------------------------------------------------------------

/// 目的端挂载点的画像（本卡结构闸的载体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FsProfile {
    /// 挂载点名。
    pub mount: &'static str,
    /// 块大小（字节）。
    pub bsize: u32,
    /// 空闲块数。
    pub blocks_free: u32,
    /// 是否延迟分配（close 才爆的根源）。
    pub delalloc: bool,
    /// 是否支持稀疏文件。
    pub sparse_ok: bool,
    /// 是否支持扩展属性。
    pub xattr_ok: bool,
}

impl FsProfile {
    /// 空白画像。
    pub const fn blank() -> FsProfile {
        FsProfile {
            mount: "",
            bsize: 0,
            blocks_free: 0,
            delalloc: false,
            sparse_ok: false,
            xattr_ok: false,
        }
    }

    /// 空闲字节（乘法溢出视为坏画像）。
    pub fn free_bytes(&self) -> Option<u32> {
        self.bsize.checked_mul(self.blocks_free)
    }

    /// 是否结构良好。
    pub fn well_formed(&self) -> bool {
        if self.mount.is_empty() {
            return false;
        }
        if self.bsize != 512 && self.bsize != 1024 && self.bsize != 4096 {
            return false;
        }
        if self.free_bytes().is_none() {
            return false;
        }
        // 满盘画像允许零块，但不允许"负"的块数表示（上界钳制）。
        self.blocks_free <= 0x0800_0000
    }

    /// 是否已经满到零块。
    pub fn is_full(&self) -> bool {
        self.blocks_free == 0
    }

    /// 该画像上 `-S` 能否真正跳过洞。
    pub fn can_skip_holes(&self) -> bool {
        self.sparse_ok
    }

    /// 该画像上 close 是否可能迟到报错。
    pub fn close_can_fail(&self) -> bool {
        self.delalloc
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.mount);
        h = fold_word(h, self.bsize as u64);
        h = fold_word(h, self.blocks_free as u64);
        h = fold_word(h, self.delalloc as u64);
        h = fold_word(h, self.sparse_ok as u64);
        match self.free_bytes() {
            Some(v) => fold_word(fold_word(h, self.xattr_ok as u64), v as u64),
            None => fold_word(fold_word(h, self.xattr_ok as u64), 0),
        }
    }
}

/// 三块被测挂载点：满盘延迟分配、满盘即时、半满不支持稀疏。
pub fn profiles() -> [FsProfile; 3] {
    [
        FsProfile {
            mount: "/mnt/full",
            bsize: 4096,
            blocks_free: 0,
            delalloc: true,
            sparse_ok: true,
            xattr_ok: true,
        },
        FsProfile {
            mount: "/mnt/loop",
            bsize: 1024,
            blocks_free: 0,
            delalloc: false,
            sparse_ok: false,
            xattr_ok: true,
        },
        FsProfile {
            mount: "/mnt/tmpfs",
            bsize: 512,
            blocks_free: 8,
            delalloc: false,
            sparse_ok: false,
            xattr_ok: false,
        },
    ]
}

/// 稀疏记账：表观尺寸、洞字节、能否跳过。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Holes {
    /// 表观字节数（`st_size`）。
    pub apparent: u32,
    /// 洞字节数。
    pub hole: u32,
    /// 目的端能否用 seek 跳过洞。
    pub skip_ok: bool,
}

impl Holes {
    /// 空白记账。
    pub const fn blank() -> Holes {
        Holes { apparent: 0, hole: 0, skip_ok: false }
    }

    /// 真正要写进设备的字节（跳过洞时洞不计入）。
    pub fn spread(&self) -> u32 {
        if self.skip_ok {
            self.apparent.saturating_sub(self.hole)
        } else {
            self.apparent
        }
    }

    /// 洞是否被展开成真实零块（满盘时这部分要占额度）。
    pub fn holes_spread(&self) -> u32 {
        if self.skip_ok {
            0
        } else {
            self.hole
        }
    }

    /// 洞不得大于表观尺寸。
    pub fn balanced(&self) -> bool {
        self.hole <= self.apparent
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.apparent as u64);
        h = fold_word(h, self.hole as u64);
        fold_word(h, self.skip_ok as u64)
    }
}

// ---------------------------------------------------------------------------
// 形状（本卡输入的全部维度）
// ---------------------------------------------------------------------------

/// 一次 `cp` 运行的形状：工具 + 模式 + 字节 + 目的端能力 + 各段结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// 源路径。
    pub src: &'static str,
    /// 目的路径。
    pub dst: &'static str,
    /// 哪一套 `cp`。
    pub tool: Tool,
    /// 复制模式。
    pub mode: CopyMode,
    /// 源件表观字节数。
    pub size: u32,
    /// 已落进目的件的字节数（中止时就是留档尺寸）。
    pub written: u32,
    /// 稀疏洞字节数。
    pub holes: u32,
    /// 目的文件系统是否支持稀疏。
    pub fs_sparse: bool,
    /// 目的文件系统是否延迟分配。
    pub delalloc: bool,
    /// 源是否为目录。
    pub src_dir: bool,
    /// 选项是否被本工具拒绝。
    pub bad_option: bool,
    /// 目的件是否建成。
    pub create_ok: bool,
    /// 复制循环是否收到写错误。
    pub write_err: bool,
    /// close 是否报错。
    pub close_err: bool,
    /// 现场 errno（从 F11681 引进的结论，本卡不重判）。
    pub errno_seen: i32,
    /// 本次要求保留的属性位。
    pub preserved: u8,
    /// 实际失败的属性位。
    pub pres_failed: u8,
    /// 是否处在 `-r`/`-a` 遍历中。
    pub traversal: bool,
}

impl Shape {
    /// 默认形状：GNU plain 建件成功、无错误、无保留。
    pub const fn blank() -> Shape {
        Shape {
            tag: "",
            src: "a.bin",
            dst: "b.bin",
            tool: Tool::Gnu,
            mode: CopyMode::Plain,
            size: 0,
            written: 0,
            holes: 0,
            fs_sparse: true,
            delalloc: false,
            src_dir: false,
            bad_option: false,
            create_ok: true,
            write_err: false,
            close_err: false,
            errno_seen: ERR_NONE,
            preserved: PRES_NONE,
            pres_failed: PRES_NONE,
            traversal: false,
        }
    }

    /// 换"已落字节"的副本（谓词把同一形状推到不同档位复算）。
    pub const fn clone_written(&self, w: u32) -> Shape {
        Shape {
            tag: self.tag,
            src: self.src,
            dst: self.dst,
            tool: self.tool,
            mode: self.mode,
            size: self.size,
            written: w,
            holes: self.holes,
            fs_sparse: self.fs_sparse,
            delalloc: self.delalloc,
            src_dir: self.src_dir,
            bad_option: self.bad_option,
            create_ok: self.create_ok,
            write_err: self.write_err,
            close_err: self.close_err,
            errno_seen: self.errno_seen,
            preserved: self.preserved,
            pres_failed: self.pres_failed,
            traversal: self.traversal,
        }
    }

    /// 换"现场 errno"的副本（把同一现场推到邻居档）。
    pub const fn clone_errno(&self, e: i32) -> Shape {
        Shape {
            tag: self.tag,
            src: self.src,
            dst: self.dst,
            tool: self.tool,
            mode: self.mode,
            size: self.size,
            written: self.written,
            holes: self.holes,
            fs_sparse: self.fs_sparse,
            delalloc: self.delalloc,
            src_dir: self.src_dir,
            bad_option: self.bad_option,
            create_ok: self.create_ok,
            write_err: self.write_err,
            close_err: self.close_err,
            errno_seen: e,
            preserved: self.preserved,
            pres_failed: self.pres_failed,
            traversal: self.traversal,
        }
    }

    /// 换"工具面"的副本（GNU/busybox 对照复算）。
    pub const fn clone_tool(&self, t: Tool) -> Shape {
        Shape {
            tag: self.tag,
            src: self.src,
            dst: self.dst,
            tool: t,
            mode: self.mode,
            size: self.size,
            written: self.written,
            holes: self.holes,
            fs_sparse: self.fs_sparse,
            delalloc: self.delalloc,
            src_dir: self.src_dir,
            bad_option: self.bad_option,
            create_ok: self.create_ok,
            write_err: self.write_err,
            close_err: self.close_err,
            errno_seen: self.errno_seen,
            preserved: self.preserved,
            pres_failed: self.pres_failed,
            traversal: self.traversal,
        }
    }

    /// 真正需要写进设备的字节：洞能跳过时不计入。
    pub fn payload(&self) -> u32 {
        if self.mode.uses_sparse() && self.fs_sparse {
            self.size.saturating_sub(self.holes)
        } else {
            self.size
        }
    }

    /// 稀疏记账视图。
    pub fn geometry(&self) -> Holes {
        Holes {
            apparent: self.size,
            hole: self.holes,
            skip_ok: self.mode.uses_sparse() && self.fs_sparse,
        }
    }

    /// 是否会打稀疏行为告警（只有 GNU 有这条文案）。
    pub fn sparse_warning(&self) -> bool {
        self.tool.supports_sparse_flag() && self.mode.uses_sparse() && self.holes > 0
            && !self.fs_sparse
    }

    /// busybox 丢弃 close 错误时未记账的延迟块。
    pub fn deferred_lost(&self) -> u32 {
        if self.close_err && !self.tool.reports_close_error() && self.delalloc {
            self.written
        } else {
            0
        }
    }

    /// 用法错（未识别选项）在这件工具上的退出码。
    /// GNU 的选项解析失败走 usage()，退 2；busybox 不区分用法错与文件错，统一退 1。
    /// 这个值只由工具决定，与现场字节/errno 无关，所以单独抽出来当判据。
    pub fn code_of_usage(&self) -> u32 {
        self.tool.usage_exit()
    }

    /// 现场 errno 是否在合法集合内。
    pub fn errno_legal(&self) -> bool {
        self.errno_seen == ERR_NONE
            || self.errno_seen == ERR_ENOSPC
            || self.errno_seen == ERR_EACCES
            || self.errno_seen == ERR_EPERM
            || self.errno_seen == ERR_EISDIR
            || self.errno_seen == ERR_EDQUOT
            || self.errno_seen == ERR_EIO
    }

    /// 形状是否合法可判（各维必须自洽，不自洽的形状不进判据）。
    pub fn valid(&self) -> bool {
        if !self.errno_legal() {
            return false;
        }
        if self.size > SIZE_MAX || self.written > SIZE_MAX || self.holes > SIZE_MAX {
            return false;
        }
        if self.holes > self.size {
            return false;
        }
        if self.written > self.payload() {
            return false;
        }
        if !pres_subset(self.preserved, self.mode.preserve_bits()) {
            return false;
        }
        if !pres_subset(self.preserved, self.tool.preserve_ceiling()) {
            return false;
        }
        if !pres_subset(self.pres_failed, self.preserved) {
            return false;
        }
        if self.mode == CopyMode::Sparse && !self.tool.supports_sparse_flag() {
            return false;
        }
        if self.traversal && !self.mode.recursive() {
            return false;
        }
        if self.bad_option && (self.written != 0 || self.write_err || self.close_err) {
            return false;
        }
        if self.bad_option && self.pres_failed != PRES_NONE {
            return false;
        }
        if !self.create_ok {
            if self.written != 0 || self.errno_seen == ERR_NONE {
                return false;
            }
            if self.write_err || self.close_err {
                return false;
            }
        }
        if self.write_err && (self.close_err || self.errno_seen == ERR_NONE) {
            return false;
        }
        if self.close_err && self.errno_seen == ERR_NONE {
            return false;
        }
        if self.pres_failed != PRES_NONE && self.errno_seen == ERR_NONE {
            return false;
        }
        true
    }

    /// 指纹：对全部判定字段敏感。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_str(h, self.src);
        h = fold_str(h, self.dst);
        h = fold_str(h, self.tool.text());
        h = fold_str(h, self.mode.name());
        h = fold_word(h, self.size as u64);
        h = fold_word(h, self.written as u64);
        h = fold_word(h, self.holes as u64);
        h = fold_word(h, self.fs_sparse as u64);
        h = fold_word(h, self.delalloc as u64);
        h = fold_word(h, self.src_dir as u64);
        h = fold_word(h, self.bad_option as u64);
        h = fold_word(h, self.create_ok as u64);
        h = fold_word(h, self.write_err as u64);
        h = fold_word(h, self.close_err as u64);
        h = fold_i32(h, self.errno_seen);
        h = fold_word(h, self.preserved as u64);
        h = fold_word(h, self.pres_failed as u64);
        fold_word(h, self.traversal as u64)
    }
}

// ---------------------------------------------------------------------------
// 结果
// ---------------------------------------------------------------------------

/// 一次判定的结果（六维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 结束阶段（最后拦下这次运行的闸）。
    pub stage: Stage,
    /// 退出码（0/1/2）。
    pub code: u32,
    /// 目的件留档字节。
    pub value: u32,
    /// 文案落点。
    pub note: Note,
    /// 归因位：是否 ENOSPC 中止。
    pub default: bool,
}

impl Outcome {
    /// 空白结果。
    pub const fn blank() -> Outcome {
        Outcome { stage: Stage::Exit, code: EXIT_OK, value: 0, note: Note::Complete, default: false }
    }

    /// 是否成功（码 0 且文案属成功族）。
    pub fn ok(&self) -> bool {
        self.code == EXIT_OK && self.note.success()
    }

    /// 是否失败。
    pub fn failed(&self) -> bool {
        !self.ok()
    }

    /// 是否归因命中：ENOSPC 中止且文案与退出码同族。
    pub fn attributed(&self) -> bool {
        self.default && self.note.abort_family() && self.code == EXIT_FILE && self.failed()
    }

    /// 三元组自洽：文案说的中止必须与留档字节、退出码同族。
    ///
    /// 写错文案要求"少字节"（半件留档），close 错文案要求"字节全在"（延迟块
    /// 才可能丢），建件错要求"零字节"。这三条各自只在自己的闸里有意义。
    pub fn abort_consistent(&self, s: &Shape) -> bool {
        if !self.default {
            return true;
        }
        if self.code != EXIT_FILE {
            return false;
        }
        match self.note {
            Note::ErrorWriting | Note::WriteError => self.value < s.payload(),
            Note::CloseEnospc => self.value == s.payload(),
            Note::CreateFail => self.value == 0,
            _ => false,
        }
    }

    /// 留档字节相对源尺寸的亏空（审计用，饱和减）。
    pub fn shortfall(&self, s: &Shape) -> u32 {
        s.payload().saturating_sub(self.value)
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.order() as u64);
        h = fold_word(h, self.code as u64);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        fold_word(h, self.default as u64)
    }
}

// ---------------------------------------------------------------------------
// 定义纵线（闸门次序：open → stat → copy → werr → cerr → pres → exit）
// ---------------------------------------------------------------------------

/// 判定一次 `cp` 运行在满盘现场的收尾结论。
///
/// 每一闸只在上一闸放行后才执行；任何一段拦下就立刻定案，后面的段不再有权
/// 改写它——这是"文案落点"能被反查的唯一前提。busybox 丢弃 close 错误是这条
/// 纵线上唯一的"红被吞掉"路径：它在 cerr 闸被识别，但落点记在 exit 闸，
/// 归因位为假，因为工具自己认为成了。
pub fn define(s: &Shape) -> Outcome {
    // ---- open：选项与目的件创建 ----
    if s.bad_option {
        return Outcome {
            stage: Stage::Open,
            code: s.tool.usage_exit(),
            value: 0,
            note: Note::BadOption,
            default: false,
        };
    }
    if !s.create_ok {
        // 目的件没建成，留档字节恒为 0，这条信息只有本闸能给出。
        if s.errno_seen == ERR_ENOSPC {
            return Outcome {
                stage: Stage::Open,
                code: EXIT_FILE,
                value: 0,
                note: Note::CreateFail,
                default: true,
            };
        }
        return Outcome {
            stage: Stage::Open,
            code: EXIT_FILE,
            value: 0,
            note: Note::ForeignErr,
            default: false,
        };
    }

    // ---- stat：源类型与稀疏几何定案 ----
    let dir_case = s.src_dir && !s.mode.recursive();
    if dir_case && s.tool.diagnoses_dir_at_stat() {
        return Outcome {
            stage: Stage::Stat,
            code: EXIT_FILE,
            value: 0,
            note: Note::OmitDir,
            default: false,
        };
    }

    // ---- copy：循环本身 ----
    if dir_case {
        // busybox 到读循环才报 Is a directory，落点因此比 GNU 晚两道闸。
        return Outcome {
            stage: Stage::Copy,
            code: EXIT_FILE,
            value: 0,
            note: Note::IsDir,
            default: false,
        };
    }
    if !s.write_err && !s.close_err && s.written < s.payload() {
        // 少了字节却没有错误码——静默截断，满盘"继续往下走"的最危险形态。
        return Outcome {
            stage: Stage::Copy,
            code: EXIT_FILE,
            value: s.written,
            note: Note::SilentTrunc,
            default: false,
        };
    }

    // ---- werr：写错误归因 ----
    if s.write_err {
        if s.errno_seen == ERR_ENOSPC {
            let note = match s.tool {
                Tool::Gnu => Note::ErrorWriting,
                Tool::Busybox => Note::WriteError,
            };
            return Outcome {
                stage: Stage::Werr,
                code: EXIT_FILE,
                value: s.written,
                note,
                default: true,
            };
        }
        return Outcome {
            stage: Stage::Werr,
            code: EXIT_FILE,
            value: s.written,
            note: Note::ForeignErr,
            default: false,
        };
    }

    // ---- cerr：close 提交归因 ----
    let mut dropped_close = false;
    if s.close_err {
        if s.tool.reports_close_error() {
            if s.errno_seen == ERR_ENOSPC {
                return Outcome {
                    stage: Stage::ClErr,
                    code: EXIT_FILE,
                    value: s.written,
                    note: Note::CloseEnospc,
                    default: true,
                };
            }
            return Outcome {
                stage: Stage::ClErr,
                code: EXIT_FILE,
                value: s.written,
                note: Note::ForeignErr,
                default: false,
            };
        }
        // busybox 丢弃：工具自认成功，但要记住这里吞过一次红。
        dropped_close = true;
    }

    // ---- pres：属性尾段 ----
    let mut i = 0usize;
    while i < PRES_BITS.len() && s.pres_failed != PRES_NONE {
        let bit = match PRES_BITS.get(i) {
            Some(v) => *v,
            None => return Outcome::blank(),
        };
        if s.pres_failed & bit != 0 {
            let note = match bit {
                PRES_MODE => Note::PresMode,
                PRES_TIME => Note::PresTime,
                PRES_XATTR => Note::PresXattr,
                _ => Note::PresSparse,
            };
            // 尾段失败不改留档字节：数据已经到位。
            return Outcome {
                stage: Stage::Pres,
                code: EXIT_FILE,
                value: s.written,
                note,
                default: false,
            };
        }
        i += 1;
    }

    // ---- exit：退出码聚合 ----
    if dropped_close {
        return Outcome {
            stage: Stage::Exit,
            code: EXIT_OK,
            value: s.written,
            note: Note::CloseIgnored,
            default: false,
        };
    }
    if s.sparse_warning() {
        return Outcome {
            stage: Stage::Exit,
            code: EXIT_OK,
            value: s.written,
            note: Note::WarnSparse,
            default: false,
        };
    }
    Outcome { stage: Stage::Exit, code: EXIT_OK, value: s.written, note: Note::Complete, default: false }
}

/// 现场文案（note → 真实文本；成功且无声的注解返回空串）。
pub fn message(s: &Shape, o: &Outcome) -> String {
    match o.note {
        Note::Complete => String::new(),
        Note::WarnSparse => msg_gnu_sparse_warning(s.dst),
        Note::CloseIgnored => msg_close_ignored(s.dst, s.deferred_lost()),
        Note::CreateFail => match s.tool {
            Tool::Gnu => msg_gnu_cannot_create(s.dst, s.errno_seen),
            Tool::Busybox => msg_bb_cannot_create(s.dst, s.errno_seen),
        },
        Note::BadOption => match s.tool {
            Tool::Gnu => msg_gnu_unrecognized_option("--preserve=xattr"),
            Tool::Busybox => msg_bb_unrecognized_option("--preserve=xattr"),
        },
        Note::OmitDir => msg_gnu_omitting_directory(s.src),
        Note::IsDir => msg_bb_is_a_directory(s.src),
        Note::SilentTrunc => msg_silent_truncate(s.dst, o.value, s.payload()),
        Note::ErrorWriting => msg_gnu_error_writing(s.dst, s.errno_seen),
        Note::WriteError => msg_bb_write_error(s.dst, s.errno_seen),
        Note::CloseEnospc => msg_gnu_failed_to_close(s.dst, s.errno_seen),
        Note::PresMode => match s.tool {
            Tool::Gnu => msg_gnu_preserving_permissions(s.dst, s.errno_seen),
            Tool::Busybox => msg_bb_cannot_chmod(s.dst, s.errno_seen),
        },
        Note::PresTime => msg_gnu_preserving_times(s.dst, s.errno_seen),
        Note::PresXattr => msg_gnu_failed_copy_xattrs(),
        Note::PresSparse => msg_gnu_cannot_preserve_sparse(s.dst),
        Note::ForeignErr => match (o.stage, s.tool) {
            // 邻居 errno 的落点文案跟着闸走：close 闸报的是 failed to close，
            // open 闸报的是建不出目的件，其余仍是写循环那句。
            (Stage::ClErr, _) => msg_gnu_failed_to_close(s.dst, s.errno_seen),
            (Stage::Open, Tool::Busybox) => msg_bb_cannot_create(s.dst, s.errno_seen),
            (Stage::Open, Tool::Gnu) => msg_gnu_cannot_create(s.dst, s.errno_seen),
            _ => msg_foreign_stop(s.dst, s.errno_seen),
        },
        Note::AbortMismatch => msg_abort_broken(o.note.name(), o.value, o.code),
    }
}

/// 邻居 errno 中止的通用文案（红，但不是 ENOSPC）。
pub fn msg_foreign_stop(path: &str, code: i32) -> String {
    let mut s = cp_prefix();
    s.push_str("error writing '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// 告警文案（与错误文案分开：`message()` 只给结论，`warning()` 给告警行）。
pub fn warning(s: &Shape, o: &Outcome) -> String {
    match o.note {
        Note::WarnSparse => msg_gnu_sparse_warning(s.dst),
        Note::CloseIgnored => msg_close_ignored(s.dst, s.deferred_lost()),
        _ => String::new(),
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
    /// 退出码。
    pub code: u32,
    /// 留档字节。
    pub value: u32,
    /// 文案落点。
    pub note: Note,
    /// 归因位。
    pub default: bool,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim { stage: Stage::Exit, code: EXIT_OK, value: 0, note: Note::Complete, default: false }
    }

    /// 是否申报成功（码 0 且注解属成功族）。
    pub fn success(&self) -> bool {
        self.code == EXIT_OK && self.note.success()
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

/// 8 个可正确申报的形状 + 6 个复刻形状（复刻自 gnu-r-mid-copy-enospc）。
pub const SHAPES: [Shape; CORPUS] = [
    Shape {
        tag: "gnu-plain-complete",
        src: "report.pdf",
        dst: "mnt/report.pdf",
        tool: Tool::Gnu,
        mode: CopyMode::Plain,
        size: 8192,
        written: 8192,
        holes: 0,
        fs_sparse: true,
        delalloc: false,
        src_dir: false,
        bad_option: false,
        create_ok: true,
        write_err: false,
        close_err: false,
        errno_seen: ERR_NONE,
        preserved: PRES_NONE,
        pres_failed: PRES_NONE,
        traversal: false,
    },
    Shape {
        tag: "gnu-sparse-warn",
        src: "vm.img",
        dst: "mnt/vm.img",
        tool: Tool::Gnu,
        mode: CopyMode::Sparse,
        size: 8192,
        written: 8192,
        holes: 4096,
        fs_sparse: false,
        delalloc: false,
        src_dir: false,
        bad_option: false,
        create_ok: true,
        write_err: false,
        close_err: false,
        errno_seen: ERR_NONE,
        preserved: PRES_SPARSE,
        pres_failed: PRES_NONE,
        traversal: false,
    },
    Shape {
        tag: "gnu-omit-dir",
        src: "logs",
        dst: "mnt/logs",
        tool: Tool::Gnu,
        mode: CopyMode::Plain,
        size: 0,
        written: 0,
        holes: 0,
        fs_sparse: true,
        delalloc: false,
        src_dir: true,
        bad_option: false,
        create_ok: true,
        write_err: false,
        close_err: false,
        errno_seen: ERR_NONE,
        preserved: PRES_NONE,
        pres_failed: PRES_NONE,
        traversal: false,
    },
    Shape {
        tag: "gnu-r-mid-copy-enospc",
        src: "logs/day.bin",
        dst: "mnt/logs/day.bin",
        tool: Tool::Gnu,
        mode: CopyMode::Recursive,
        size: 8192,
        written: 4096,
        holes: 0,
        fs_sparse: true,
        delalloc: false,
        src_dir: false,
        bad_option: false,
        create_ok: true,
        write_err: true,
        close_err: false,
        errno_seen: ERR_ENOSPC,
        preserved: PRES_NONE,
        pres_failed: PRES_NONE,
        traversal: true,
    },
    Shape {
        tag: "gnu-close-enospc",
        src: "db.dump",
        dst: "mnt/db.dump",
        tool: Tool::Gnu,
        mode: CopyMode::Plain,
        size: 8192,
        written: 8192,
        holes: 0,
        fs_sparse: true,
        delalloc: true,
        src_dir: false,
        bad_option: false,
        create_ok: true,
        write_err: false,
        close_err: true,
        errno_seen: ERR_ENOSPC,
        preserved: PRES_NONE,
        pres_failed: PRES_NONE,
        traversal: false,
    },
    Shape {
        tag: "gnu-preserving-times",
        src: "cfg.conf",
        dst: "mnt/cfg.conf",
        tool: Tool::Gnu,
        mode: CopyMode::Archive,
        size: 2048,
        written: 2048,
        holes: 0,
        fs_sparse: true,
        delalloc: false,
        src_dir: false,
        bad_option: false,
        create_ok: true,
        write_err: false,
        close_err: false,
        errno_seen: ERR_EPERM,
        preserved: PRES_ALL,
        pres_failed: PRES_TIME,
        traversal: true,
    },
    Shape {
        tag: "bb-create-enospc",
        src: "seed.bin",
        dst: "mnt/seed.bin",
        tool: Tool::Busybox,
        mode: CopyMode::Plain,
        size: 8192,
        written: 0,
        holes: 0,
        fs_sparse: true,
        delalloc: false,
        src_dir: false,
        bad_option: false,
        create_ok: false,
        write_err: false,
        close_err: false,
        errno_seen: ERR_ENOSPC,
        preserved: PRES_NONE,
        pres_failed: PRES_NONE,
        traversal: false,
    },
    Shape {
        tag: "bb-close-ignored",
        src: "notes.txt",
        dst: "mnt/notes.txt",
        tool: Tool::Busybox,
        mode: CopyMode::Preserve,
        size: 4096,
        written: 4096,
        holes: 0,
        fs_sparse: true,
        delalloc: true,
        src_dir: false,
        bad_option: false,
        create_ok: true,
        write_err: false,
        close_err: true,
        errno_seen: ERR_ENOSPC,
        preserved: PRES_P,
        pres_failed: PRES_NONE,
        traversal: false,
    },
    Shape {
        tag: "estate-swap",
        src: "logs/day.bin",
        dst: "mnt/logs/day.bin",
        tool: Tool::Gnu,
        mode: CopyMode::Recursive,
        size: 8192,
        written: 4096,
        holes: 0,
        fs_sparse: true,
        delalloc: false,
        src_dir: false,
        bad_option: false,
        create_ok: true,
        write_err: true,
        close_err: false,
        errno_seen: ERR_ENOSPC,
        preserved: PRES_NONE,
        pres_failed: PRES_NONE,
        traversal: true,
    },
    Shape {
        tag: "stage-swap",
        src: "logs/day.bin",
        dst: "mnt/logs/day.bin",
        tool: Tool::Gnu,
        mode: CopyMode::Recursive,
        size: 8192,
        written: 4096,
        holes: 0,
        fs_sparse: true,
        delalloc: false,
        src_dir: false,
        bad_option: false,
        create_ok: true,
        write_err: true,
        close_err: false,
        errno_seen: ERR_ENOSPC,
        preserved: PRES_NONE,
        pres_failed: PRES_NONE,
        traversal: true,
    },
    Shape {
        tag: "code-swap",
        src: "logs/day.bin",
        dst: "mnt/logs/day.bin",
        tool: Tool::Gnu,
        mode: CopyMode::Recursive,
        size: 8192,
        written: 4096,
        holes: 0,
        fs_sparse: true,
        delalloc: false,
        src_dir: false,
        bad_option: false,
        create_ok: true,
        write_err: true,
        close_err: false,
        errno_seen: ERR_ENOSPC,
        preserved: PRES_NONE,
        pres_failed: PRES_NONE,
        traversal: true,
    },
    Shape {
        tag: "value-swap",
        src: "logs/day.bin",
        dst: "mnt/logs/day.bin",
        tool: Tool::Gnu,
        mode: CopyMode::Recursive,
        size: 8192,
        written: 4096,
        holes: 0,
        fs_sparse: true,
        delalloc: false,
        src_dir: false,
        bad_option: false,
        create_ok: true,
        write_err: true,
        close_err: false,
        errno_seen: ERR_ENOSPC,
        preserved: PRES_NONE,
        pres_failed: PRES_NONE,
        traversal: true,
    },
    Shape {
        tag: "note-swap",
        src: "logs/day.bin",
        dst: "mnt/logs/day.bin",
        tool: Tool::Gnu,
        mode: CopyMode::Recursive,
        size: 8192,
        written: 4096,
        holes: 0,
        fs_sparse: true,
        delalloc: false,
        src_dir: false,
        bad_option: false,
        create_ok: true,
        write_err: true,
        close_err: false,
        errno_seen: ERR_ENOSPC,
        preserved: PRES_NONE,
        pres_failed: PRES_NONE,
        traversal: true,
    },
    Shape {
        tag: "default-swap",
        src: "logs/day.bin",
        dst: "mnt/logs/day.bin",
        tool: Tool::Gnu,
        mode: CopyMode::Recursive,
        size: 8192,
        written: 4096,
        holes: 0,
        fs_sparse: true,
        delalloc: false,
        src_dir: false,
        bad_option: false,
        create_ok: true,
        write_err: true,
        close_err: false,
        errno_seen: ERR_ENOSPC,
        preserved: PRES_NONE,
        pres_failed: PRES_NONE,
        traversal: true,
    },
];

/// 与 `SHAPES` 一一对应的申报。
///
/// 前 8 槽逐维等于现场；后 6 槽各只错一维，错的那维依次是
/// estate/stage/code/value/note/default。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim { stage: Stage::Exit, code: 0, value: 8192, note: Note::Complete, default: false },
    Claim { stage: Stage::Exit, code: 0, value: 8192, note: Note::WarnSparse, default: false },
    Claim { stage: Stage::Stat, code: 1, value: 0, note: Note::OmitDir, default: false },
    Claim { stage: Stage::Werr, code: 1, value: 4096, note: Note::ErrorWriting, default: true },
    Claim { stage: Stage::ClErr, code: 1, value: 8192, note: Note::CloseEnospc, default: true },
    Claim { stage: Stage::Pres, code: 1, value: 2048, note: Note::PresTime, default: false },
    Claim { stage: Stage::Open, code: 1, value: 0, note: Note::CreateFail, default: true },
    Claim { stage: Stage::Exit, code: 0, value: 4096, note: Note::CloseIgnored, default: false },
    Claim { stage: Stage::Exit, code: 0, value: 8192, note: Note::Complete, default: false },
    Claim { stage: Stage::ClErr, code: 1, value: 4096, note: Note::ErrorWriting, default: true },
    Claim { stage: Stage::Werr, code: 0, value: 4096, note: Note::ErrorWriting, default: true },
    Claim { stage: Stage::Werr, code: 1, value: 0, note: Note::ErrorWriting, default: true },
    Claim { stage: Stage::Werr, code: 1, value: 4096, note: Note::CloseEnospc, default: true },
    Claim { stage: Stage::Werr, code: 1, value: 4096, note: Note::ErrorWriting, default: false },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Stage, Fault::Code, Fault::Value, Fault::Note, Fault::Default];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] = [
    CP_ESTATE,
    CP_ESTAGE,
    CP_ECODE,
    CP_EVALUE,
    CP_ENOTE,
    CP_EDEFAULT,
];

/// 账本 8 个成功格的值（总数、成、败、完成档、告警档、尾段档、中止档、红档 ppm）。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 1, 2, 2, 3, 929];

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
        return Err(CP_ENOBS);
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

/// 对判：状态 → 阶段 → 退出码 → 留档字节 → 文案落点 → 归因位。
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
    let mut s = String::from(c.shape.dst);
    s.push_str(" tool=");
    s.push_str(c.shape.tool.text());
    s.push_str(" stage=");
    s.push_str(o.stage.name());
    s.push_str(" exit=");
    s.push_str(dec_text(o.code).as_str());
    s.push_str(" bytes=");
    s.push_str(dec_text(o.value).as_str());
    s.push_str(" abort=");
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
/// "我声明这次 cp 结束在哪一闸"，期望表说的是"校验器按纵线实际会走到哪一闸"。
/// 两者一致才算数，任一处笔误都会在这里露出来。
const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Exit,
    Stage::Exit,
    Stage::Stat,
    Stage::Werr,
    Stage::ClErr,
    Stage::Pres,
    Stage::Open,
    Stage::Exit,
    Stage::Werr,
    Stage::Werr,
    Stage::Werr,
    Stage::Werr,
    Stage::Werr,
    Stage::Werr,
];

/// 14 槽的期望退出码。
const WANT_CODE: [u32; CORPUS] = [0, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1];

/// 14 槽的期望留档字节。
const WANT_VALUE: [u32; CORPUS] = [
    8192, 8192, 0, 4096, 8192, 2048, 0, 4096, 4096, 4096, 4096, 4096, 4096, 4096,
];

/// 14 槽的期望文案落点。
const WANT_NOTE: [Note; CORPUS] = [
    Note::Complete,
    Note::WarnSparse,
    Note::OmitDir,
    Note::ErrorWriting,
    Note::CloseEnospc,
    Note::PresTime,
    Note::CreateFail,
    Note::CloseIgnored,
    Note::ErrorWriting,
    Note::ErrorWriting,
    Note::ErrorWriting,
    Note::ErrorWriting,
    Note::ErrorWriting,
    Note::ErrorWriting,
];

/// 14 槽的期望归因位。
const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, true, true, false, true, false, true, true, true, true, true, true,
];

/// 14 槽标签。
const WANT_TAGS: [&str; CORPUS] = [
    "gnu-plain-complete",
    "gnu-sparse-warn",
    "gnu-omit-dir",
    "gnu-r-mid-copy-enospc",
    "gnu-close-enospc",
    "gnu-preserving-times",
    "bb-create-enospc",
    "bb-close-ignored",
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

/// 复刻槽（后六槽）的标签后缀。
const SWAP_SUFFIX: &str = "-swap";

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

    /// 该档是否只由复刻槽变红（本卡的立卡理由）。
    pub fn reds_are_replicas(&self) -> bool {
        self.red() == 0 || self.red() == REJECT
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
/// 分档只看**实际结果**落进哪一档，申报正确与否单独计。本卡的档是按
/// "退出码 + 归因位"投影的，因此 busybox 丢 close 错误那一格会落进 warned
/// 而不是 aborted——工具的结论本来就是"码 0"，把它硬塞进中止档就等于替
/// busybox 补了它没做的归因。红档槽仍留在自己那一档里，覆盖面才看得见。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行（complete/warned/tail/aborted）。
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
        let tiers = [Tier::Complete, Tier::Warned, Tier::Tail, Tier::Aborted];
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
    /// 完成档正确数。
    pub complete: u32,
    /// 告警档正确数。
    pub warned: u32,
    /// 尾段档正确数。
    pub tail: u32,
    /// 中止档正确数。
    pub aborted: u32,
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
            Err(CP_ECOUNT)
        }
    }

    /// 码 0 档合计（退出码为成功的那两档）。
    pub fn zero_cells(&self) -> u32 {
        self.complete + self.warned
    }

    /// 码非 0 档合计。
    pub fn nonzero_cells(&self) -> u32 {
        self.tail + self.aborted
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("cp-abort corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" complete=");
        s.push_str(u32_text(self.complete).as_str());
        s.push_str(" warned=");
        s.push_str(u32_text(self.warned).as_str());
        s.push_str(" tail=");
        s.push_str(u32_text(self.tail).as_str());
        s.push_str(" aborted=");
        s.push_str(u32_text(self.aborted).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 全绿渲染的逐字钉版。
const RENDER_OK: &str = "cp-abort corpus=14 accept=8 reject=6 complete=1 warned=2 tail=2 aborted=3 match=14 rate=1000ppm\n";

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 可注入一槽缺陷的全跑。
///
/// 注入的作用是把"期望表"本身也放进被测面：强行宣称某槽犯了另一种缺陷，
/// 报表就必须掉 matched、rate 随之离开 1000。若注入后仍报全绿，说明 matched
/// 是硬编码的，而不是算出来的。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(CP_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut complete = 0u32;
    let mut warned = 0u32;
    let mut tail = 0u32;
    let mut aborted = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(CP_ENOBS),
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
            None => return Err(CP_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(&a) {
                Tier::Complete => complete += 1,
                Tier::Warned => warned += 1,
                Tier::Tail => tail += 1,
                Tier::Aborted => aborted += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, complete, warned, tail, aborted, matched })
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门。
///
/// 观测纵线是 `open → stat → copy → werr → cerr → pres → exit`，七段只能向前。
/// 回退（例如已经判死在 close 段又声称停在写段）说明申报者不是按闸顺序观测的，
/// 而是拿着文案倒推——这类申报在真实故障里最危险，因为文案看着完全合理。
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
            return Err(CP_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(CP_ESTAGE);
        }
        self.last = o;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }

    /// 关闭，返回段数。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(CP_ESTATE);
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
    /// 槽位（必须严格等于当前长度）。
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

    /// 登记一格；槽位不等于当前长度、越界或已冻结一律拒绝并计数。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CP_ESLOT);
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
            return Err(CP_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CP_EBAL),
            };
            if s.slot as usize != i {
                return Err(CP_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CP_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CP_EVALUE);
                        }
                    }
                    None => return Err(CP_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(CP_EVALUE);
                }
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CP_EVALUE);
                        }
                    }
                    None => return Err(CP_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(CP_EBAL);
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
            return Err(CP_EBAL);
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
            Err(CP_ESEAL)
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
    let _ = l.register(count_cell(3, OP_OPEN, r.map(|x| x.complete)));
    let _ = l.register(count_cell(4, OP_STAT, r.map(|x| x.warned)));
    let _ = l.register(count_cell(5, OP_PRES, r.map(|x| x.tail)));
    let _ = l.register(count_cell(6, OP_WERR, r.map(|x| x.aborted)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_REPLAY, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_CERR,
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
/// 1. 表路——段基址、枚举、文件系统画像、形状合法域；
/// 2. 判路——七闸纵线、中止归因三元组、对判、矩阵；
/// 3. 渲染路——语料、期望表、红档、文案、释义；
/// 4. 账本路——阶段闸门、逐格对账、冻结重放。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && enums_ok() && profile_ok() && shape_ok();
    out[1] = define_ok() && abort_ok() && judge_ok() && matrix_ok();
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

/// 名称与段基址：常量链、尺寸、errno 真值、退出码、操作码、保留位一致性。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d140 {
        return false;
    }
    if CP_ESTAGE != SEG_BASE + 0x01
        || CP_ECODE != SEG_BASE + 0x02
        || CP_EVALUE != SEG_BASE + 0x03
        || CP_ENOTE != SEG_BASE + 0x04
        || CP_EDEFAULT != SEG_BASE + 0x05
        || CP_EABORT != SEG_BASE + 0x06
    {
        return false;
    }
    if CP_ESTATE != SEG_BASE + 0x07
        || CP_ECOUNT != SEG_BASE + 0x08
        || CP_ESLOT != SEG_BASE + 0x09
        || CP_ESEAL != SEG_BASE + 0x0a
        || CP_ENOBS != SEG_BASE + 0x0b
        || CP_EMATRIX != SEG_BASE + 0x0c
        || CP_EBAL != SEG_BASE + 0x0d
    {
        return false;
    }
    if CORPUS != 14 || SAMPLES != 14 || ACCEPT != 8 || REJECT != 6 || TIER_COUNT != 4 {
        return false;
    }
    // errno 必须是 Linux 真值，工具卡不许自造号码，只许引进 F11681 的结论。
    if ERR_ENOSPC != 28 || ERR_EACCES != 13 || ERR_EPERM != 1 || ERR_EISDIR != 21 {
        return false;
    }
    if ERR_EDQUOT != 122 || ERR_EIO != 5 || ERR_NONE != 0 {
        return false;
    }
    if EXIT_OK != 0 || EXIT_FILE != 1 || EXIT_HARD != 2 {
        return false;
    }
    if OP_OPEN != 1 || OP_STAT != 2 || OP_COPY != 3 || OP_WERR != 4 {
        return false;
    }
    if OP_CERR != 5 || OP_PRES != 6 || OP_CORPUS != 7 || OP_REPLAY != 8 {
        return false;
    }
    // 保留位必须互不相同且 `-a` 是全集、`-p` 是 mode|mtime。
    if PRES_NONE != 0 || PRES_MODE != 1 || PRES_TIME != 2 || PRES_XATTR != 4 || PRES_SPARSE != 8 {
        return false;
    }
    if PRES_ALL != 15 || PRES_P != 3 || PRES_BIT_COUNT != 4 {
        return false;
    }
    if pres_popcount(PRES_ALL) != 4 || pres_popcount(PRES_P) != 2 || pres_popcount(PRES_NONE) != 0
    {
        return false;
    }
    if pres_subset(PRES_ALL, PRES_P) || !pres_subset(PRES_P, PRES_ALL) {
        return false;
    }
    if strerror(ERR_ENOSPC) != "No space left on device" {
        return false;
    }
    if strerror(ERR_EACCES) != "Permission denied" {
        return false;
    }
    if strerror(ERR_EPERM) != "Operation not permitted" || strerror(ERR_EISDIR) != "Is a directory"
    {
        return false;
    }
    if strerror(ERR_EDQUOT) != "Quota exceeded" || strerror(ERR_EIO) != "Input/output error" {
        return false;
    }
    if !strerror(ERR_NONE).is_empty() || strerror(4242) != "Unknown error" {
        return false;
    }
    if ERRNO_SET_SIZE != 7 {
        return false;
    }
    let mut v = String::new();
    v.push_str("mnt/logs/day.bin");
    !v.is_empty() && cp_prefix().as_str() == "cp: "
}

/// 枚举：次序、文本唯一性、五分类不重不漏、工具面差异方向。
fn enums_ok() -> bool {
    let stages = [
        Stage::Open,
        Stage::Stat,
        Stage::Copy,
        Stage::Werr,
        Stage::ClErr,
        Stage::Pres,
        Stage::Exit,
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
        // 只有最后一段不是失败落点。
        if i + 1 == stages.len() {
            if s.failure() {
                return false;
            }
        } else if !s.failure() {
            return false;
        }
        // 数据面四段：open/copy/werr/cerr。
        if s.data_plane() != (i == 0 || i == 2 || i == 3 || i == 4) {
            return false;
        }
        // 能记 ENOSPC 归因的只有 open/werr/cerr 三段。
        if s.abort_plane() != (i == 0 || i == 3 || i == 4) {
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
        Note::Complete,
        Note::WarnSparse,
        Note::CloseIgnored,
        Note::CreateFail,
        Note::BadOption,
        Note::OmitDir,
        Note::IsDir,
        Note::SilentTrunc,
        Note::ErrorWriting,
        Note::WriteError,
        Note::CloseEnospc,
        Note::PresMode,
        Note::PresTime,
        Note::PresXattr,
        Note::PresSparse,
        Note::ForeignErr,
        Note::AbortMismatch,
    ];
    if notes.len() != 17 {
        return false;
    }
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
        // 五分类互斥：成功 / 中止 / 尾段 / 前置 / 缺陷，一注解只属一类。
        let fam = a.success() as u8
            + a.abort_family() as u8
            + a.tail_family() as u8
            + a.prelude_family() as u8
            + a.defect_family() as u8;
        if fam != 1 {
            return false;
        }
        // 只有成功族里的两格是"码 0 但现场有话"。
        if a.noisy_success() && !a.success() {
            return false;
        }
        if a.noisy_success() != (a == Note::WarnSparse || a == Note::CloseIgnored) {
            return false;
        }
        // 尾段族必须映射到唯一一个保留位；其余族一律 PRES_NONE。
        if a.tail_family() {
            if a.preserve_bit() == PRES_NONE || pres_popcount(a.preserve_bit()) != 1 {
                return false;
            }
        } else if a.preserve_bit() != PRES_NONE {
            return false;
        }
        // 只有 GNU 的两条写文案与建件/close 文案算中止族。
        if a.abort_family() && a.success() {
            return false;
        }
        n += 1;
    }
    if Note::ErrorWriting.name() != "error-writing" || Note::WriteError.name() != "write-error" {
        return false;
    }
    if Note::CloseEnospc.name() != "failed-to-close" {
        return false;
    }
    if !Note::CreateFail.abort_family() || Note::SilentTrunc.abort_family() {
        return false;
    }

    let tiers = [Tier::Complete, Tier::Warned, Tier::Tail, Tier::Aborted];
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
        if x.aborted() != (t == 3) {
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

    let modes = [
        CopyMode::Plain,
        CopyMode::Recursive,
        CopyMode::Preserve,
        CopyMode::Archive,
        CopyMode::Sparse,
    ];
    let mut d = 0usize;
    while d < modes.len() {
        let x = match modes.get(d) {
            Some(v) => *v,
            None => return false,
        };
        // 递归只有 -r 与 -a 两种；稀疏感知是 -a 与 -S。
        if x.recursive() != (x == CopyMode::Recursive || x == CopyMode::Archive) {
            return false;
        }
        if x.uses_sparse() != (x == CopyMode::Sparse || x == CopyMode::Archive) {
            return false;
        }
        if pres_subset(x.preserve_bits(), PRES_ALL) == false {
            return false;
        }
        let mut e = d + 1;
        while e < modes.len() {
            if let Some(z) = modes.get(e) {
                if x.name() == z.name() {
                    return false;
                }
            }
            e += 1;
        }
        d += 1;
    }

    // 工具面差异的方向性：GNU 更严（查 close、支持 -S 与 xattr、用法退 2）。
    if Tool::Gnu.usage_exit() != EXIT_HARD || Tool::Busybox.usage_exit() != EXIT_FILE {
        return false;
    }
    if !Tool::Gnu.reports_close_error() || Tool::Busybox.reports_close_error() {
        return false;
    }
    if !Tool::Gnu.supports_sparse_flag() || Tool::Busybox.supports_sparse_flag() {
        return false;
    }
    if !Tool::Gnu.supports_xattr_preserve() || Tool::Busybox.supports_xattr_preserve() {
        return false;
    }
    if !Tool::Gnu.diagnoses_dir_at_stat() || Tool::Busybox.diagnoses_dir_at_stat() {
        return false;
    }
    if Tool::Gnu.tail_is_chmod_only() || !Tool::Busybox.tail_is_chmod_only() {
        return false;
    }
    if Tool::Gnu.preserve_ceiling() != PRES_ALL || Tool::Busybox.preserve_ceiling() != PRES_P {
        return false;
    }
    if Tool::Gnu.text() != "gnu-cp" || Tool::Busybox.text() != "busybox-cp" {
        return false;
    }

    let faults = [
        Fault::Ok,
        Fault::Stage,
        Fault::Code,
        Fault::Value,
        Fault::Note,
        Fault::Default,
        Fault::Abort,
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
        && Fault::Stage.code() == CP_ESTAGE
        && Fault::Abort.code() == CP_EABORT
        && Fault::Estate.code() == CP_ESTATE
        && Fault::Bal.code() == CP_EBAL
}

/// 目的端画像闸：三块表全良，逐项破坏必判死；稀疏/延迟能力方向正确。
fn profile_ok() -> bool {
    let ps = profiles();
    if ps.len() != 3 {
        return false;
    }
    let mut i = 0usize;
    while i < ps.len() {
        if let Some(p) = ps.get(i) {
            if !p.well_formed() {
                return false;
            }
            if p.free_bytes().is_none() {
                return false;
            }
        }
        i += 1;
    }
    let lazy_full = match ps.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let flat_full = match ps.get(1) {
        Some(v) => *v,
        None => return false,
    };
    let half = match ps.get(2) {
        Some(v) => *v,
        None => return false,
    };
    if !lazy_full.is_full() || !flat_full.is_full() || half.is_full() {
        return false;
    }
    if lazy_full.free_bytes() != Some(0) || half.free_bytes() != Some(4096) {
        return false;
    }
    // 延迟分配只可能在 close 报错；不支持稀疏就不能跳洞。
    if !lazy_full.close_can_fail() || flat_full.close_can_fail() {
        return false;
    }
    if !lazy_full.can_skip_holes() || flat_full.can_skip_holes() || half.can_skip_holes() {
        return false;
    }
    if half.xattr_ok {
        return false;
    }
    // 逐项破坏。
    let mut b = lazy_full;
    b.mount = "";
    if b.well_formed() {
        return false;
    }
    b.mount = "/mnt/full";
    b.bsize = 4095;
    if b.well_formed() {
        return false;
    }
    b.bsize = 0;
    if b.well_formed() {
        return false;
    }
    b.bsize = 4096;
    b.blocks_free = 0x0800_0001;
    if b.well_formed() {
        return false;
    }
    b.blocks_free = 0x0800_0000;
    // 块数本身在上界内，但乘法溢出 → free_bytes 必为 None，画像判死。
    if b.well_formed() || b.free_bytes().is_some() {
        return false;
    }
    b.blocks_free = 0;
    // 复原必须重新良好，且指纹可复算。
    b.well_formed()
        && lazy_full.seal() == FsProfile {
            mount: lazy_full.mount,
            bsize: lazy_full.bsize,
            blocks_free: lazy_full.blocks_free,
            delalloc: lazy_full.delalloc,
            sparse_ok: lazy_full.sparse_ok,
            xattr_ok: lazy_full.xattr_ok,
        }
        .seal()
        && lazy_full.seal() != flat_full.seal()
        && half.seal() != flat_full.seal()
}

/// 稀疏记账：seek 与 write 之差只在能跳洞时存在。
fn holes_ok() -> bool {
    let skip = Holes { apparent: 8192, hole: 4096, skip_ok: true };
    let write = Holes { apparent: 8192, hole: 4096, skip_ok: false };
    let none = Holes { apparent: 8192, hole: 0, skip_ok: true };
    if !skip.balanced() || !write.balanced() || !none.balanced() {
        return false;
    }
    if skip.spread() != 4096 || write.spread() != 8192 || none.spread() != 8192 {
        return false;
    }
    if skip.holes_spread() != 0 || write.holes_spread() != 4096 {
        return false;
    }
    // 洞大于表观尺寸是坏记账（不可信的形状）。
    let bad = Holes { apparent: 1024, hole: 2048, skip_ok: false };
    if bad.balanced() {
        return false;
    }
    let blank = Holes::blank();
    if blank.apparent != 0 || blank.spread() != 0 || !blank.balanced() {
        return false;
    }
    skip.seal() != write.seal() && skip.seal() != none.seal() && skip.seal() == skip.seal()
}

/// 形状合法域：能力边界、字节自洽、错误标记互斥。

fn shape_ok() -> bool {
    let mut g = Shape::blank();
    g.tag = "probe";
    g.size = 8192;
    g.written = 8192;
    if !g.valid() {
        return false;
    }
    // busybox 没有 `-S`：该组合是不可判形状。
    let mut bb = g.clone_tool(Tool::Busybox);
    bb.mode = CopyMode::Sparse;
    bb.preserved = PRES_SPARSE;
    if bb.valid() {
        return false;
    }
    bb.mode = CopyMode::Plain;
    bb.preserved = PRES_NONE;
    if !bb.valid() {
        return false;
    }
    // busybox 的保留上限是 mode|mtime：要 xattr 即非法。
    let mut bx = bb;
    bx.mode = CopyMode::Archive;
    bx.preserved = PRES_ALL;
    if bx.valid() {
        return false;
    }
    bx.preserved = PRES_P;
    if !bx.valid() {
        return false;
    }
    // 超出模式声明的保留要求同样非法。
    let mut gp = g;
    gp.preserved = PRES_TIME;
    if gp.valid() {
        return false;
    }
    // 洞大于表观尺寸。
    let mut hd = g;
    hd.holes = 9000;
    if hd.valid() {
        return false;
    }
    // 稀疏生效时写入字节不得超过 payload。
    let mut sp = g;
    sp.mode = CopyMode::Sparse;
    sp.fs_sparse = true;
    sp.holes = 4096;
    if sp.payload() != 4096 || sp.valid() {
        return false;
    }
    sp.written = 4096;
    if !sp.valid() || sp.sparse_warning() || sp.geometry().spread() != sp.payload() {
        return false;
    }
    // 目的端不支持稀疏时洞要占额度：payload 回到表观尺寸。
    let mut ws = sp;
    ws.fs_sparse = false;
    ws.written = 8192;
    if ws.payload() != 8192 || !ws.sparse_warning() || !ws.valid() {
        return false;
    }
    // errno 合法域。
    let mut er = g.clone_errno(ERR_ENOSPC);
    if !er.valid() {
        return false;
    }
    er = er.clone_errno(4242);
    if er.valid() {
        return false;
    }
    er = er.clone_errno(-1);
    if er.valid() {
        return false;
    }
    // 遍历标志只在递归模式下成立。
    let mut tr = g;
    tr.traversal = true;
    if tr.valid() {
        return false;
    }
    tr.mode = CopyMode::Recursive;
    if !tr.valid() {
        return false;
    }
    // 用法错与字节/错误标记互斥。
    let mut bo = g;
    bo.bad_option = true;
    if bo.valid() {
        return false;
    }
    bo.written = 0;
    if !bo.valid() || bo.code_of_usage() != EXIT_HARD {
        return false;
    }
    bo.tool = Tool::Busybox;
    if bo.code_of_usage() != EXIT_FILE {
        return false;
    }
    // 建件失败：字节必为 0，errno 必 nonzero，且不得再带循环错误。
    let mut cf = g.clone_written(0);
    cf.create_ok = false;
    if cf.valid() {
        return false;
    }
    cf.errno_seen = ERR_ENOSPC;
    if !cf.valid() {
        return false;
    }
    cf.write_err = true;
    if cf.valid() {
        return false;
    }
    cf.write_err = false;
    cf.close_err = true;
    if cf.valid() {
        return false;
    }
    // 写错与 close 错不得同时成立（写红了就走不到 close）。
    let mut both = g;
    both.written = 4096;
    both.write_err = true;
    both.errno_seen = ERR_ENOSPC;
    both.close_err = true;
    if both.valid() {
        return false;
    }
    both.close_err = false;
    if !both.valid() {
        return false;
    }
    // 尾段失败要求 errno 有据、且必须是保留要求的子集。
    let mut pf = g;
    pf.mode = CopyMode::Archive;
    pf.preserved = PRES_ALL;
    pf.pres_failed = PRES_MODE;
    if pf.valid() {
        return false;
    }
    pf.errno_seen = ERR_EPERM;
    if !pf.valid() {
        return false;
    }
    pf.pres_failed = PRES_MODE | 16;
    if pf.valid() {
        return false;
    }
    // clone_* 只改目标维。
    let base = Shape { tag: "clone-src", src: "a", dst: "b", size: 1024, written: 512, ..Shape::blank() };
    let w1 = base.clone_written(1024);
    if w1.written != 1024 || w1.size != 1024 || w1.tag != base.tag || w1.dst != base.dst {
        return false;
    }
    let e1 = base.clone_errno(ERR_EDQUOT);
    if e1.errno_seen != ERR_EDQUOT || e1.written != base.written {
        return false;
    }
    let t1 = base.clone_tool(Tool::Busybox);
    if t1.tool != Tool::Busybox || t1.mode != base.mode || t1.size != base.size {
        return false;
    }
    // 指纹对每一维都敏感。
    let s0 = base.seal();
    let s1 = base.clone_written(513).seal();
    let s2 = base.clone_errno(ERR_EACCES).seal();
    let s3 = base.clone_tool(Tool::Busybox).seal();
    let mut m4 = base;
    m4.write_err = true;
    let s4 = m4.seal();
    let mut m5 = base;
    m5.pres_failed = PRES_TIME;
    let s5 = m5.seal();
    s0 != s1 && s0 != s2 && s0 != s3 && s0 != s4 && s0 != s5 && s3 != s4
}

/// 定义纵线：七闸逐段推进，先短路的闸不得被后闸改写。
fn define_ok() -> bool {
    let mut g = Shape::blank();
    g.tag = "gate-open";
    g.size = 8192;
    g.written = 8192;

    // 闸一：用法错。GNU 退 2、busybox 退 1，两者都不动留档字节。
    let mut bo = g;
    bo.bad_option = true;
    bo.written = 0;
    let o = define(&bo);
    if o.stage != Stage::Open || o.code != EXIT_HARD || o.note != Note::BadOption || o.value != 0
        || o.default
    {
        return false;
    }
    if o.ok() || Tier::of(&o) != Tier::Tail {
        return false;
    }
    if define(&bo.clone_tool(Tool::Busybox)).code != EXIT_FILE {
        return false;
    }

    // 闸一之二：建件失败。ENOSPC 记中止、邻居记 foreign。
    let mut cf = bo;
    cf.bad_option = false;
    cf.create_ok = false;
    cf.errno_seen = ERR_ENOSPC;
    let o = define(&cf);
    if o.stage != Stage::Open
        || o.code != EXIT_FILE
        || o.note != Note::CreateFail
        || o.value != 0
        || !o.default
        || !o.attributed()
    {
        return false;
    }
    if Tier::of(&o) != Tier::Aborted || !o.abort_consistent(&cf) {
        return false;
    }
    let foreign = cf.clone_errno(ERR_EACCES);
    let o = define(&foreign);
    if o.note != Note::ForeignErr || o.default || o.code != EXIT_FILE || Tier::of(&o) != Tier::Tail
    {
        return false;
    }

    // 闸二：目录诊断落点。GNU 在 stat、busybox 晚两道闸到 copy。
    let mut dir = g;
    dir.src = "logs";
    dir.dst = "mnt/logs";
    dir.src_dir = true;
    dir.size = 0;
    dir.written = 0;
    let o = define(&dir);
    if o.stage != Stage::Stat
        || o.note != Note::OmitDir
        || o.code != EXIT_FILE
        || o.value != 0
        || o.default
    {
        return false;
    }
    if o.stage.order() >= Stage::Copy.order() {
        return false;
    }
    let o = define(&dir.clone_tool(Tool::Busybox));
    if o.stage != Stage::Copy || o.note != Note::IsDir || o.code != EXIT_FILE {
        return false;
    }
    // 带 `-r` 的同一目录源必须走完全程（遍历正例）。
    let mut okdir = dir;
    okdir.mode = CopyMode::Recursive;
    okdir.traversal = true;
    let o = define(&okdir);
    if !o.ok() || o.note != Note::Complete || o.stage != Stage::Exit {
        return false;
    }

    // 闸三：循环里的静默截断（少了字节却没报错）。
    let mut silent = g.clone_written(1024);
    silent.tag = "gate-copy";
    let o = define(&silent);
    if o.stage != Stage::Copy || o.note != Note::SilentTrunc || o.code != EXIT_FILE {
        return false;
    }
    if o.value != 1024 || o.default || o.shortfall(&silent) != 7168 {
        return false;
    }
    if Tier::of(&o) != Tier::Tail || !message(&silent, &o).contains("silent truncation") {
        return false;
    }

    // 闸四：写错误归因。两工具文案不同、归因相同。
    let mut we = silent.clone_written(4096);
    we.tag = "gate-werr";
    we.write_err = true;
    we.errno_seen = ERR_ENOSPC;
    we.mode = CopyMode::Recursive;
    we.traversal = true;
    let o = define(&we);
    if o.stage != Stage::Werr
        || o.note != Note::ErrorWriting
        || o.code != EXIT_FILE
        || o.value != 4096
        || !o.default
    {
        return false;
    }
    if !o.attributed() || !o.abort_consistent(&we) {
        return false;
    }
    if Tier::of(&o) != Tier::Aborted {
        return false;
    }
    // 同一现场的静默检查：满盘留半件与静默截断必须分属两闸。
    if define(&silent).stage == define(&we).stage {
        return false;
    }
    let bbw = define(&we.clone_tool(Tool::Busybox));
    if bbw.note != Note::WriteError || bbw.stage != Stage::Werr || !bbw.default {
        return false;
    }
    if bbw.seal() == o.seal() {
        return false;
    }
    // 配额满不是 ENOSPC。
    let dq = define(&we.clone_errno(ERR_EDQUOT));
    if dq.note != Note::ForeignErr || dq.default || dq.code != EXIT_FILE {
        return false;
    }

    // 闸五：close 才爆。GNU 记中止，busybox 吞掉并退 0。
    let mut ce = g;
    ce.tag = "gate-cerr";
    ce.delalloc = true;
    ce.close_err = true;
    ce.errno_seen = ERR_ENOSPC;
    let o = define(&ce);
    if o.stage != Stage::ClErr
        || o.note != Note::CloseEnospc
        || o.code != EXIT_FILE
        || o.value != 8192
        || !o.default
        || !o.abort_consistent(&ce)
    {
        return false;
    }
    if o.shortfall(&ce) != 0 {
        return false;
    }
    let bco = define(&ce.clone_tool(Tool::Busybox));
    if bco.stage != Stage::Exit || bco.note != Note::CloseIgnored || bco.code != EXIT_OK {
        return false;
    }
    if bco.default || bco.attributed() || Tier::of(&bco) != Tier::Warned {
        return false;
    }
    // 只有丢弃 close 错误的工具才有"未记账延迟块"：GNU 一句记 0，busybox 记全额。
    let bb_ce = ce.clone_tool(Tool::Busybox);
    if bb_ce.deferred_lost() != 8192 || ce.deferred_lost() != 0 || g.deferred_lost() != 0 {
        return false;
    }
    if message(&bb_ce, &bco)
        != "close error dropped by busybox for 'b.bin' (8192 bytes of deferred blocks unaccounted)"
    {
        return false;
    }
    let io = define(&ce.clone_errno(ERR_EIO));
    if io.note != Note::ForeignErr || io.stage != Stage::ClErr || io.default {
        return false;
    }

    // 闸六：属性尾段。不改留档字节，只把码推到 1。
    let mut pf = g;
    pf.tag = "gate-pres";
    pf.mode = CopyMode::Archive;
    pf.traversal = true;
    pf.preserved = PRES_ALL;
    pf.pres_failed = PRES_TIME;
    pf.errno_seen = ERR_EPERM;
    let o = define(&pf);
    if o.stage != Stage::Pres || o.note != Note::PresTime || o.code != EXIT_FILE || o.value != 8192
        || o.default
    {
        return false;
    }
    if Tier::of(&o) != Tier::Tail || !o.note.tail_family() {
        return false;
    }
    // 多位同时失败按 mode→mtime→xattr→sparse 的固定次序取首位。
    pf.pres_failed = PRES_MODE | PRES_TIME | PRES_XATTR;
    if define(&pf).note != Note::PresMode {
        return false;
    }
    pf.pres_failed = PRES_XATTR;
    if define(&pf).note != Note::PresXattr {
        return false;
    }
    pf.pres_failed = PRES_SPARSE;
    let o = define(&pf);
    if o.note != Note::PresSparse || !o.note.tail_family() {
        return false;
    }
    // 尾段的 ENOSPC 仍不算中止：数据已经到位。
    pf.errno_seen = ERR_ENOSPC;
    let o = define(&pf);
    if o.default || o.code != EXIT_FILE || o.stage != Stage::Pres {
        return false;
    }

    // 闸七：退出码聚合。纯告警退 0。
    let mut warn = g;
    warn.tag = "gate-exit";
    warn.mode = CopyMode::Sparse;
    warn.preserved = PRES_SPARSE;
    warn.holes = 4096;
    warn.fs_sparse = false;
    if warn.payload() != 8192 {
        return false;
    }
    let o = define(&warn);
    if o.stage != Stage::Exit || o.note != Note::WarnSparse || o.code != EXIT_OK || !o.ok() {
        return false;
    }
    if o.default || Tier::of(&o) != Tier::Warned {
        return false;
    }
    if !message(&warn, &o).starts_with("cp: warning: behavior of cp is dependent") {
        return false;
    }
    if !message(&g, &define(&g)).is_empty() {
        return false;
    }
    if define(&g).note != Note::Complete || !define(&g).ok() {
        return false;
    }
    // 幂等与异形：同形同指纹，异形异指纹。
    define(&we).seal() == define(&we).seal() && define(&we).seal() != define(&ce).seal()
}

/// 中止归因三元组：文案、留档字节、退出码必须同族。
fn abort_ok() -> bool {
    // 写错文案要求"半件留档"。
    let mut we = Shape::blank();
    we.tag = "attr-write";
    we.src = "logs/day.bin";
    we.dst = "mnt/logs/day.bin";
    we.mode = CopyMode::Recursive;
    we.traversal = true;
    we.size = 8192;
    we.written = 4096;
    we.write_err = true;
    we.errno_seen = ERR_ENOSPC;
    let o = define(&we);
    if !o.attributed() || !o.abort_consistent(&we) {
        return false;
    }
    // 手工造一个矛盾三元组：说中止、字节却说写全了。
    let lie = Outcome { stage: Stage::Werr, code: EXIT_FILE, value: 8192, note: o.note, default: true };
    if lie.abort_consistent(&we) {
        return false;
    }
    // 归因位为假时三元组检查一律放行（成功与尾段不受此约束）。
    let free = Outcome { stage: Stage::Exit, code: EXIT_OK, value: 8192, note: Note::Complete, default: false };
    if !free.abort_consistent(&we) || free.attributed() {
        return false;
    }
    // close 错文案要求"字节全在"。
    let mut ce = Shape::blank();
    ce.tag = "attr-close";
    ce.src = "db.dump";
    ce.dst = "mnt/db.dump";
    ce.size = 8192;
    ce.written = 8192;
    ce.delalloc = true;
    ce.close_err = true;
    ce.errno_seen = ERR_ENOSPC;
    if !define(&ce).abort_consistent(&ce) {
        return false;
    }
    let short_close = ce.clone_written(4096);
    if define(&short_close).abort_consistent(&short_close) {
        return false;
    }
    // 建件错要求"零字节"。
    let mut cf = Shape::blank();
    cf.tag = "attr-create";
    cf.dst = "mnt/seed.bin";
    cf.create_ok = false;
    cf.errno_seen = ERR_ENOSPC;
    let o = define(&cf);
    if o.value != 0 || !o.abort_consistent(&cf) || !o.attributed() {
        return false;
    }
    // 3 档字节 x 3 档 errno 的网格：只有 ENOSPC 那一列亮归因位。
    let grid = [0u32, 4096, 8192];
    let errs = [ERR_ENOSPC, ERR_EDQUOT, ERR_EIO];
    let mut hit = 0usize;
    let mut consistent = 0usize;
    let mut gi = 0usize;
    while gi < grid.len() {
        let mut ei = 0usize;
        while ei < errs.len() {
            if let (Some(w), Some(e)) = (grid.get(gi), errs.get(ei)) {
                let mut p = we.clone_written(*w);
                p.errno_seen = *e;
                let o = define(&p);
                if o.default != (*e == ERR_ENOSPC) {
                    return false;
                }
                if o.attributed() {
                    hit += 1;
                }
                if o.abort_consistent(&p) {
                    consistent += 1;
                }
                if o.code != EXIT_FILE {
                    return false;
                }
            }
            ei += 1;
        }
        gi += 1;
    }
    // 3x3 网格里归因命中恰为 3 列（每个字节档各一次 ENOSPC）。
    if hit != 3 {
        return false;
    }
    // 自洽性在网格里只失守于"写全了却说写错"那一格。
    consistent == 8
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
            c1.claim.stage = Stage::Pres;
            if a.stage != Stage::Pres && judge(&c1) != Fault::Stage {
                return false;
            }
            if a.stage == Stage::Pres {
                c1.claim.stage = Stage::Open;
                if judge(&c1) != Fault::Stage {
                    return false;
                }
            }
            // 退出码单维注入：码改了不影响成败族的话会先撞 Estate。
            let mut c3 = c;
            c3.claim.code = a.code + 2;
            let f3 = judge(&c3);
            if f3 != Fault::Code && f3 != Fault::Estate {
                return false;
            }
        }
        i += 1;
    }
    // 状态互报必须优先：实际中止 + 成功申报 → Estate（不是 Stage/Code/Note）。
    let red = match case_at(8) {
        Some(c) => c,
        None => return false,
    };
    if red.actual().ok() || judge(&red) != Fault::Estate {
        return false;
    }
    // 反向互报：成功实际 + 失败申报 → 同为 Estate。
    let green = match case_at(0) {
        Some(c) => c,
        None => return false,
    };
    let mut g2 = green;
    g2.claim =
        Claim { stage: Stage::Werr, code: 1, value: 4096, note: Note::ErrorWriting, default: true };
    if judge(&g2) != Fault::Estate {
        return false;
    }
    // busybox 的 close 忽略格：申报者若替它补上归因，必须落到 Default。
    let mut b = match case_at(7) {
        Some(c) => c,
        None => return false,
    };
    b.claim.default = true;
    if judge(&b) != Fault::Default {
        return false;
    }
    // busybox 那格的码若被申报成 1（替工具"纠正"），必须先撞 Estate。
    b.claim.default = false;
    b.claim.code = EXIT_FILE;
    judge(&b) == Fault::Estate
}

/// 矩阵统计：分档覆盖、红档只出现在复刻槽那一档。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    if m.slots() != CORPUS || m.goods() != ACCEPT {
        return false;
    }
    if !m.covered() {
        return false;
    }
    let complete = match m.row(Tier::Complete) {
        Some(r) => r,
        None => return false,
    };
    let warned = match m.row(Tier::Warned) {
        Some(r) => r,
        None => return false,
    };
    let tail = match m.row(Tier::Tail) {
        Some(r) => r,
        None => return false,
    };
    let aborted = match m.row(Tier::Aborted) {
        Some(r) => r,
        None => return false,
    };
    if complete.total != 1 || complete.good != 1 {
        return false;
    }
    if warned.total != 2 || warned.good != 2 {
        return false;
    }
    if tail.total != 2 || tail.good != 2 {
        return false;
    }
    // 六格复刻同一中止现场：该档 total 9、good 3、red 6。
    if aborted.total != 9 || aborted.good != 3 || aborted.red() != 6 {
        return false;
    }
    if !aborted.reds_are_replicas() {
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
        || r.complete != complete.good as u32
        || r.warned != warned.good as u32
        || r.tail != tail.good as u32
        || r.aborted != aborted.good as u32
    {
        return false;
    }
    if r.zero_cells() + r.nonzero_cells() != r.accept {
        return false;
    }
    if r.matched != r.total || !r.green() || r.gate() != Ok(()) {
        return false;
    }
    // 注入一槽缺陷后：matched 必降、rate 必离 1000、gate 必报计数错。
    let red = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    red.matched == r.matched - 1
        && red.rate() == 929
        && red.gate() == Err(CP_ECOUNT)
        && !red.green()
        && red.accept == r.accept - 1
        && red.complete == r.complete - 1
        && run_with(Some((CORPUS, Fault::Code))).is_err()
        && m.row(Tier::Complete).map(|x| x.render(Tier::Complete)).is_some()
}

// ---------------------------------------------------------------------------
// 语料 / 槽位 / 红档
// ---------------------------------------------------------------------------

/// 差分位：申报与现场在"成败"这维不一致。
const D_ESTATE: u8 = 1;
/// 差分位：结束阶段不一致。
const D_STAGE: u8 = 2;
/// 差分位：退出码不一致。
const D_CODE: u8 = 4;
/// 差分位：留档字节不一致。
const D_VALUE: u8 = 8;
/// 差分位：文案落点不一致。
const D_NOTE: u8 = 16;
/// 差分位：归因位不一致。
const D_DEFAULT: u8 = 32;
/// 五个观测维全不一致（复刻槽 estate-swap 的形态：整份绿申报贴到红现场上）。
const D_DATA_ALL: u8 = D_STAGE | D_CODE | D_VALUE | D_NOTE | D_DEFAULT;

/// 14 槽申报与现场的差分掩码（语料的第三份陈述）。
///
/// 前 8 槽必须逐维等于现场（掩码 0）；后 6 槽各只允许**判序里最先撞上的那
/// 一维**决定缺陷归属，因此掩码写死在这里，任何一处手滑都会在这条上炸开。
const WANT_DIFF: [u8; CORPUS] = [
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    D_ESTATE | D_DATA_ALL,
    D_STAGE,
    D_CODE,
    D_VALUE,
    D_NOTE,
    D_DEFAULT,
];

/// 数一维：申报与现场到底差在哪几维（与 `judge()` 独立实现，互校用）。
fn dims_off(a: &Outcome, k: &Claim) -> u8 {
    let mut m = 0u8;
    if a.ok() != k.success() {
        m |= D_ESTATE;
    }
    if a.stage != k.stage {
        m |= D_STAGE;
    }
    if a.code != k.code {
        m |= D_CODE;
    }
    if a.value != k.value {
        m |= D_VALUE;
    }
    if a.note != k.note {
        m |= D_NOTE;
    }
    if a.default != k.default {
        m |= D_DEFAULT;
    }
    m
}

/// 掩码里的置位数。
fn dim_popcount(mut m: u8) -> usize {
    let mut n = 0usize;
    while m != 0 {
        if m & 1 != 0 {
            n += 1;
        }
        m = (m >> 1) as u8;
    }
    n
}

/// 掩码 → 判序里第一个撞上的缺陷（与 `judge()` 同序的独立重导）。
fn dim_fault(m: u8) -> Fault {
    if m & D_ESTATE != 0 {
        return Fault::Estate;
    }
    if m & D_STAGE != 0 {
        return Fault::Stage;
    }
    if m & D_CODE != 0 {
        return Fault::Code;
    }
    if m & D_VALUE != 0 {
        return Fault::Value;
    }
    if m & D_NOTE != 0 {
        return Fault::Note;
    }
    if m & D_DEFAULT != 0 {
        return Fault::Default;
    }
    Fault::Ok
}

/// 两处形状是否同一现场（逐判定字段比对，只看标签例外）。
///
/// 复刻槽的定义就是"同一现场、申报不同"；若现场本身也被顺手改掉，红档就不再
/// 只指向那一维缺陷，判据的归因力会被稀释。
fn same_scene(x: &Shape, y: &Shape) -> bool {
    x.src == y.src
        && x.dst == y.dst
        && x.tool == y.tool
        && x.mode == y.mode
        && x.size == y.size
        && x.written == y.written
        && x.holes == y.holes
        && x.fs_sparse == y.fs_sparse
        && x.delalloc == y.delalloc
        && x.src_dir == y.src_dir
        && x.bad_option == y.bad_option
        && x.create_ok == y.create_ok
        && x.write_err == y.write_err
        && x.close_err == y.close_err
        && x.errno_seen == y.errno_seen
        && x.preserved == y.preserved
        && x.pres_failed == y.pres_failed
        && x.traversal == y.traversal
}

/// 语料自洽：形状合法、期望表与纵线实跑一致、复刻槽各错一维。
///
/// 本卡把同一件事写了三份：`CLAIMS`（申报）、`WANT_*`（期望）、`WANT_DIFF`
/// （差分）。三份必须互相圆回来——只有一份的话，笔误会静默地变成"自洽的错"。
fn corpus_ok() -> bool {
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS {
        return false;
    }
    if cases().len() != CORPUS {
        return false;
    }
    if cover(CORPUS) != Ok(()) {
        return false;
    }
    if cover(CORPUS - 1) != Err(CP_ENOBS) || cover(0) != Err(CP_ENOBS) {
        return false;
    }
    // 段基址渲染出来的十六进制形态与常量互校（0x6d140 段，末码 0x6d14d）。
    if hex_text(SEG_BASE, 5).as_str() != "6d140" {
        return false;
    }
    if hex_text(CP_EBAL, 5).as_str() != "6d14d" {
        return false;
    }
    if hex_text(SEG_BASE, 6).as_str() != "06d140" {
        return false;
    }
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if !c.shape.valid() {
            return false;
        }
        if c.shape.tag.is_empty() || c.shape.dst.is_empty() {
            return false;
        }
        let want_tag = match WANT_TAGS.get(i) {
            Some(t) => *t,
            None => return false,
        };
        if c.tag() != want_tag {
            return false;
        }
        let a = c.actual();
        // 期望表是"纵线实跑"的另一种写法：与实跑逐维相同才算数。
        if WANT_STAGE.get(i) != Some(&a.stage) {
            return false;
        }
        if WANT_CODE.get(i) != Some(&a.code) {
            return false;
        }
        if WANT_VALUE.get(i) != Some(&a.value) {
            return false;
        }
        if WANT_NOTE.get(i) != Some(&a.note) {
            return false;
        }
        if WANT_DEFAULT.get(i) != Some(&a.default) {
            return false;
        }
        // 期望表与申报的关系：活格必须重合，死格只在被判的那维分开。
        if i < ACCEPT {
            if WANT_STAGE.get(i) != Some(&c.claim.stage) || WANT_CODE.get(i) != Some(&c.claim.code) {
                return false;
            }
            if WANT_VALUE.get(i) != Some(&c.claim.value) || WANT_NOTE.get(i) != Some(&c.claim.note) {
                return false;
            }
            if WANT_DEFAULT.get(i) != Some(&c.claim.default) {
                return false;
            }
        }
        let m = match WANT_DIFF.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if dims_off(&a, &c.claim) != m {
            return false;
        }
        // 独立重导的判序与 judge() 必须一致（两处实现互相看住）。
        if judge(&c) != dim_fault(m) {
            return false;
        }
        if i < ACCEPT && m != 0 {
            return false;
        }
        if i >= ACCEPT && m == 0 {
            return false;
        }
        i += 1;
    }
    // 标签唯一，且复刻槽一律带 -swap 后缀（活槽不带）。
    let mut p = 0usize;
    while p < CORPUS {
        let t = match WANT_TAGS.get(p) {
            Some(v) => *v,
            None => return false,
        };
        let mut q = p + 1;
        while q < CORPUS {
            if let Some(u) = WANT_TAGS.get(q) {
                if *u == t {
                    return false;
                }
            }
            q += 1;
        }
        if p < ACCEPT && t.ends_with(SWAP_SUFFIX) {
            return false;
        }
        if p >= ACCEPT && !t.ends_with(SWAP_SUFFIX) {
            return false;
        }
        p += 1;
    }
    // 六格复刻的是同一现场：实际结果指纹必须互等，且等于活槽 3 的指纹。
    let base = match cs.get(3) {
        Some(v) => *v,
        None => return false,
    };
    let base_seal = base.actual().seal();
    let mut k = ACCEPT;
    while k < CORPUS {
        let one = match cs.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if one.actual().seal() != base_seal {
            return false;
        }
        if !same_scene(&one.shape, &base.shape) {
            return false;
        }
        if one.shape.seal() == base.shape.seal() {
            return false;
        }
        k += 1;
    }
    // 申报全同则缺陷全同的反例：复刻槽彼此申报必须不同。
    let mut x = ACCEPT;
    while x < CORPUS {
        let a = match cs.get(x) {
            Some(v) => *v,
            None => return false,
        };
        let mut y = x + 1;
        while y < CORPUS {
            if let Some(b) = cs.get(y) {
                if b.claim == a.claim {
                    return false;
                }
            }
            y += 1;
        }
        x += 1;
    }
    base.tag() == "gnu-r-mid-copy-enospc" && dim_popcount(D_ESTATE | D_DATA_ALL) == 6
}

/// 槽位划分：活槽/死槽两半覆盖 0..14 不重不漏，值表与码表各就各位。
fn slots_ok() -> bool {
    if ACCEPT_SLOTS.len() != ACCEPT || REJECT_SLOTS.len() != REJECT {
        return false;
    }
    if ACCEPT + REJECT != CORPUS {
        return false;
    }
    if VALS.len() != ACCEPT {
        return false;
    }
    let mut seen = [false; CORPUS];
    let mut i = 0usize;
    while i < ACCEPT {
        let v = match ACCEPT_SLOTS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if v >= CORPUS {
            return false;
        }
        if let Some(b) = seen.get_mut(v) {
            if *b {
                return false;
            }
            *b = true;
        } else {
            return false;
        }
        if expected(v) != Some(Fault::Ok) {
            return false;
        }
        if i != v {
            return false;
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < REJECT {
        let v = match REJECT_SLOTS.get(j) {
            Some(x) => *x,
            None => return false,
        };
        if v >= CORPUS {
            return false;
        }
        if let Some(b) = seen.get_mut(v) {
            if *b {
                return false;
            }
            *b = true;
        } else {
            return false;
        }
        if j != v - ACCEPT {
            return false;
        }
        if expected(v) != REJECT_FAULTS.get(j).copied() {
            return false;
        }
        if case_at(v).map(|c| c.claim) != CLAIMS.get(v).copied() {
            return false;
        }
        j += 1;
    }
    let mut z = 0usize;
    while z < CORPUS {
        if let Some(b) = seen.get(z) {
            if !*b {
                return false;
            }
        }
        z += 1;
    }
    // 值表：总数、成、败、四档、红档千分率。
    if VALS.first() != Some(&(CORPUS as u32)) {
        return false;
    }
    if VALS.get(1) != Some(&(ACCEPT as u32)) || VALS.get(2) != Some(&(REJECT as u32)) {
        return false;
    }
    if VALS.get(3) != Some(&1) || VALS.get(4) != Some(&2) || VALS.get(5) != Some(&2) {
        return false;
    }
    if VALS.get(6) != Some(&3) || VALS.last() != Some(&929) {
        return false;
    }
    // 缺陷码表：六码互异、都在本段内、且与缺陷枚举一一对应。
    let mut c = 0usize;
    while c < REJECT {
        let f = match REJECT_FAULTS.get(c) {
            Some(v) => *v,
            None => return false,
        };
        let code = match REJECT_CODES.get(c) {
            Some(v) => *v,
            None => return false,
        };
        if code != f.code() {
            return false;
        }
        if !f.alive() {
            return false;
        }
        if fault_short(f).is_empty() {
            return false;
        }
        if code <= SEG_BASE || code > SEG_BASE + 0x0d {
            return false;
        }
        let mut d = c + 1;
        while d < REJECT {
            if let Some(o) = REJECT_CODES.get(d) {
                if *o == code {
                    return false;
                }
            }
            if let Some(o) = REJECT_FAULTS.get(d) {
                if *o == f {
                    return false;
                }
            }
            d += 1;
        }
        c += 1;
    }
    // 越界：取槽与申报期望都返回 None，注入越界槽报 CP_ESLOT。
    if case_at(CORPUS).is_some() || case_at(CORPUS + 7).is_some() {
        return false;
    }
    if expected(CORPUS).is_some() || expected(CORPUS + 1).is_some() {
        return false;
    }
    let bad = match run_with(Some((CORPUS, Fault::Code))) {
        Ok(_) => CP_ECOUNT,
        Err(e) => e,
    };
    if bad != CP_ESLOT {
        return false;
    }
    // 有符号文本：errno 在文案里是符号量，取负号后必须带 "-"。
    if i32_text(ERR_NONE) != "0" {
        return false;
    }
    if i32_text(ERR_EPERM) != "1" || i32_text(ERR_EDQUOT) != "122" {
        return false;
    }
    if i32_text(-ERR_EDQUOT) != "-122" || i32_text(-ERR_NONE) != "0" {
        return false;
    }
    run().is_ok()
}

/// 红档显式：六格红全来自复刻槽，其余三档零红，摘掉任何一维都会漏红。
fn reds_ok() -> bool {
    let cs = cases();
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if r.reject != REJECT as u32 || r.accept != ACCEPT as u32 {
        return false;
    }
    if r.matched != CORPUS as u32 {
        return false;
    }
    let m = Matrix::of();
    let mut t = 0usize;
    let tiers = [Tier::Complete, Tier::Warned, Tier::Tail, Tier::Aborted];
    while t < tiers.len() {
        let tier = match tiers.get(t) {
            Some(v) => *v,
            None => return false,
        };
        let row = match m.row(tier) {
            Some(v) => v,
            None => return false,
        };
        if tier == Tier::Aborted {
            if row.red() != REJECT {
                return false;
            }
            if row.total <= row.good {
                return false;
            }
        } else if row.red() != 0 {
            return false;
        }
        if !row.reds_are_replicas() {
            return false;
        }
        t += 1;
    }
    // 活槽全绿。
    let mut i = 0usize;
    while i < ACCEPT {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if !c.ok() || judge(&c) != Fault::Ok {
            return false;
        }
        if c.actual().stage != c.claim.stage || c.actual().code != c.claim.code {
            return false;
        }
        i += 1;
    }
    // 死槽全红，且红在复刻现场（中止档）里显形。
    let mut j = ACCEPT;
    let mut seen_code = [0u32; REJECT];
    while j < CORPUS {
        let c = match cs.get(j) {
            Some(v) => *v,
            None => return false,
        };
        let f = judge(&c);
        if c.ok() || !f.alive() {
            return false;
        }
        if expected(j) != Some(f) {
            return false;
        }
        if Tier::of(&c.actual()) != Tier::Aborted {
            return false;
        }
        if !c.actual().default || !c.actual().attributed() {
            return false;
        }
        if f.code() <= SEG_BASE {
            return false;
        }
        let k = match seen_code.get_mut(j - ACCEPT) {
            Some(v) => v,
            None => return false,
        };
        *k = f.code();
        j += 1;
    }
    let mut a = 0usize;
    while a < REJECT {
        let code = match seen_code.get(a) {
            Some(v) => *v,
            None => return false,
        };
        if REJECT_CODES.get(a) != Some(&code) {
            return false;
        }
        let mut b = a + 1;
        while b < REJECT {
            if let Some(o) = seen_code.get(b) {
                if *o == code {
                    return false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    // 反证：把红的那一维抹平（等价于摘掉该维判据），对应那格必须转绿——
    // 说明六维条条在实判，没有一条是摆设。
    let mut relax = 0usize;
    while relax < REJECT {
        let c = match cs.get(relax + ACCEPT) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let mut healed = c;
        match relax {
            0 => {
                healed.claim.stage = a.stage;
                healed.claim.code = a.code;
                healed.claim.value = a.value;
                healed.claim.note = a.note;
                healed.claim.default = a.default;
            }
            1 => healed.claim.stage = a.stage,
            2 => healed.claim.code = a.code,
            3 => healed.claim.value = a.value,
            4 => healed.claim.note = a.note,
            _ => healed.claim.default = a.default,
        }
        if judge(&healed) != Fault::Ok {
            return false;
        }
        relax += 1;
    }
    // 第 0 格（estate-swap）单独把五维一起抹平才转绿：其余维都不够。
    let est = match cs.get(ACCEPT) {
        Some(v) => *v,
        None => return false,
    };
    // 第 0 格（estate-swap）单独把任一维补齐都不够：成败互报永远压过单维。
    let ea = est.actual();
    let mut est1 = est;
    est1.claim.default = ea.default;
    if judge(&est1) != Fault::Estate {
        return false;
    }
    let mut est2 = est;
    est2.claim.stage = ea.stage;
    if judge(&est2) != Fault::Estate {
        return false;
    }
    // 补上红文案那一维后成败位翻面，缺陷才退到阶段维——判序优先级的活证据。
    let mut est3 = est;
    est3.claim.note = ea.note;
    if judge(&est3) != Fault::Stage {
        return false;
    }
    r.gate() == Ok(()) && r.green() && est.tag() == "estate-swap"
}

// ---------------------------------------------------------------------------
// 文案与释义
// ---------------------------------------------------------------------------

/// GNU 建件错的落点句式（busybox 对应句式是 `can't create`）。
const GNU_CREATE_TAIL: &str = "cannot create regular file";
/// GNU 写循环错的落点句式。
const GNU_WRITE_TAIL: &str = "error writing";
/// busybox 写循环错的落点句式（同一件事，另一种措辞）。
const BB_WRITE_TAIL: &str = "write error to";
/// GNU 只在 close 才爆的句式。
const GNU_CLOSE_TAIL: &str = "failed to close";
/// ENOSPC 的 strerror 尾巴（七值表里唯一与满盘有关的那句）。
const ENOSPC_TAIL: &str = "No space left on device";
/// 稀疏告警的固定起头。
const SPARSE_HEAD: &str = "cp: warning: behavior of cp is dependent";
/// 目录被略过的固定起头。
const OMIT_HEAD: &str = "cp: -r not specified; omitting directory";

/// 文案逐字：八槽现句 + 两工具同事件的措辞分岔 + 邻居 errno 尾巴。
fn message_ok() -> bool {
    let cs = cases();
    // 八槽的现场文案（逐字符钉死）。
    let want = [
        "",
        "cp: warning: behavior of cp is dependent on the file system's sparse support, \
but it is not enabled for 'mnt/vm.img', file system does not support sparse files",
        "cp: -r not specified; omitting directory 'logs'",
        "cp: error writing 'mnt/logs/day.bin': No space left on device",
        "cp: failed to close 'mnt/db.dump': No space left on device",
        "cp: preserving times for 'mnt/cfg.conf': Operation not permitted",
        "cp: can't create 'mnt/seed.bin': No space left on device",
        "close error dropped by busybox for 'mnt/notes.txt' \
(4096 bytes of deferred blocks unaccounted)",
    ];
    if want.len() != ACCEPT {
        return false;
    }
    let mut i = 0usize;
    while i < ACCEPT {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let w = match want.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        if message(&c.shape, &a).as_str() != w {
            return false;
        }
        // 成功族只有告警行；错误族必须占满 message 通道。
        if a.note.noisy_success() {
            if warning(&c.shape, &a) != message(&c.shape, &a) {
                return false;
            }
        } else if a.note.success() {
            if !warning(&c.shape, &a).is_empty() {
                return false;
            }
        } else if !warning(&c.shape, &a).is_empty() {
            return false;
        }
        if a.note == Note::Complete && !w.is_empty() {
            return false;
        }
        if a.note.abort_family() {
            if !message(&c.shape, &a).ends_with(ENOSPC_TAIL) {
                return false;
            }
            if !a.default || a.code != EXIT_FILE {
                return false;
            }
        }
        i += 1;
    }
    // strerror 七值全互异，且未知值不静默变空串。
    let errs = [
        ERR_NONE,
        ERR_ENOSPC,
        ERR_EACCES,
        ERR_EPERM,
        ERR_EISDIR,
        ERR_EDQUOT,
        ERR_EIO,
    ];
    if errs.len() != ERRNO_SET_SIZE {
        return false;
    }
    if strerror(ERR_ENOSPC) != ENOSPC_TAIL {
        return false;
    }
    if strerror(4242) != "Unknown error" {
        return false;
    }
    // errno 0 没有 strerror 文案："没报错"必须是一句空话，不能借邻居的文案顶包。
    if !strerror(ERR_NONE).is_empty() {
        return false;
    }
    let mut p = 0usize;
    while p < errs.len() {
        let e = match errs.get(p) {
            Some(v) => *v,
            None => return false,
        };
        if e != ERR_NONE && strerror(e).is_empty() {
            return false;
        }
        let mut q = p + 1;
        while q < errs.len() {
            if let Some(f) = errs.get(q) {
                if strerror(*f) == strerror(e) {
                    return false;
                }
            }
            q += 1;
        }
        p += 1;
    }
    // 同一件事、两种措辞：写循环红的落点句必须分岔。
    let live3 = match cs.get(3) {
        Some(v) => *v,
        None => return false,
    };
    let gnu_write = message(&live3.shape, &live3.actual());
    let bb_shape = live3.shape.clone_tool(Tool::Busybox);
    let bb_out = define(&bb_shape);
    let bb_write = message(&bb_shape, &bb_out);
    if gnu_write.contains(BB_WRITE_TAIL) || !bb_write.contains(BB_WRITE_TAIL) {
        return false;
    }
    if !gnu_write.contains(GNU_WRITE_TAIL) || bb_write.contains(GNU_WRITE_TAIL) {
        return false;
    }
    if bb_out.note != Note::WriteError || bb_out.stage != Stage::Werr {
        return false;
    }
    if !bb_write.ends_with(ENOSPC_TAIL) || !bb_out.default {
        return false;
    }
    // 建件错的分岔：GNU 说 regular file，busybox 只说 can't create。
    let live6 = match cs.get(6) {
        Some(v) => *v,
        None => return false,
    };
    let bb_create = message(&live6.shape, &live6.actual());
    let gnu_create_shape = live6.shape.clone_tool(Tool::Gnu);
    let gnu_create = message(&gnu_create_shape, &define(&gnu_create_shape));
    if !bb_create.contains("can't create") || bb_create.contains(GNU_CREATE_TAIL) {
        return false;
    }
    if !gnu_create.contains(GNU_CREATE_TAIL) || gnu_create.contains("can't create ") {
        return false;
    }
    // 目录源：GNU 停在 stat 段，busybox 走到 copy 段，两句毫无重合。
    let live2 = match cs.get(2) {
        Some(v) => *v,
        None => return false,
    };
    let omit = message(&live2.shape, &live2.actual());
    let bb_dir_shape = live2.shape.clone_tool(Tool::Busybox);
    let bb_dir = define(&bb_dir_shape);
    let dir_msg = message(&bb_dir_shape, &bb_dir);
    if !omit.starts_with(OMIT_HEAD) || !omit.ends_with("'logs'") {
        return false;
    }
    if bb_dir.stage != Stage::Copy || bb_dir.note != Note::IsDir {
        return false;
    }
    if dir_msg != "cp: 'logs': Is a directory" {
        return false;
    }
    if omit.contains("Is a directory") || dir_msg.contains("-r not specified") {
        return false;
    }
    // 用法错：文案同族，退出码却分岔（GNU 2 / busybox 1）。
    let mut usage = Shape::blank();
    usage.tag = "usage";
    usage.dst = "mnt/x";
    usage.bad_option = true;
    let uo = define(&usage);
    if message(&usage, &uo) != "cp: unrecognized option '--preserve=xattr'" {
        return false;
    }
    if uo.code != EXIT_HARD || uo.stage != Stage::Open || uo.default {
        return false;
    }
    let usage_bb = usage.clone_tool(Tool::Busybox);
    let ubo = define(&usage_bb);
    if message(&usage_bb, &ubo) != "cp: unrecognized option: --preserve=xattr" {
        return false;
    }
    if ubo.code != EXIT_FILE || ubo.stage != Stage::Open {
        return false;
    }
    // 尾段四句：mode/time/xattr/sparse 各自独立，且都不带 ENOSPC 尾巴。
    let mut pf = match cs.get(5) {
        Some(v) => v.shape,
        None => return false,
    };
    pf.pres_failed = PRES_MODE;
    let po = define(&pf);
    if message(&pf, &po) != "cp: preserving permissions for 'mnt/cfg.conf': Operation not permitted"
    {
        return false;
    }
    if po.note != Note::PresMode || po.stage != Stage::Pres || po.default {
        return false;
    }
    pf.pres_failed = PRES_XATTR;
    if message(&pf, &define(&pf)) != "cp: failed to copy xattrs" {
        return false;
    }
    pf.pres_failed = PRES_SPARSE;
    if message(&pf, &define(&pf))
        != "cp: cannot preserve sparse file state for 'mnt/cfg.conf'"
    {
        return false;
    }
    pf.tool = Tool::Busybox;
    pf.preserved = PRES_P;
    pf.mode = CopyMode::Preserve;
    pf.pres_failed = PRES_MODE;
    let bo = define(&pf);
    if message(&pf, &bo) != "cp: can't chmod 'mnt/cfg.conf': Operation not permitted" {
        return false;
    }
    // 邻居 errno：落点句跟着闸走，但尾巴绝不写成满盘。
    let mut nb = match cs.get(4) {
        Some(v) => *v,
        None => return false,
    };
    nb.shape = nb.shape.clone_errno(ERR_EDQUOT);
    let no = define(&nb.shape);
    if message(&nb.shape, &no) != "cp: failed to close 'mnt/db.dump': Quota exceeded" {
        return false;
    }
    if no.note != Note::ForeignErr || no.default || Tier::of(&no) != Tier::Tail {
        return false;
    }
    nb.shape = nb.shape.clone_errno(ERR_EIO);
    nb.shape.write_err = true;
    nb.shape.close_err = false;
    let nw = define(&nb.shape);
    if !message(&nb.shape, &nw).ends_with("Input/output error") {
        return false;
    }
    if message(&nb.shape, &nw).contains(ENOSPC_TAIL) {
        return false;
    }
    // 静默截断是本卡自研审计句：不带 cp: 前缀，且明写少了多少字节。
    let mut silent = match cs.get(3) {
        Some(v) => *v,
        None => return false,
    };
    silent.shape.write_err = false;
    silent.shape.errno_seen = ERR_NONE;
    let so = define(&silent.shape);
    let sm = message(&silent.shape, &so);
    if so.note != Note::SilentTrunc || so.stage != Stage::Copy || so.code != EXIT_FILE {
        return false;
    }
    if sm != "silent truncation at 'mnt/logs/day.bin' (4096 of 8192 bytes, no error reported)" {
        return false;
    }
    if sm.starts_with("cp: ") {
        return false;
    }
    // 中止三元组失配时的专用审计句（不来自任何真实工具，只来自本卡）。
    let lie = Outcome {
        stage: Stage::Werr,
        code: EXIT_FILE,
        value: 8192,
        note: Note::AbortMismatch,
        default: true,
    };
    let mm = message(&silent.shape, &lie);
    if mm != "abort attribution broken (note=abort-mismatch, bytes=8192, exit=1)" {
        return false;
    }
    // 说中止却没自洽：归因位为真时三元组检查必须亮红。
    // 该句也不属中止族，所以归因命中（attributed）同样为假——它只在
    // 手工比对里出现，永远不会由 `define()` 产出。
    if lie.abort_consistent(&silent.shape) || lie.attributed() {
        return false;
    }
    if lie.note.abort_family() || !lie.note.defect_family() {
        return false;
    }
    !gnu_write.is_empty() && bb_write != gnu_write
}

/// 八槽释义逐字（`detail()` 只渲染现场，不渲染申报，故复刻槽与活槽同文）。
const DETAIL_LIVE: [&str; ACCEPT] = [
    "mnt/report.pdf tool=gnu-cp stage=exit exit=0 bytes=8192 abort=false note=complete\n",
    "mnt/vm.img tool=gnu-cp stage=exit exit=0 bytes=8192 abort=false note=warn-sparse msg=\"cp: warning: behavior of cp is dependent on the file system's sparse support, but it is not enabled for 'mnt/vm.img', file system does not support sparse files\"\n",
    "mnt/logs tool=gnu-cp stage=stat exit=1 bytes=0 abort=false note=omit-dir msg=\"cp: -r not specified; omitting directory 'logs'\"\n",
    "mnt/logs/day.bin tool=gnu-cp stage=werr exit=1 bytes=4096 abort=true note=error-writing msg=\"cp: error writing 'mnt/logs/day.bin': No space left on device\"\n",
    "mnt/db.dump tool=gnu-cp stage=cerr exit=1 bytes=8192 abort=true note=failed-to-close msg=\"cp: failed to close 'mnt/db.dump': No space left on device\"\n",
    "mnt/cfg.conf tool=gnu-cp stage=pres exit=1 bytes=2048 abort=false note=pres-time msg=\"cp: preserving times for 'mnt/cfg.conf': Operation not permitted\"\n",
    "mnt/seed.bin tool=busybox-cp stage=open exit=1 bytes=0 abort=true note=create-fail msg=\"cp: can't create 'mnt/seed.bin': No space left on device\"\n",
    "mnt/notes.txt tool=busybox-cp stage=exit exit=0 bytes=4096 abort=false note=close-ignored msg=\"close error dropped by busybox for 'mnt/notes.txt' (4096 bytes of deferred blocks unaccounted)\"\n",
];

/// 释义：逐槽文本与钉版一致；复刻槽共享同一句（现场相同）；全串可拼接复算。
fn detail_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < ACCEPT {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let w = match DETAIL_LIVE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let d = detail(&c);
        if d.as_str() != w {
            return false;
        }
        // 结构件：七段键名固定出现一次，且以换行收尾。
        if !d.ends_with('\n') {
            return false;
        }
        if d.matches(" tool=").count() != 1 || d.matches(" stage=").count() != 1 {
            return false;
        }
        if d.matches(" exit=").count() != 1 || d.matches(" bytes=").count() != 1 {
            return false;
        }
        if d.matches(" abort=").count() != 1 || d.matches(" note=").count() != 1 {
            return false;
        }
        if i == 0 {
            if d.contains(" msg=") {
                return false;
            }
        } else if !d.contains(" msg=\"") {
            return false;
        }
        // 释义里的阶段名必须与实跑阶段名同源。
        let st = c.actual().stage;
        if !d.contains(st.name()) {
            return false;
        }
        if st.name().len() > 6 {
            return false;
        }
        i += 1;
    }
    // 复刻槽与活槽 3 同句：detail() 只说现场，不说申报。
    let base = match cs.get(3) {
        Some(v) => *v,
        None => return false,
    };
    let base_text = detail(&base);
    let mut j = ACCEPT;
    while j < CORPUS {
        let c = match cs.get(j) {
            Some(v) => *v,
            None => return false,
        };
        if detail(&c) != base_text {
            return false;
        }
        if judge(&c).alive() && c.ok() {
            return false;
        }
        j += 1;
    }
    // 全量拼接：十四槽串起来的长度与逐槽相加一致（无静默截断）。
    let mut all = String::new();
    let mut k = 0usize;
    while k < CORPUS {
        if let Some(c) = cs.get(k) {
            all.push_str(detail(c).as_str());
        }
        k += 1;
    }
    if all.matches('\n').count() != CORPUS {
        return false;
    }
    if !all.contains("close error dropped by busybox") || !all.contains("error writing") {
        return false;
    }
    if all.contains("note=complete msg=") {
        return false;
    }
    all.len() > 800
}

// ---------------------------------------------------------------------------
// 阶段闸与账本
// ---------------------------------------------------------------------------

/// 七闸纵线（与 `Stage::order()` 同序）。
const GATE_LINE: [Stage; 7] = [
    Stage::Open,
    Stage::Stat,
    Stage::Copy,
    Stage::Werr,
    Stage::ClErr,
    Stage::Pres,
    Stage::Exit,
];

/// 阶段闸：单调前进、同闸可重踩、回退必拒、关闸后一切必拒。
///
/// 这条闸是"申报者到底按什么顺序看现场"的结构证据。真实故障里最危险的一类
/// 申报是拿着文案倒推阶段——文案完全合理，阶段却回跳了。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    if g.steps() != 0 || g.last() != -1 || g.closed() {
        return false;
    }
    let mut i = 0usize;
    while i < GATE_LINE.len() {
        let s = match GATE_LINE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if g.step(s) != Ok(()) {
            return false;
        }
        if g.steps() != (i as u32) + 1 {
            return false;
        }
        if g.last() != s.order() as i32 {
            return false;
        }
        if g.closed() {
            return false;
        }
        i += 1;
    }
    // 同一闸重踩不算回退（复制循环里同一件目的重复观测）。
    if g.step(Stage::Exit) != Ok(()) || g.steps() != 8 {
        return false;
    }
    // 回退一律 CP_ESTAGE。
    if g.step(Stage::Open) != Err(CP_ESTAGE) {
        return false;
    }
    if g.step(Stage::Stat) != Err(CP_ESTAGE) || g.step(Stage::Copy) != Err(CP_ESTAGE) {
        return false;
    }
    if g.steps() != 8 || g.last() != Stage::Exit.order() as i32 {
        return false;
    }
    // 关闸：一次成功、二次报状态错。
    if g.close() != Ok(8) {
        return false;
    }
    if !g.closed() {
        return false;
    }
    if g.close() != Err(CP_ESTATE) || g.step(Stage::Exit) != Err(CP_ESTATE) {
        return false;
    }
    // 乱序起步：只许"先高后低"里的高位通过。
    let mut h = Gate::new();
    if h.step(Stage::Exit) != Ok(()) {
        return false;
    }
    if h.step(Stage::Stat) != Err(CP_ESTAGE) {
        return false;
    }
    if h.step(Stage::Exit) != Ok(()) {
        return false;
    }
    // 空闸门直接关：段数 0。
    let mut e = Gate::new();
    if e.close() != Ok(0) {
        return false;
    }
    // 每槽实际阶段都能由"从 open 起顺序推进"走到：闸序与 define 的结论同向。
    let cs = cases();
    let mut k = 0usize;
    while k < CORPUS {
        let c = match cs.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let mut walk = Gate::new();
        let mut n = 0usize;
        while n < GATE_LINE.len() {
            let s = match GATE_LINE.get(n) {
                Some(v) => *v,
                None => return false,
            };
            if s == a.stage {
                break;
            }
            if walk.step(s) != Ok(()) {
                return false;
            }
            n += 1;
        }
        if walk.step(a.stage) != Ok(()) {
            return false;
        }
        if walk.steps() != a.stage.order() as u32 + 1 {
            return false;
        }
        // 失败落点必在 Exit 之前，成功落点必是 Exit。
        if a.stage.failure() != (a.stage != Stage::Exit) {
            return false;
        }
        if a.stage.abort_plane() && !a.stage.data_plane() {
            return false;
        }
        k += 1;
    }
    // 闸序本身：七段两两不同、面划分计数正确。
    let mut p = 0usize;
    let mut data = 0usize;
    let mut abort = 0usize;
    let mut fail = 0usize;
    while p < GATE_LINE.len() {
        let s = match GATE_LINE.get(p) {
            Some(v) => *v,
            None => return false,
        };
        if s.order() as usize != p {
            return false;
        }
        if s.data_plane() {
            data += 1;
        }
        if s.abort_plane() {
            abort += 1;
        }
        if s.failure() {
            fail += 1;
        }
        let mut q = p + 1;
        while q < GATE_LINE.len() {
            if let Some(u) = GATE_LINE.get(q) {
                if u.order() == s.order() || u.name() == s.name() {
                    return false;
                }
            }
            q += 1;
        }
        p += 1;
    }
    data == 4 && abort == 3 && fail == 6 && GATE_LINE.len() == 7
}

/// 账本：14 格、8 成 6 败、槽位严格递增、冻结后拒登记、重放指纹守恒。
///
/// 被拒过的账本指纹必变，因此它的 `replay()` 合理地报 CP_ESEAL——这不是缺陷，
/// 而是"账本被动过就回放不出来"的审计性质；本判据把这两种情形分开验证。
fn ledger_ok() -> bool {
    let mut l = build_ledger();
    if l.len() != SAMPLES || l.is_empty() {
        return false;
    }
    if l.ok_count() != ACCEPT || l.err_count() != REJECT {
        return false;
    }
    if l.rejects() != 0 {
        return false;
    }
    if l.audit() != Ok(()) {
        return false;
    }
    if l.root() != 0 || l.is_frozen() {
        return false;
    }
    // 逐格：槽位等于下标，操作码在本段操作集内。
    let mut i = 0usize;
    while i < l.len() {
        let s = match l.sample(i) {
            Some(v) => v,
            None => return false,
        };
        if s.slot as usize != i {
            return false;
        }
        if s.op < OP_OPEN || s.op > OP_REPLAY {
            return false;
        }
        if s.ok {
            if s.code != 0 || s.value == 0 {
                return false;
            }
            if VALS.get(i).copied() != Some(s.value) {
                return false;
            }
        } else if s.value != 0 {
            return false;
        }
        if i + 1 < l.len() && l.sample(i + 1).is_none() {
            return false;
        }
        i += 1;
    }
    if l.sample(SAMPLES).is_some() || l.sample(usize::MAX).is_some() {
        return false;
    }
    // 成格值表与败格码表各自落位（前 8 成、后 6 败）。
    let mut e = 0usize;
    while e < REJECT {
        let s = match l.sample(ACCEPT + e) {
            Some(v) => v,
            None => return false,
        };
        let code = match REJECT_CODES.get(e) {
            Some(v) => *v,
            None => return false,
        };
        if s.ok || s.code != code {
            return false;
        }
        if s.op != OP_CERR || s.slot as usize != ACCEPT + e {
            return false;
        }
        e += 1;
    }
    // 空账本：审计必不平（长度就是账不平的第一处证据）。
    let fresh = Ledger::new();
    if fresh.audit() != Err(CP_EBAL) || !fresh.is_empty() || fresh.len() != 0 {
        return false;
    }
    if fresh.sample(0).is_some() || fresh.ok_count() != 0 || fresh.err_count() != 0 {
        return false;
    }
    // 槽位错：跳号登记被拒且计数，补回正确槽位则放行。
    let mut s2 = Ledger::new();
    let early = Sample {
        slot: 1,
        op: OP_OPEN,
        ok: true,
        code: 0,
        value: 14,
    };
    if s2.register(early) != Err(CP_ESLOT) {
        return false;
    }
    if s2.rejects() != 1 || s2.len() != 0 {
        return false;
    }
    let good = Sample {
        slot: 0,
        op: OP_OPEN,
        ok: true,
        code: 0,
        value: 14,
    };
    if s2.register(good) != Ok(()) || s2.len() != 1 {
        return false;
    }
    let back = match s2.sample(0) {
        Some(v) => v,
        None => return false,
    };
    if back.slot != good.slot || back.op != good.op || back.ok != good.ok {
        return false;
    }
    if back.code != good.code || back.value != good.value {
        return false;
    }
    // 越界槽位同样拒。
    let over = Sample {
        slot: SAMPLES as u8,
        op: OP_COPY,
        ok: false,
        code: CP_EVALUE,
        value: 0,
    };
    if s2.register(over) != Err(CP_ESLOT) || s2.rejects() != 2 {
        return false;
    }
    // 冻结：指纹非零、幂等拒二次冻结、冻结后登记拒。
    let root = match l.freeze() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if root == 0 || !l.is_frozen() || l.root() != root {
        return false;
    }
    if l.audit() != Ok(()) {
        return false;
    }
    if l.replay() != Ok(root) {
        return false;
    }
    if l.freeze() != Err(CP_EBAL) {
        return false;
    }
    if l.register(good) != Err(CP_ESLOT) || l.rejects() != 2 {
        return false;
    }
    // 被动过的账本回放不出来：两次拒绝都进了 rejects，指纹随之漂移。
    if l.replay() == Ok(root) {
        return false;
    }
    if l.replay() != Err(CP_ESEAL) {
        return false;
    }
    // 未被拒过的账本：指纹互等，且与逐格重算一致。
    let mut x = build_ledger();
    let mut y = build_ledger();
    if x.seal() != y.seal() {
        return false;
    }
    if x.freeze() != Ok(y.seal()) {
        return false;
    }
    if y.freeze() != Ok(x.root()) || x.replay() != Ok(y.root()) {
        return false;
    }
    if x.rejects() != 0 || y.rejects() != 0 {
        return false;
    }
    l.rejects() == 2 && x.is_frozen() && y.is_frozen()
}

// ---------------------------------------------------------------------------
// 十三判据
// ---------------------------------------------------------------------------

/// F11682「cp 磁盘满中止与退出码判据」的十三项自检。
///
/// 每项一个红灯理由，全部为纯计算：不读文件、不碰时钟、不分线程。
pub fn run_unx11682_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11682_cp_enospc_abort");

    s.add("CP01-名称与段基址", name_ok(), "static");

    s.add("CP02-枚举与七闸", enums_ok(), "static");

    s.add("CP03-挂载画像与稀疏几何", profile_ok() && holes_ok(), "static");

    s.add("CP04-形状合法域", shape_ok(), "static");

    s.add("CP05-写回七闸纵线", define_ok(), "static");

    s.add("CP06-中止三元组归因", abort_ok(), "static");

    s.add("CP07-申报对判", judge_ok(), "static");

    s.add("CP08-矩阵统计", matrix_ok(), "static");

    s.add("CP09-语料与三份陈述", corpus_ok(), "static");

    s.add("CP10-槽位与红档显式", slots_ok() && reds_ok(), "static");

    s.add("CP11-文案逐字", message_ok(), "static");

    s.add("CP12-释义与阶段闸", detail_ok() && gate_ok(), "static");

    s.add(
        "CP13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            let inj = inject_four();
            let four = inj.iter().all(|x| *x);
            a && b && ledger_ok() && four
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

    // ---- 造形助手：只在测试里出现，不参与生产判定 ----

    /// 递归复制中途满盘（写循环收到 `-1/ENOSPC`）。
    fn write_scene(tool: Tool) -> Shape {
        let mut s = Shape::blank();
        s.tag = "probe-write-enospc";
        s.src = "logs/day.bin";
        s.dst = "mnt/logs/day.bin";
        s.tool = tool;
        s.mode = CopyMode::Recursive;
        s.traversal = true;
        s.size = 8192;
        s.written = 4096;
        s.write_err = true;
        s.errno_seen = ERR_ENOSPC;
        s
    }

    /// 写全了、`close()` 才爆（延迟分配现场）。
    fn close_scene(tool: Tool) -> Shape {
        let mut s = Shape::blank();
        s.tag = "probe-close-enospc";
        s.src = "db.dump";
        s.dst = "mnt/db.dump";
        s.tool = tool;
        s.size = 8192;
        s.written = 8192;
        s.delalloc = true;
        s.close_err = true;
        s.errno_seen = ERR_ENOSPC;
        s
    }

    /// 目的件建不出来（open 段）。
    fn create_scene(tool: Tool, errno: i32) -> Shape {
        let mut s = Shape::blank();
        s.tag = "probe-create-fail";
        s.dst = "mnt/seed.bin";
        s.tool = tool;
        s.size = 8192;
        s.create_ok = false;
        s.errno_seen = errno;
        s
    }

    /// 选项被本工具拒绝（open 段用法错）。
    fn usage_scene(tool: Tool) -> Shape {
        let mut s = Shape::blank();
        s.tag = "probe-bad-option";
        s.dst = "mnt/x";
        s.tool = tool;
        s.bad_option = true;
        s
    }

    /// 属性尾段失败（数据已到位）。
    fn tail_scene(bit: u8) -> Shape {
        let mut s = Shape::blank();
        s.tag = "probe-tail";
        s.dst = "mnt/cfg.conf";
        s.mode = CopyMode::Archive;
        s.traversal = true;
        s.size = 8192;
        s.written = 8192;
        s.preserved = PRES_ALL;
        s.pres_failed = bit;
        s.errno_seen = ERR_EPERM;
        s
    }

    /// 纯稀疏告警（GNU 独有的一句，退出码仍 0）。
    fn warn_scene(tool: Tool) -> Shape {
        let mut s = Shape::blank();
        s.tag = "probe-warn-sparse";
        s.dst = "mnt/vm.img";
        s.tool = tool;
        s.mode = CopyMode::Sparse;
        s.preserved = PRES_SPARSE;
        s.size = 8192;
        s.written = 8192;
        s.holes = 4096;
        s.fs_sparse = false;
        s
    }

    /// 少了字节却没报错（静默截断）。
    fn silent_scene() -> Shape {
        let mut s = Shape::blank();
        s.tag = "probe-silent";
        s.dst = "mnt/logs/day.bin";
        s.size = 8192;
        s.written = 4096;
        s
    }

    /// 目录源（不带 `-r`）。
    fn dir_scene(tool: Tool) -> Shape {
        let mut s = Shape::blank();
        s.tag = "probe-dir";
        s.src = "logs";
        s.dst = "mnt/logs";
        s.tool = tool;
        s.src_dir = true;
        s
    }

    /// 申报逐维等于现场（做单维注入用）。
    fn claim_of(o: &Outcome) -> Claim {
        Claim { stage: o.stage, code: o.code, value: o.value, note: o.note, default: o.default }
    }

    /// 取一槽；下标恒小于 `CORPUS`，越界即测试自身写错。
    fn slot(i: usize) -> Case {
        match case_at(i) {
            Some(c) => c,
            None => panic!("语料槽 {} 不存在", i),
        }
    }

    /// 换一维注解但**不跨成败族**：跨族会连带翻掉成败位，缺陷就会被更靠前的
    /// Estate 判据抢先抓走，那样测不到文案维本身。
    fn kin_note(n: Note) -> Note {
        match n {
            Note::Complete => Note::WarnSparse,
            Note::WarnSparse => Note::CloseIgnored,
            Note::CloseIgnored => Note::Complete,
            _ => Note::CloseEnospc,
        }
    }

    // ------------------------------------------------------------------
    // 常量与错误码段
    // ------------------------------------------------------------------

    #[test]
    fn seg_base_is_assigned_and_codes_are_contiguous() {
        assert_eq!(SEG_BASE, 0x6d140);
        assert_eq!(CP_ESTAGE, SEG_BASE + 0x01);
        assert_eq!(CP_ECODE, SEG_BASE + 0x02);
        assert_eq!(CP_EVALUE, SEG_BASE + 0x03);
        assert_eq!(CP_ENOTE, SEG_BASE + 0x04);
        assert_eq!(CP_EDEFAULT, SEG_BASE + 0x05);
        assert_eq!(CP_EABORT, SEG_BASE + 0x06);
        assert_eq!(CP_ESTATE, SEG_BASE + 0x07);
        assert_eq!(CP_ECOUNT, SEG_BASE + 0x08);
        assert_eq!(CP_ESLOT, SEG_BASE + 0x09);
        assert_eq!(CP_ESEAL, SEG_BASE + 0x0a);
        assert_eq!(CP_ENOBS, SEG_BASE + 0x0b);
        assert_eq!(CP_EMATRIX, SEG_BASE + 0x0c);
        assert_eq!(CP_EBAL, SEG_BASE + 0x0d);
        assert!(name_ok());
    }

    #[test]
    fn thirteen_codes_are_pairwise_distinct() {
        let codes = [
            CP_ESTAGE, CP_ECODE, CP_EVALUE, CP_ENOTE, CP_EDEFAULT, CP_EABORT, CP_ESTATE, CP_ECOUNT,
            CP_ESLOT, CP_ESEAL, CP_ENOBS, CP_EMATRIX, CP_EBAL,
        ];
        assert_eq!(codes.len(), 13);
        let mut i = 0usize;
        while i < codes.len() {
            let a = match codes.get(i) {
                Some(v) => *v,
                None => panic!("码表下标越界"),
            };
            assert!(a > SEG_BASE);
            assert!(a <= SEG_BASE + 0x0d);
            assert_eq!(hex_text(a, 5).as_str(), match i {
                0 => "6d141",
                1 => "6d142",
                2 => "6d143",
                3 => "6d144",
                4 => "6d145",
                5 => "6d146",
                6 => "6d147",
                7 => "6d148",
                8 => "6d149",
                9 => "6d14a",
                10 => "6d14b",
                11 => "6d14c",
                _ => "6d14d",
            });
            let mut j = i + 1;
            while j < codes.len() {
                if let Some(b) = codes.get(j) {
                    assert_ne!(a, *b, "错误码 {} 与 {} 撞号", i, j);
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn scale_constants_match_the_corpus_shape() {
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(ACCEPT + REJECT, CORPUS);
        assert_eq!(SHAPES.len(), CORPUS);
        assert_eq!(CLAIMS.len(), CORPUS);
        assert_eq!(VALS.len(), ACCEPT);
        assert_eq!(REJECT_FAULTS.len(), REJECT);
        assert_eq!(REJECT_CODES.len(), REJECT);
        assert_eq!(ERRNO_SET_SIZE, 7);
        assert_eq!(SIZE_MAX, 0x0800_0000);
    }

    #[test]
    fn errno_constants_are_linux_truth_not_invention() {
        // 本卡只引进 F11681 的结论，errno 必须是 Linux 真值。
        assert_eq!(ERR_ENOSPC, 28);
        assert_eq!(ERR_EACCES, 13);
        assert_eq!(ERR_EPERM, 1);
        assert_eq!(ERR_EISDIR, 21);
        assert_eq!(ERR_EDQUOT, 122);
        assert_eq!(ERR_EIO, 5);
        assert_eq!(ERR_NONE, 0);
        assert_eq!(strerror(ERR_ENOSPC), ENOSPC_TAIL);
        assert_eq!(strerror(ERR_EACCES), "Permission denied");
        assert_eq!(strerror(ERR_EPERM), "Operation not permitted");
        assert_eq!(strerror(ERR_EISDIR), "Is a directory");
        assert_eq!(strerror(ERR_EDQUOT), "Quota exceeded");
        assert_eq!(strerror(ERR_EIO), "Input/output error");
        assert_eq!(strerror(ERR_NONE), "");
        assert_eq!(strerror(4242), "Unknown error");
    }

    #[test]
    fn exit_code_constants_are_the_three_tiers() {
        assert_eq!(EXIT_OK, 0);
        assert_eq!(EXIT_FILE, 1);
        assert_eq!(EXIT_HARD, 2);
        assert_eq!(Tool::Gnu.usage_exit(), EXIT_HARD);
        assert_eq!(Tool::Busybox.usage_exit(), EXIT_FILE);
    }

    #[test]
    fn preserve_bitmap_is_four_disjoint_bits() {
        assert_eq!(PRES_NONE, 0);
        assert_eq!(PRES_MODE, 1);
        assert_eq!(PRES_TIME, 2);
        assert_eq!(PRES_XATTR, 4);
        assert_eq!(PRES_SPARSE, 8);
        assert_eq!(PRES_ALL, 15);
        assert_eq!(PRES_P, 3);
        assert_eq!(PRES_BIT_COUNT, 4);
        assert_eq!(PRES_BITS.len(), PRES_BIT_COUNT);
        assert_eq!(pres_popcount(PRES_ALL), 4);
        assert_eq!(pres_popcount(PRES_P), 2);
        assert_eq!(pres_popcount(PRES_NONE), 0);
        // 第 5 位不在本卡的位图上：popcount 只数得着四道尾段。
        assert_eq!(pres_popcount(16), 0);
        assert!(!pres_subset(16, PRES_ALL));
        assert!(pres_subset(PRES_NONE, PRES_NONE));
    }

    #[test]
    fn op_codes_cover_seven_gates_plus_corpus_and_replay() {
        assert_eq!(OP_OPEN, 1);
        assert_eq!(OP_STAT, 2);
        assert_eq!(OP_COPY, 3);
        assert_eq!(OP_WERR, 4);
        assert_eq!(OP_CERR, 5);
        assert_eq!(OP_PRES, 6);
        assert_eq!(OP_CORPUS, 7);
        assert_eq!(OP_REPLAY, 8);
        let ops = [OP_OPEN, OP_STAT, OP_COPY, OP_WERR, OP_CERR, OP_PRES, OP_CORPUS, OP_REPLAY];
        let mut i = 0usize;
        while i < ops.len() {
            let a = match ops.get(i) {
                Some(v) => *v,
                None => panic!("操作码表越界"),
            };
            assert!(a >= OP_OPEN && a <= OP_REPLAY);
            let mut j = i + 1;
            while j < ops.len() {
                if let Some(b) = ops.get(j) {
                    assert_ne!(a, *b);
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn reject_code_table_matches_the_fault_table() {
        assert_eq!(
            REJECT_FAULTS,
            [Fault::Estate, Fault::Stage, Fault::Code, Fault::Value, Fault::Note, Fault::Default]
        );
        let mut i = 0usize;
        while i < REJECT {
            let f = match REJECT_FAULTS.get(i) {
                Some(v) => *v,
                None => panic!("缺陷表下标越界"),
            };
            let c = match REJECT_CODES.get(i) {
                Some(v) => *v,
                None => panic!("缺陷码表下标越界"),
            };
            assert_eq!(c, f.code());
            assert!(f.alive());
            assert!(!fault_short(f).is_empty());
            let mut j = i + 1;
            while j < REJECT {
                if let Some(o) = REJECT_CODES.get(j) {
                    assert_ne!(*o, c, "六槽共用了一个错误码");
                }
                if let Some(g) = REJECT_FAULTS.get(j) {
                    assert_ne!(*g, f);
                }
                j += 1;
            }
            i += 1;
        }
        // 六码全落在本段内，且不与结构码（abort/matrix/bal）混用。
        assert_eq!(REJECT_CODES, [CP_ESTATE, CP_ESTAGE, CP_ECODE, CP_EVALUE, CP_ENOTE, CP_EDEFAULT]);
    }

    #[test]
    fn vals_table_last_cell_is_the_red_rate() {
        assert_eq!(VALS, [14, 8, 6, 1, 2, 2, 3, 929]);
        assert_eq!(VALS.first(), Some(&(CORPUS as u32)));
        assert_eq!(VALS.get(1), Some(&(ACCEPT as u32)));
        assert_eq!(VALS.get(2), Some(&(REJECT as u32)));
        assert_eq!(VALS.last(), Some(&929));
        // 929 是 "注入一槽缺陷后" 的千分率：(13*1000 + 14/2) / 14。
        assert_eq!((13 * 1000 + 14 / 2) / 14, 929);
        assert_eq!(VALS.get(7), Some(&((13 * 1000 + 7) / 14)));
        assert_ne!(VALS.get(7), Some(&1000));
    }

    // ------------------------------------------------------------------
    // 数字 / 布尔 / 指纹基元
    // ------------------------------------------------------------------

    #[test]
    fn dec_text_covers_zero_to_the_widest_number() {
        assert_eq!(dec_text(0).as_str(), "0");
        assert_eq!(dec_text(1).as_str(), "1");
        assert_eq!(dec_text(9).as_str(), "9");
        assert_eq!(dec_text(10).as_str(), "10");
        assert_eq!(dec_text(999).as_str(), "999");
        assert_eq!(dec_text(1000).as_str(), "1000");
        assert_eq!(dec_text(4096).as_str(), "4096");
        assert_eq!(dec_text(929).as_str(), "929");
        assert_eq!(dec_text(100000).as_str(), "100000");
        assert_eq!(dec_text(u32::MAX).as_str(), "4294967295");
        assert_eq!(dec_text(u32::MAX).len(), 10);
        assert_eq!(dec_text(0).len(), 1);
    }

    #[test]
    fn hex_text_is_lowercase_fixed_width_and_truncating() {
        assert_eq!(hex_text(SEG_BASE, 5).as_str(), "6d140");
        assert_eq!(hex_text(SEG_BASE, 6).as_str(), "06d140");
        assert_eq!(hex_text(CP_EBAL, 5).as_str(), "6d14d");
        assert_eq!(hex_text(0xf, 2).as_str(), "0f");
        assert_eq!(hex_text(0x10, 2).as_str(), "10");
        assert_eq!(hex_text(0xabcdef, 8).as_str(), "00abcdef");
        assert_eq!(hex_text(0, 4).as_str(), "0000");
        assert_eq!(hex_text(0x6d140, 0).as_str(), "");
        assert!(hex_text(0x6d140, 0).is_empty());
        // 定宽不足时高位被舍弃（本卡只用 5/6 两档，这里钉住截断方向）。
        assert_eq!(hex_text(0x6d140, 2).as_str(), "40");
        assert!(!hex_text(0xabcdef, 6).contains('A'));
    }

    #[test]
    fn signed_and_unsigned_text_helpers_stay_in_lane() {
        assert_eq!(i32_text(ERR_NONE).as_str(), "0");
        assert_eq!(i32_text(-ERR_NONE).as_str(), "0");
        assert_eq!(i32_text(ERR_EPERM).as_str(), "1");
        assert_eq!(i32_text(ERR_ENOSPC).as_str(), "28");
        assert_eq!(i32_text(ERR_EDQUOT).as_str(), "122");
        assert_eq!(i32_text(-ERR_EDQUOT).as_str(), "-122");
        assert_eq!(i32_text(-1).as_str(), "-1");
        assert_eq!(i32_text(i32::MIN).as_str(), "-2147483648");
        assert_eq!(u32_text(929).as_str(), dec_text(929).as_str());
        assert_eq!(u32_text(0).as_str(), "0");
    }

    #[test]
    fn bool_text_is_the_audit_wording_used_by_detail() {
        // `detail()` 里的 `abort=` 字段就是它：本卡用 "true"/"false"，不是 TAP 的 ok/not ok。
        assert_eq!(bool_text(true), "true");
        assert_eq!(bool_text(false), "false");
        assert_ne!(bool_text(true), bool_text(false));
        assert_eq!(bool_text(true).len(), 4);
        assert_eq!(bool_text(false).len(), 5);
        let d = detail(&slot(3));
        assert!(d.contains(" abort=true"));
        assert!(!d.contains(" abort=ok"));
        assert!(detail(&slot(0)).contains(" abort=false"));
    }

    #[test]
    fn fnv_folds_are_stable_and_position_sensitive() {
        assert_eq!(FNV_OFF, 0xcbf2_9ce4_8422_2325);
        assert_eq!(FNV_PRIME, 0x100_0000_01b3);
        assert_eq!(fold_byte(FNV_OFF, 0), fold_byte(FNV_OFF, 0));
        assert_ne!(fold_byte(FNV_OFF, 0), fold_byte(FNV_OFF, 1));
        assert_eq!(fold_str(FNV_OFF, ""), FNV_OFF);
        assert_ne!(fold_str(FNV_OFF, "a"), fold_str(FNV_OFF, "b"));
        // 折入次序进指纹：拼接同字节、异次序必须分家。
        assert_eq!(fold_str(fold_str(FNV_OFF, "ab"), "c"), fold_str(fold_str(FNV_OFF, "a"), "bc"));
        assert_ne!(fold_str(fold_str(FNV_OFF, "ab"), "c"), fold_str(fold_str(FNV_OFF, "ac"), "b"));
        assert_ne!(fold_word(FNV_OFF, 1), fold_word(FNV_OFF, 2));
        // 大端逐字节：最高字节与最低字节各自扰动指纹。
        assert_ne!(fold_word(FNV_OFF, 0x0000_0000_0000_00ff), fold_word(FNV_OFF, 0xff00_0000_0000_0000));
        assert_eq!(fold_word(FNV_OFF, 0), fold_word(FNV_OFF, 0));
        assert_ne!(fold_i32(FNV_OFF, -1), fold_i32(FNV_OFF, 1));
        assert_eq!(fold_i32(FNV_OFF, ERR_ENOSPC), fold_word(FNV_OFF, ERR_ENOSPC as u64));
    }

    #[test]
    fn cp_prefix_is_the_argv_zero_of_both_tools() {
        assert_eq!(cp_prefix().as_str(), "cp: ");
        assert_eq!(cp_prefix().len(), 4);
        assert!(msg_gnu_error_writing("x", ERR_ENOSPC).starts_with(cp_prefix().as_str()));
        assert!(msg_bb_write_error("x", ERR_ENOSPC).starts_with(cp_prefix().as_str()));
        // 本卡自研的三句审计文案不带 `cp: `——它们不冒充任何真实工具。
        assert!(!msg_silent_truncate("x", 1, 2).starts_with("cp: "));
        assert!(!msg_close_ignored("x", 1).starts_with("cp: "));
        assert!(!msg_abort_broken("n", 1, 2).starts_with("cp: "));
    }

    // ------------------------------------------------------------------
    // 文案档：逐字长度与字节数
    // ------------------------------------------------------------------

    #[test]
    fn write_abort_wording_differs_by_one_byte_only() {
        // 同一现场、两种措辞：GNU `error writing '...'` vs busybox `write error to '...'`。
        let g = msg_gnu_error_writing("mnt/logs/day.bin", ERR_ENOSPC);
        let b = msg_bb_write_error("mnt/logs/day.bin", ERR_ENOSPC);
        assert_eq!(g.as_str(), "cp: error writing 'mnt/logs/day.bin': No space left on device");
        assert_eq!(b.as_str(), "cp: write error to 'mnt/logs/day.bin': No space left on device");
        assert_eq!(g.len(), 61);
        assert_eq!(b.len(), 62);
        assert_eq!(b.len(), g.len() + 1);
        assert_eq!(g.chars().count(), g.len());
        assert_eq!(b.chars().count(), b.len());
        assert!(g.ends_with(ENOSPC_TAIL) && b.ends_with(ENOSPC_TAIL));
        assert!(g.contains(GNU_WRITE_TAIL) && !g.contains(BB_WRITE_TAIL));
        assert!(b.contains(BB_WRITE_TAIL) && !b.contains(GNU_WRITE_TAIL));
        assert_eq!(BB_WRITE_TAIL.len() - GNU_WRITE_TAIL.len(), 1);
    }

    #[test]
    fn deferred_close_wording_is_gnu_only() {
        let c = msg_gnu_failed_to_close("mnt/db.dump", ERR_ENOSPC);
        assert_eq!(c.as_str(), "cp: failed to close 'mnt/db.dump': No space left on device");
        assert_eq!(c.len(), 58);
        assert!(c.contains(GNU_CLOSE_TAIL));
        assert!(c.ends_with(ENOSPC_TAIL));
        // busybox 没有这句：它丢弃 close 返回值，所以这句在 busybox 的输出里永不出现在 cerr 闸。
        assert!(!msg_bb_write_error("mnt/db.dump", ERR_ENOSPC).contains(GNU_CLOSE_TAIL));
        assert_eq!(
            msg_gnu_failed_to_close("x", ERR_EDQUOT).as_str(),
            "cp: failed to close 'x': Quota exceeded"
        );
    }

    #[test]
    fn create_failure_wording_splits_by_tool() {
        let g = msg_gnu_cannot_create("mnt/seed.bin", ERR_ENOSPC);
        let b = msg_bb_cannot_create("mnt/seed.bin", ERR_ENOSPC);
        assert_eq!(
            g.as_str(),
            "cp: cannot create regular file 'mnt/seed.bin': No space left on device"
        );
        assert_eq!(b.as_str(), "cp: can't create 'mnt/seed.bin': No space left on device");
        assert!(g.contains(GNU_CREATE_TAIL));
        assert!(!b.contains(GNU_CREATE_TAIL));
        assert!(b.contains("can't create '"));
        assert!(!g.contains("can't create "));
        assert_eq!(g.len(), 70);
        assert_eq!(b.len(), 56);
        assert_eq!(g.len(), b.len() + 14, "GNU 多写的是对象类型 regular file");
    }

    #[test]
    fn directory_wordings_do_not_overlap_at_all() {
        let omit = msg_gnu_omitting_directory("logs");
        let isdir = msg_bb_is_a_directory("logs");
        assert_eq!(omit.as_str(), "cp: -r not specified; omitting directory 'logs'");
        assert_eq!(isdir.as_str(), "cp: 'logs': Is a directory");
        assert!(omit.starts_with(OMIT_HEAD));
        assert!(omit.ends_with("'logs'"));
        assert!(!omit.contains("Is a directory"));
        assert!(!isdir.contains("-r not specified"));
        assert_eq!(isdir.len(), 26);
        assert_eq!(omit.len(), 47);
        assert!(omit.len() > isdir.len(), "GNU 那句长，是因为它把缺的开关也写进句子");
    }

    #[test]
    fn usage_wording_keeps_the_same_subject_and_splits_the_punctuation() {
        let g = msg_gnu_unrecognized_option("--preserve=xattr");
        let b = msg_bb_unrecognized_option("--preserve=xattr");
        assert_eq!(g.as_str(), "cp: unrecognized option '--preserve=xattr'");
        assert_eq!(b.as_str(), "cp: unrecognized option: --preserve=xattr");
        assert!(g.contains('\''));
        assert!(!b.contains('\''));
        assert!(b.contains("option: "));
        assert!(!g.contains("option: "));
        assert_eq!(g.len(), 42);
        assert_eq!(b.len(), 41);
        assert_eq!(g.len(), b.len() + 1, "两句只差那对引号与冒号的替换");
        assert_eq!(g.matches('\'').count(), 2);
        assert_eq!(b.matches('\'').count(), 0);
    }

    #[test]
    fn sparse_warning_is_one_sentence_naming_the_mount() {
        let w = msg_gnu_sparse_warning("/mnt/full");
        assert_eq!(
            w.as_str(),
            "cp: warning: behavior of cp is dependent on the file system's sparse support, \
but it is not enabled for '/mnt/full', file system does not support sparse files"
        );
        assert!(w.starts_with(SPARSE_HEAD));
        assert!(w.contains("'/mnt/full'"));
        assert!(w.ends_with("does not support sparse files"));
        assert!(!w.contains(ENOSPC_TAIL));
        assert_eq!(w.matches('\'').count(), 3, "两处包路径，一处是 file system's 的撇号");
        assert_eq!(w.len(), 158);
        assert!(!w.contains("': "));
        assert_eq!(w.lines().count(), 1);
    }

    #[test]
    fn tail_section_wordings_are_four_distinct_sentences() {
        assert_eq!(
            msg_gnu_preserving_times("mnt/cfg.conf", ERR_EPERM).as_str(),
            "cp: preserving times for 'mnt/cfg.conf': Operation not permitted"
        );
        assert_eq!(
            msg_gnu_preserving_permissions("mnt/cfg.conf", ERR_EPERM).as_str(),
            "cp: preserving permissions for 'mnt/cfg.conf': Operation not permitted"
        );
        assert_eq!(
            msg_gnu_failed_preserve_ownership("mnt/cfg.conf", ERR_EPERM).as_str(),
            "cp: failed to preserve ownership for 'mnt/cfg.conf': Operation not permitted"
        );
        // 这句逐字不带路径、不带 strerror 尾巴。
        assert_eq!(msg_gnu_failed_copy_xattrs().as_str(), "cp: failed to copy xattrs");
        assert!(!msg_gnu_failed_copy_xattrs().contains('\''));
        // 这句也不带 strerror 尾巴（它不是 errno 归因，是能力缺失）。
        assert_eq!(
            msg_gnu_cannot_preserve_sparse("mnt/cfg.conf").as_str(),
            "cp: cannot preserve sparse file state for 'mnt/cfg.conf'"
        );
        assert!(!msg_gnu_cannot_preserve_sparse("x").ends_with(ENOSPC_TAIL));
        // busybox 的尾段只有 chmod 一句。
        assert_eq!(
            msg_bb_cannot_chmod("mnt/cfg.conf", ERR_EPERM).as_str(),
            "cp: can't chmod 'mnt/cfg.conf': Operation not permitted"
        );
    }

    #[test]
    fn open_and_stat_source_side_wordings() {
        assert_eq!(
            msg_gnu_cannot_open("a.bin", ERR_EACCES).as_str(),
            "cp: cannot open 'a.bin' for reading: Permission denied"
        );
        assert_eq!(
            msg_gnu_cannot_stat("a.bin", ERR_EIO).as_str(),
            "cp: cannot stat 'a.bin': Input/output error"
        );
        assert_eq!(
            msg_bb_cannot_open("a.bin", ERR_EACCES).as_str(),
            "cp: can't open 'a.bin': Permission denied"
        );
        assert_eq!(
            msg_bb_cannot_stat("a.bin", ERR_EIO).as_str(),
            "cp: can't stat 'a.bin': Input/output error"
        );
        // GNU 的 "for reading" 短语是 busybox 没有的落点信息。
        assert!(msg_gnu_cannot_open("x", ERR_NONE).contains("for reading"));
        assert!(!msg_bb_cannot_open("x", ERR_NONE).contains("for reading"));
        assert_eq!(msg_gnu_cannot_open("x", ERR_NONE).as_str(), "cp: cannot open 'x' for reading: ");
    }

    #[test]
    fn audit_only_wordings_state_the_missing_bytes() {
        assert_eq!(
            msg_silent_truncate("mnt/logs/day.bin", 4096, 8192).as_str(),
            "silent truncation at 'mnt/logs/day.bin' (4096 of 8192 bytes, no error reported)"
        );
        assert_eq!(
            msg_close_ignored("mnt/notes.txt", 4096).as_str(),
            "close error dropped by busybox for 'mnt/notes.txt' \
(4096 bytes of deferred blocks unaccounted)"
        );
        assert_eq!(
            msg_abort_broken("abort-mismatch", 8192, 1).as_str(),
            "abort attribution broken (note=abort-mismatch, bytes=8192, exit=1)"
        );
        assert_eq!(
            msg_foreign_stop("b.bin", ERR_EIO).as_str(),
            "cp: error writing 'b.bin': Input/output error"
        );
        // 零字节的两种情形不许混淆：静默截断说 "0 of N"，丢弃 close 说 "0 bytes"。
        assert_eq!(
            msg_silent_truncate("b", 0, 8192).as_str(),
            "silent truncation at 'b' (0 of 8192 bytes, no error reported)"
        );
        assert!(msg_close_ignored("b", 0).ends_with("(0 bytes of deferred blocks unaccounted)"));
    }

    // ------------------------------------------------------------------
    // 枚举：工具面 / 模式 / 阶段 / 注解 / 档位 / 缺陷
    // ------------------------------------------------------------------

    #[test]
    fn tool_face_differences_all_point_the_same_way() {
        assert_eq!(Tool::Gnu.text(), "gnu-cp");
        assert_eq!(Tool::Busybox.text(), "busybox-cp");
        assert_ne!(Tool::Gnu.text(), Tool::Busybox.text());
        assert!(Tool::Gnu.reports_close_error());
        assert!(!Tool::Busybox.reports_close_error());
        assert!(Tool::Gnu.supports_sparse_flag());
        assert!(!Tool::Busybox.supports_sparse_flag());
        assert!(Tool::Gnu.supports_xattr_preserve());
        assert!(!Tool::Busybox.supports_xattr_preserve());
        assert!(Tool::Gnu.diagnoses_dir_at_stat());
        assert!(!Tool::Busybox.diagnoses_dir_at_stat());
        assert!(!Tool::Gnu.tail_is_chmod_only());
        assert!(Tool::Busybox.tail_is_chmod_only());
        assert_eq!(Tool::Gnu.usage_exit(), 2);
        assert_eq!(Tool::Busybox.usage_exit(), 1);
        assert_eq!(Tool::Gnu.preserve_ceiling(), PRES_ALL);
        assert_eq!(Tool::Busybox.preserve_ceiling(), PRES_P);
        assert!(pres_subset(Tool::Busybox.preserve_ceiling(), Tool::Gnu.preserve_ceiling()));
        assert!(enums_ok());
    }

    #[test]
    fn copy_mode_semantics_map_to_the_documented_flags() {
        assert_eq!(CopyMode::Plain.name(), "plain");
        assert_eq!(CopyMode::Recursive.name(), "recursive");
        assert_eq!(CopyMode::Preserve.name(), "preserve");
        assert_eq!(CopyMode::Archive.name(), "archive");
        assert_eq!(CopyMode::Sparse.name(), "sparse");
        assert!(!CopyMode::Plain.recursive());
        assert!(CopyMode::Recursive.recursive());
        assert!(!CopyMode::Preserve.recursive());
        assert!(CopyMode::Archive.recursive());
        assert!(!CopyMode::Sparse.recursive());
        assert!(!CopyMode::Plain.uses_sparse());
        assert!(!CopyMode::Recursive.uses_sparse());
        assert!(CopyMode::Sparse.uses_sparse());
        assert!(CopyMode::Archive.uses_sparse());
        assert_eq!(CopyMode::Plain.preserve_bits(), PRES_NONE);
        assert_eq!(CopyMode::Recursive.preserve_bits(), PRES_NONE);
        assert_eq!(CopyMode::Preserve.preserve_bits(), PRES_P);
        assert_eq!(CopyMode::Archive.preserve_bits(), PRES_ALL);
        assert_eq!(CopyMode::Sparse.preserve_bits(), PRES_SPARSE);
        // 只有 -a 同时是递归与稀疏感知；-r 不保属性，-p 不递归。
        assert!(CopyMode::Archive.recursive() && CopyMode::Archive.uses_sparse());
        assert!(!CopyMode::Preserve.uses_sparse());
    }

    #[test]
    fn stage_line_is_seven_and_the_planes_partition_it() {
        assert_eq!(GATE_LINE.len(), 7);
        assert_eq!(Stage::Open.order(), 0);
        assert_eq!(Stage::Exit.order(), 6);
        let mut i = 0usize;
        while i < GATE_LINE.len() {
            let s = match GATE_LINE.get(i) {
                Some(v) => *v,
                None => panic!("闸序表越界"),
            };
            assert_eq!(s.order() as usize, i);
            assert_eq!(s.failure(), i + 1 < GATE_LINE.len());
            assert_eq!(s.data_plane(), i == 0 || i == 2 || i == 3 || i == 4);
            assert_eq!(s.abort_plane(), i == 0 || i == 3 || i == 4);
            assert!(s.name().len() <= 6);
            // 中止族闸必是数据面子集：只有 open/werr/cerr 能把 ENOSPC 记成中止。
            if s.abort_plane() {
                assert!(s.data_plane());
            }
            let mut j = i + 1;
            while j < GATE_LINE.len() {
                if let Some(o) = GATE_LINE.get(j) {
                    assert_ne!(o.name(), s.name());
                    assert_ne!(o.order(), s.order());
                }
                j += 1;
            }
            i += 1;
        }
        assert_eq!(Stage::Open.name(), "open");
        assert_eq!(Stage::ClErr.name(), "cerr");
        assert_eq!(Stage::Werr.name(), "werr");
        assert_eq!(Stage::Pres.name(), "pres");
        assert!(!Stage::Exit.failure());
        assert!(!Stage::Stat.data_plane());
        assert!(!Stage::Copy.abort_plane());
    }

    #[test]
    fn note_families_are_a_disjoint_cover_of_all_seventeen() {
        let notes = [
            Note::Complete,
            Note::WarnSparse,
            Note::CloseIgnored,
            Note::CreateFail,
            Note::BadOption,
            Note::OmitDir,
            Note::IsDir,
            Note::SilentTrunc,
            Note::ErrorWriting,
            Note::WriteError,
            Note::CloseEnospc,
            Note::PresMode,
            Note::PresTime,
            Note::PresXattr,
            Note::PresSparse,
            Note::ForeignErr,
            Note::AbortMismatch,
        ];
        assert_eq!(notes.len(), 17);
        let mut i = 0usize;
        while i < notes.len() {
            let a = match notes.get(i) {
                Some(v) => *v,
                None => panic!("注解表越界"),
            };
            let fam = a.success() as u8
                + a.abort_family() as u8
                + a.tail_family() as u8
                + a.prelude_family() as u8
                + a.defect_family() as u8;
            assert_eq!(fam, 1, "注解 {} 落进 {} 个族", a.name(), fam);
            assert_eq!(a.noisy_success(), a == Note::WarnSparse || a == Note::CloseIgnored);
            if a.noisy_success() {
                assert!(a.success());
            }
            if a.tail_family() {
                assert_eq!(pres_popcount(a.preserve_bit()), 1);
            } else {
                assert_eq!(a.preserve_bit(), PRES_NONE);
            }
            let mut j = i + 1;
            while j < notes.len() {
                if let Some(b) = notes.get(j) {
                    assert_ne!(a.name(), b.name());
                }
                j += 1;
            }
            i += 1;
        }
        assert_eq!(Note::ErrorWriting.name(), "error-writing");
        assert_eq!(Note::WriteError.name(), "write-error");
        assert_eq!(Note::CloseEnospc.name(), "failed-to-close");
        assert_eq!(Note::CloseIgnored.name(), "close-ignored");
        assert_eq!(Note::SilentTrunc.name(), "silent-trunc");
        assert_eq!(Note::AbortMismatch.name(), "abort-mismatch");
        assert!(Note::CreateFail.abort_family());
        assert!(!Note::SilentTrunc.abort_family());
        assert!(Note::SilentTrunc.defect_family());
        assert!(Note::ForeignErr.defect_family());
        assert!(Note::AbortMismatch.defect_family());
        assert!(Note::OmitDir.prelude_family() && Note::IsDir.prelude_family());
        assert!(!Note::CreateFail.prelude_family());
        assert_eq!(Note::PresMode.preserve_bit(), PRES_MODE);
        assert_eq!(Note::PresTime.preserve_bit(), PRES_TIME);
        assert_eq!(Note::PresXattr.preserve_bit(), PRES_XATTR);
        assert_eq!(Note::PresSparse.preserve_bit(), PRES_SPARSE);
        assert!(!Note::Complete.abort_family());
        assert!(Note::Complete.success());
    }

    #[test]
    fn tier_projection_puts_the_busybox_drop_in_warned() {
        let aborted = Outcome {
            stage: Stage::Werr,
            code: EXIT_FILE,
            value: 4096,
            note: Note::ErrorWriting,
            default: true,
        };
        assert_eq!(Tier::of(&aborted), Tier::Aborted);
        assert!(aborted.default && Tier::of(&aborted).aborted());
        // 归因位压过成败投影：码 0 也说中止。
        let weird = Outcome {
            stage: Stage::Exit,
            code: EXIT_OK,
            value: 0,
            note: Note::Complete,
            default: true,
        };
        assert_eq!(Tier::of(&weird), Tier::Aborted);
        assert_eq!(Tier::of(&Outcome::blank()), Tier::Complete);
        let warn = Outcome {
            stage: Stage::Exit,
            code: EXIT_OK,
            value: 8192,
            note: Note::WarnSparse,
            default: false,
        };
        assert_eq!(Tier::of(&warn), Tier::Warned);
        let dropped = Outcome {
            stage: Stage::Exit,
            code: EXIT_OK,
            value: 4096,
            note: Note::CloseIgnored,
            default: false,
        };
        assert_eq!(Tier::of(&dropped), Tier::Warned);
        let tail = Outcome {
            stage: Stage::Pres,
            code: EXIT_FILE,
            value: 2048,
            note: Note::PresTime,
            default: false,
        };
        assert_eq!(Tier::of(&tail), Tier::Tail);
        let foreign = Outcome {
            stage: Stage::Werr,
            code: EXIT_FILE,
            value: 4096,
            note: Note::ForeignErr,
            default: false,
        };
        assert_eq!(Tier::of(&foreign), Tier::Tail);
        assert_eq!(Tier::Complete.index(), 0);
        assert_eq!(Tier::Aborted.index(), TIER_COUNT - 1);
        assert!(Tier::Complete.success() && Tier::Warned.success());
        assert!(!Tier::Tail.success() && !Tier::Aborted.success());
        assert!(!Tier::Complete.aborted());
        assert_eq!(Tier::Warned.name(), "warned");
        assert_eq!(Tier::Tail.name(), "tail");
    }

    #[test]
    fn fault_codes_are_one_per_variant() {
        assert_eq!(Fault::Ok.code(), 0);
        assert_eq!(Fault::Stage.code(), CP_ESTAGE);
        assert_eq!(Fault::Code.code(), CP_ECODE);
        assert_eq!(Fault::Value.code(), CP_EVALUE);
        assert_eq!(Fault::Note.code(), CP_ENOTE);
        assert_eq!(Fault::Default.code(), CP_EDEFAULT);
        assert_eq!(Fault::Abort.code(), CP_EABORT);
        assert_eq!(Fault::Estate.code(), CP_ESTATE);
        assert_eq!(Fault::Count.code(), CP_ECOUNT);
        assert_eq!(Fault::Slot.code(), CP_ESLOT);
        assert_eq!(Fault::Seal.code(), CP_ESEAL);
        assert_eq!(Fault::Nobs.code(), CP_ENOBS);
        assert_eq!(Fault::Matrix.code(), CP_EMATRIX);
        assert_eq!(Fault::Bal.code(), CP_EBAL);
        assert!(!Fault::Ok.alive());
        assert!(Fault::Abort.alive());
        assert_eq!(fault_short(Fault::Ok), "ok");
        assert_eq!(fault_short(Fault::Estate), "estate");
        assert_eq!(fault_short(Fault::Bal), "bal");
        assert_eq!(fault_short(Fault::Nobs), "nobs");
        // 十四缺陷对应十三个码：只有 Ok 占 0，其余一维一码。
        let faults = [
            Fault::Ok, Fault::Stage, Fault::Code, Fault::Value, Fault::Note, Fault::Default,
            Fault::Abort, Fault::Estate, Fault::Count, Fault::Slot, Fault::Seal, Fault::Nobs,
            Fault::Matrix, Fault::Bal,
        ];
        assert_eq!(faults.len(), 14);
        let mut i = 0usize;
        while i < faults.len() {
            let a = match faults.get(i) {
                Some(v) => *v,
                None => panic!("缺陷枚举表越界"),
            };
            assert_eq!(a.alive(), a != Fault::Ok);
            assert!(!fault_short(a).is_empty());
            let mut j = i + 1;
            while j < faults.len() {
                if let Some(b) = faults.get(j) {
                    assert_ne!(a.code(), b.code());
                    assert_ne!(a, *b);
                }
                j += 1;
            }
            i += 1;
        }
        assert!(enums_ok());
    }

    #[test]
    fn pres_helpers_name_every_bit_and_reject_the_rest() {
        assert_eq!(pres_name(PRES_MODE), "mode");
        assert_eq!(pres_name(PRES_TIME), "mtime");
        assert_eq!(pres_name(PRES_XATTR), "xattr");
        assert_eq!(pres_name(PRES_SPARSE), "sparse");
        assert_eq!(pres_name(PRES_NONE), "none");
        assert_eq!(pres_name(16), "none");
        assert_eq!(pres_name(3), "none");
        assert!(pres_subset(PRES_P, PRES_ALL));
        assert!(!pres_subset(PRES_ALL, PRES_P));
        assert!(pres_subset(PRES_XATTR, PRES_XATTR));
        assert!(!pres_subset(PRES_XATTR, PRES_P));
        assert_eq!(pres_popcount(PRES_MODE | PRES_SPARSE), 2);
        let mut i = 0usize;
        while i < PRES_BITS.len() {
            if let Some(b) = PRES_BITS.get(i) {
                assert_eq!(pres_popcount(*b), 1);
                assert_ne!(pres_name(*b), "none");
            }
            i += 1;
        }
    }

    // ------------------------------------------------------------------
    // 挂载画像与稀疏几何
    // ------------------------------------------------------------------

    #[test]
    fn profiles_are_the_three_documented_mounts() {
        let ps = profiles();
        assert_eq!(ps.len(), 3);
        let lazy = match ps.get(0) {
            Some(v) => *v,
            None => panic!("画像表首位缺失"),
        };
        let flat = match ps.get(1) {
            Some(v) => *v,
            None => panic!("画像表中位缺失"),
        };
        let half = match ps.get(2) {
            Some(v) => *v,
            None => panic!("画像表末位缺失"),
        };
        assert_eq!(lazy.mount, "/mnt/full");
        assert_eq!(flat.mount, "/mnt/loop");
        assert_eq!(half.mount, "/mnt/tmpfs");
        assert!(lazy.well_formed() && flat.well_formed() && half.well_formed());
        assert!(lazy.is_full() && flat.is_full() && !half.is_full());
        assert_eq!(lazy.free_bytes(), Some(0));
        assert_eq!(half.free_bytes(), Some(4096));
        assert!(lazy.close_can_fail() && !flat.close_can_fail());
        assert!(lazy.can_skip_holes() && !flat.can_skip_holes() && !half.can_skip_holes());
        assert!(lazy.xattr_ok && flat.xattr_ok && !half.xattr_ok);
        assert_ne!(lazy.seal(), flat.seal());
        assert_ne!(half.seal(), flat.seal());
        assert_eq!(lazy.seal(), lazy.seal());
        assert!(profile_ok());
    }

    #[test]
    fn blank_and_broken_profiles_are_always_refused() {
        let b = FsProfile::blank();
        assert!(!b.well_formed());
        assert_eq!(b.free_bytes(), Some(0));
        assert!(b.is_full());
        assert!(!b.can_skip_holes());
        assert!(!b.close_can_fail());
        let mut p = FsProfile {
            mount: "/mnt/full",
            bsize: 4096,
            blocks_free: 0,
            delalloc: true,
            sparse_ok: true,
            xattr_ok: true,
        };
        assert!(p.well_formed());
        p.mount = "";
        assert!(!p.well_formed());
        p.mount = "/mnt/full";
        p.bsize = 4095;
        assert!(!p.well_formed());
        p.bsize = 512;
        assert!(p.well_formed());
        p.bsize = 1024;
        assert!(p.well_formed());
        p.blocks_free = 0x0800_0001;
        assert!(!p.well_formed());
        p.blocks_free = 0x0800_0000;
        // 块数本身在上界内，但乘法溢出 → 坏画像。
        assert_eq!(p.free_bytes(), None);
        assert!(!p.well_formed());
    }

    #[test]
    fn holes_bookkeeping_keeps_spread_and_stored_apart() {
        let skip = Holes { apparent: 8192, hole: 4096, skip_ok: true };
        let write = Holes { apparent: 8192, hole: 4096, skip_ok: false };
        assert_eq!(skip.spread(), 4096);
        assert_eq!(write.spread(), 8192);
        assert_eq!(skip.holes_spread(), 0);
        assert_eq!(write.holes_spread(), 4096);
        // 两个数不许互相顶替：能跳洞时"实占"少于"表观"，不能跳时展开成零块。
        assert_ne!(skip.spread(), write.spread());
        assert_eq!(skip.spread() + skip.holes_spread(), 4096);
        assert_eq!(write.spread(), write.apparent);
        assert!(skip.balanced() && write.balanced());
        let bad = Holes { apparent: 1024, hole: 2048, skip_ok: true };
        assert!(!bad.balanced());
        assert_eq!(bad.spread(), 0);
        let all = Holes { apparent: 4096, hole: 4096, skip_ok: true };
        assert!(all.balanced());
        assert_eq!(all.spread(), 0);
        let b = Holes::blank();
        assert_eq!(b.apparent, 0);
        assert_eq!(b.hole, 0);
        assert_eq!(b.spread(), 0);
        assert!(b.balanced());
        assert_ne!(skip.seal(), write.seal());
        assert_eq!(skip.seal(), skip.seal());
        assert!(holes_ok());
    }

    // ------------------------------------------------------------------
    // 形状：合法域、指纹、稀疏与延迟
    // ------------------------------------------------------------------

    #[test]
    fn blank_shape_is_valid_and_completes_cleanly() {
        let s = Shape::blank();
        assert_eq!(s.tag, "");
        assert_eq!(s.src, "a.bin");
        assert_eq!(s.dst, "b.bin");
        assert_eq!(s.tool, Tool::Gnu);
        assert_eq!(s.mode, CopyMode::Plain);
        assert_eq!(s.errno_seen, ERR_NONE);
        assert_eq!(s.preserved, PRES_NONE);
        assert!(s.create_ok);
        assert!(!s.write_err);
        assert!(!s.close_err);
        assert!(s.valid());
        let o = define(&s);
        assert_eq!(o.stage, Stage::Exit);
        assert_eq!(o.code, EXIT_OK);
        assert_eq!(o.note, Note::Complete);
        assert!(o.ok());
        assert_eq!(message(&s, &o).as_str(), "");
        assert_eq!(s.payload(), 0);
        assert_eq!(s.deferred_lost(), 0);
        assert!(!s.sparse_warning());
        assert!(s.errno_legal());
    }

    #[test]
    fn shape_valid_domain_refjects_incoherent_combos() {
        let mut g = Shape::blank();
        g.tag = "probe";
        g.size = 8192;
        g.written = 8192;
        assert!(g.valid());
        // busybox 没有 `-S`。
        let mut bb = g.clone_tool(Tool::Busybox);
        bb.mode = CopyMode::Sparse;
        bb.preserved = PRES_SPARSE;
        assert!(!bb.valid());
        // busybox 的保留上限是 mode|mtime。
        let mut bx = bb;
        bx.mode = CopyMode::Archive;
        bx.preserved = PRES_ALL;
        assert!(!bx.valid());
        bx.preserved = PRES_P;
        assert!(bx.valid());
        // 超出模式声明的保留要求。
        let mut gp = g;
        gp.preserved = PRES_TIME;
        assert!(!gp.valid());
        // 洞大于表观尺寸。
        let mut hd = g;
        hd.holes = 9000;
        assert!(!hd.valid());
        assert!(!hd.geometry().balanced());
        // 写入字节超过 payload。
        let mut sp = g;
        sp.mode = CopyMode::Sparse;
        sp.fs_sparse = true;
        sp.holes = 4096;
        assert_eq!(sp.payload(), 4096);
        assert!(!sp.valid());
        sp.written = 4096;
        assert!(sp.valid());
        // 非法 errno 不进判据。
        assert!(g.clone_errno(ERR_ENOSPC).valid());
        assert!(!g.clone_errno(4242).valid());
        assert!(!g.clone_errno(-1).valid());
        assert!(!g.clone_errno(2).valid());
        // 遍历标志只在递归模式下成立。
        let mut tr = g;
        tr.traversal = true;
        assert!(!tr.valid());
        tr.mode = CopyMode::Recursive;
        assert!(tr.valid());
        // 用法错与字节/错误标记互斥。
        let mut bo = g;
        bo.bad_option = true;
        assert!(!bo.valid());
        bo.written = 0;
        assert!(bo.valid());
        // 建件失败：字节 0、errno 非 0、不得再带循环错误。
        let mut cf = g.clone_written(0);
        cf.create_ok = false;
        assert!(!cf.valid());
        cf.errno_seen = ERR_ENOSPC;
        assert!(cf.valid());
        cf.write_err = true;
        assert!(!cf.valid());
        cf.write_err = false;
        cf.close_err = true;
        assert!(!cf.valid());
        // 写错与 close 错不得同时成立。
        let mut both = g.clone_written(4096);
        both.write_err = true;
        both.errno_seen = ERR_ENOSPC;
        both.close_err = true;
        assert!(!both.valid());
        both.close_err = false;
        assert!(both.valid());
        // 尺寸上界。
        let mut big = g;
        big.size = SIZE_MAX + 1;
        assert!(!big.valid());
        assert!(!g.clone_written(SIZE_MAX + 1).valid());
        // 尾段失败要求 errno 有据、且是保留要求的子集。
        let mut pf = g;
        pf.mode = CopyMode::Archive;
        pf.preserved = PRES_ALL;
        pf.pres_failed = PRES_MODE;
        assert!(!pf.valid());
        pf.errno_seen = ERR_EPERM;
        assert!(pf.valid());
        pf.pres_failed = PRES_MODE | 16;
        assert!(!pf.valid());
        assert!(shape_ok());
    }

    #[test]
    fn shape_clone_helpers_move_exactly_one_dim() {
        let base = Shape {
            tag: "clone-src",
            src: "a",
            dst: "b",
            size: 1024,
            written: 512,
            ..Shape::blank()
        };
        let w = base.clone_written(1024);
        assert_eq!(w.written, 1024);
        assert_eq!(w.size, base.size);
        assert_eq!(w.tag, base.tag);
        assert_eq!(w.dst, base.dst);
        assert_eq!(w.errno_seen, base.errno_seen);
        let e = base.clone_errno(ERR_EDQUOT);
        assert_eq!(e.errno_seen, ERR_EDQUOT);
        assert_eq!(e.written, base.written);
        assert_eq!(e.mode, base.mode);
        let t = base.clone_tool(Tool::Busybox);
        assert_eq!(t.tool, Tool::Busybox);
        assert_eq!(t.mode, base.mode);
        assert_eq!(t.size, base.size);
        assert_eq!(t.errno_seen, base.errno_seen);
        // clone_written 不动 errno，clone_errno 不动 written。
        assert_eq!(e.written, w.written - 512);
        assert_ne!(w, base);
        assert_ne!(e, base);
        assert_ne!(t, base);
    }

    #[test]
    fn shape_seal_is_sensitive_to_every_deciding_dim() {
        let base = Shape { tag: "seal", src: "a", dst: "b", size: 1024, written: 512, ..Shape::blank() };
        let s0 = base.seal();
        assert_eq!(s0, base.seal());
        let dims: [Shape; 19] = {
            let mut v = [base; 19];
            if let Some(x) = v.get_mut(0) {
                x.tag = "other-tag";
            }
            if let Some(x) = v.get_mut(1) {
                x.src = "z";
            }
            if let Some(x) = v.get_mut(2) {
                x.dst = "z";
            }
            if let Some(x) = v.get_mut(3) {
                x.tool = Tool::Busybox;
            }
            if let Some(x) = v.get_mut(4) {
                x.mode = CopyMode::Preserve;
            }
            if let Some(x) = v.get_mut(5) {
                x.size = 1025;
            }
            if let Some(x) = v.get_mut(6) {
                x.written = 513;
            }
            if let Some(x) = v.get_mut(7) {
                x.holes = 1;
            }
            if let Some(x) = v.get_mut(8) {
                x.fs_sparse = false;
            }
            if let Some(x) = v.get_mut(9) {
                x.delalloc = true;
            }
            if let Some(x) = v.get_mut(10) {
                x.src_dir = true;
            }
            if let Some(x) = v.get_mut(11) {
                x.bad_option = true;
            }
            if let Some(x) = v.get_mut(12) {
                x.create_ok = false;
            }
            if let Some(x) = v.get_mut(13) {
                x.write_err = true;
            }
            if let Some(x) = v.get_mut(14) {
                x.close_err = true;
            }
            if let Some(x) = v.get_mut(15) {
                x.errno_seen = ERR_ENOSPC;
            }
            if let Some(x) = v.get_mut(16) {
                x.preserved = PRES_MODE;
            }
            if let Some(x) = v.get_mut(17) {
                x.pres_failed = PRES_TIME;
            }
            if let Some(x) = v.get_mut(18) {
                x.traversal = true;
            }
            v
        };
        let mut i = 0usize;
        while i < dims.len() {
            let s = match dims.get(i) {
                Some(v) => *v,
                None => panic!("维度扰动表越界"),
            };
            assert_ne!(s.seal(), s0, "第 {} 维改动没进指纹", i);
            i += 1;
        }
        // 有符号 errno 与同值的无符号数分家（fold_i32 走 i64 展宽）。
        let neg = base.clone_errno(-1);
        let pos = base.clone_errno(1);
        assert_ne!(neg.seal(), pos.seal());
    }

    #[test]
    fn sparse_geometry_does_not_borrow_the_write_counter() {
        // 目的端能跳洞：payload = 表观 - 洞；不能跳洞：洞也占额度。
        let mut s = Shape::blank();
        s.tag = "geom";
        s.mode = CopyMode::Sparse;
        s.preserved = PRES_SPARSE;
        s.size = 8192;
        s.holes = 4096;
        s.fs_sparse = true;
        s.written = 4096;
        assert_eq!(s.payload(), 4096);
        let geo = s.geometry();
        assert_eq!(geo.apparent, 8192);
        assert_eq!(geo.hole, 4096);
        assert!(geo.skip_ok);
        assert_eq!(geo.spread(), s.payload());
        assert_eq!(geo.holes_spread(), 0);
        assert!(!s.sparse_warning());
        s.fs_sparse = false;
        assert_eq!(s.payload(), 8192);
        assert!(!s.geometry().skip_ok);
        assert_eq!(s.geometry().spread(), 8192);
        assert_eq!(s.geometry().holes_spread(), 4096);
        assert!(s.sparse_warning());
        // 非稀疏模式一律按表观尺寸算，洞不参与减免。
        let mut plain = s;
        plain.mode = CopyMode::Plain;
        plain.preserved = PRES_NONE;
        plain.written = 8192;
        assert_eq!(plain.payload(), 8192);
        assert!(!plain.sparse_warning());
        assert!(!plain.geometry().skip_ok);
    }

    #[test]
    fn sparse_warning_needs_gnu_holes_and_a_hostile_fs() {
        assert!(warn_scene(Tool::Gnu).sparse_warning());
        // busybox 无 `-S`：该组合根本是不可判形状。
        assert!(!warn_scene(Tool::Busybox).sparse_warning());
        assert!(!warn_scene(Tool::Busybox).valid());
        // 没有洞就没有那句告警。
        let mut none = warn_scene(Tool::Gnu);
        none.holes = 0;
        none.written = 8192;
        assert!(!none.sparse_warning());
        // 目的端支持稀疏时洞被真跳过，也不告警。
        let mut okfs = warn_scene(Tool::Gnu);
        okfs.fs_sparse = true;
        okfs.written = 4096;
        assert!(!okfs.sparse_warning());
        assert!(okfs.valid());
        let o = define(&okfs);
        assert_eq!(o.note, Note::Complete);
        assert_eq!(o.code, EXIT_OK);
    }

    #[test]
    fn deferred_lost_is_only_for_the_dropped_close() {
        let gnu = close_scene(Tool::Gnu);
        let bb = close_scene(Tool::Busybox);
        assert_eq!(gnu.deferred_lost(), 0);
        assert_eq!(bb.deferred_lost(), 8192);
        // 没有延迟分配就没有"迟到爆"的额度。
        let mut flat = bb;
        flat.delalloc = false;
        assert_eq!(flat.deferred_lost(), 0);
        // close 没红就不谈丢失。
        let mut clean = bb;
        clean.close_err = false;
        assert_eq!(clean.deferred_lost(), 0);
        // 半件留档时未记账的就是已落的那部分。
        let partial = bb.clone_written(1024);
        assert_eq!(partial.deferred_lost(), 1024);
        assert_eq!(Shape::blank().deferred_lost(), 0);
    }

    #[test]
    fn usage_code_is_decided_by_the_tool_alone() {
        let g = usage_scene(Tool::Gnu);
        let b = usage_scene(Tool::Busybox);
        assert_eq!(g.code_of_usage(), EXIT_HARD);
        assert_eq!(b.code_of_usage(), EXIT_FILE);
        assert_eq!(g.code_of_usage(), define(&g).code);
        assert_eq!(b.code_of_usage(), define(&b).code);
        // 与现场字节/errno 无关：把满盘_errno 贴到用法错形状上，码不许动。
        let mut mix = g.clone_errno(ERR_ENOSPC);
        mix.size = 8192;
        assert_eq!(mix.code_of_usage(), EXIT_HARD);
        assert_eq!(define(&mix).code, EXIT_HARD);
        assert_eq!(define(&b.clone_errno(ERR_EDQUOT)).code, EXIT_FILE);
    }

    #[test]
    fn outcome_helpers_read_the_six_dims() {
        let ok = Outcome::blank();
        assert!(ok.ok());
        assert!(!ok.failed());
        assert_eq!(ok.stage, Stage::Exit);
        assert_eq!(ok.code, EXIT_OK);
        assert_eq!(ok.note, Note::Complete);
        assert!(!ok.default);
        assert!(!ok.attributed());
        // 码 0 但注解属错误族：不叫成功。
        let lie = Outcome {
            stage: Stage::Exit,
            code: EXIT_OK,
            value: 0,
            note: Note::ErrorWriting,
            default: false,
        };
        assert!(!lie.ok());
        assert!(lie.failed());
        // 码非 0 但注解属成功族：本卡不认它成功（close 被吞的那格由工具定，不由申报定）。
        let mixed = Outcome {
            stage: Stage::Exit,
            code: EXIT_FILE,
            value: 0,
            note: Note::Complete,
            default: false,
        };
        assert!(!mixed.ok());
        let s = write_scene(Tool::Gnu);
        let o = define(&s);
        assert_eq!(o.shortfall(&s), 4096);
        assert_eq!(o.value, 4096);
        assert!(o.attributed());
        assert_eq!(o.seal(), o.seal());
        assert_ne!(o.seal(), define(&close_scene(Tool::Gnu)).seal());
        // 超额留档的亏空饱和在 0，不许回绕成大数。
        let over = Outcome { value: 9999, ..Outcome::blank() };
        assert_eq!(over.shortfall(&s), 0);
        assert_eq!(over.shortfall(&Shape { size: 20000, written: 1, ..Shape::blank() }), 20000 - 9999);
    }

    #[test]
    fn abort_triple_rules_are_note_specific() {
        // 写错文案要求"少字节"。
        let w = define(&write_scene(Tool::Gnu));
        assert!(w.abort_consistent(&write_scene(Tool::Gnu)));
        let write_full = Outcome { value: 8192, ..w };
        assert!(!write_full.abort_consistent(&write_scene(Tool::Gnu)));
        // close 错文案要求"字节全在"。
        let c = define(&close_scene(Tool::Gnu));
        assert!(c.abort_consistent(&close_scene(Tool::Gnu)));
        let close_short = Outcome { value: 4096, ..c };
        assert!(!close_short.abort_consistent(&close_scene(Tool::Gnu)));
        // 建件错要求"零字节"。
        let cf = define(&create_scene(Tool::Gnu, ERR_ENOSPC));
        assert!(cf.abort_consistent(&create_scene(Tool::Gnu, ERR_ENOSPC)));
        assert!(!Outcome { value: 1, ..cf }.abort_consistent(&create_scene(Tool::Gnu, ERR_ENOSPC)));
        // 归因位为假时三元组检查一律放行（成功与尾段不受此约束）。
        let free = Outcome { stage: Stage::Exit, code: EXIT_OK, value: 8192, note: Note::Complete, default: false };
        assert!(free.abort_consistent(&write_scene(Tool::Gnu)));
        // 码不是 1 就说不上"因满盘中止而退 1"。
        let zero = Outcome { code: EXIT_OK, ..w };
        assert!(!zero.abort_consistent(&write_scene(Tool::Gnu)));
        // 尾段注解自称中止：不属中止族，判破断。
        let tail_lie = Outcome {
            stage: Stage::Pres,
            code: EXIT_FILE,
            value: 8192,
            note: Note::PresTime,
            default: true,
        };
        assert!(!tail_lie.abort_consistent(&tail_scene(PRES_TIME)));
        assert!(!tail_lie.attributed());
        // AbortMismatch 也不属中止族：它只在手工比对里出现，define 永不产出。
        let mismatch = Outcome {
            stage: Stage::Werr,
            code: EXIT_FILE,
            value: 8192,
            note: Note::AbortMismatch,
            default: true,
        };
        assert!(!mismatch.abort_consistent(&write_scene(Tool::Gnu)));
        assert!(!mismatch.attributed());
        assert!(abort_ok());
    }

    // ------------------------------------------------------------------
    // 定义纵线：七闸逐段
    // ------------------------------------------------------------------

    #[test]
    fn open_gate_separates_usage_from_create_failure() {
        let g = define(&usage_scene(Tool::Gnu));
        assert_eq!(g.stage, Stage::Open);
        assert_eq!(g.code, EXIT_HARD);
        assert_eq!(g.note, Note::BadOption);
        assert_eq!(g.value, 0);
        assert!(!g.default);
        assert_eq!(Tier::of(&g), Tier::Tail);
        assert_eq!(message(&usage_scene(Tool::Gnu), &g).as_str(), "cp: unrecognized option '--preserve=xattr'");
        // 建件失败：ENOSPC 记中止，留档字节恒为 0（这条信息只有本闸给得出）。
        let cf = create_scene(Tool::Gnu, ERR_ENOSPC);
        let o = define(&cf);
        assert_eq!(o.stage, Stage::Open);
        assert_eq!(o.code, EXIT_FILE);
        assert_eq!(o.note, Note::CreateFail);
        assert_eq!(o.value, 0);
        assert!(o.default);
        assert!(o.attributed());
        assert_eq!(Tier::of(&o), Tier::Aborted);
        assert_eq!(o.shortfall(&cf), 8192);
        // 邻居 errno 同闸不同档：红，但不是满盘中止。
        for e in [ERR_EACCES, ERR_EPERM, ERR_EISDIR, ERR_EDQUOT, ERR_EIO] {
            let f = define(&create_scene(Tool::Busybox, e));
            assert_eq!(f.note, Note::ForeignErr, "errno {} 被误记成满盘中止", e);
            assert!(!f.default);
            assert_eq!(f.code, EXIT_FILE);
            assert_eq!(f.value, 0);
            assert_eq!(Tier::of(&f), Tier::Tail);
        }
    }

    #[test]
    fn stat_gate_lands_two_gates_earlier_on_gnu() {
        let dir = dir_scene(Tool::Gnu);
        let o = define(&dir);
        assert_eq!(o.stage, Stage::Stat);
        assert_eq!(o.note, Note::OmitDir);
        assert_eq!(o.code, EXIT_FILE);
        assert_eq!(o.value, 0);
        assert!(!o.default);
        assert!(o.stage.order() < Stage::Copy.order());
        assert_eq!(message(&dir, &o).as_str(), "cp: -r not specified; omitting directory 'logs'");
        // busybox 到读循环才报，落点晚两道闸。
        let bb = dir_scene(Tool::Busybox);
        let b = define(&bb);
        assert_eq!(b.stage, Stage::Copy);
        assert_eq!(b.note, Note::IsDir);
        assert_eq!(b.code, EXIT_FILE);
        assert_eq!(message(&bb, &b).as_str(), "cp: 'logs': Is a directory");
        assert_eq!(o.stage.order() + 1, b.stage.order());
        // 带上 `-r` 的同一目录源必须走完全程。
        let mut walk = dir;
        walk.mode = CopyMode::Recursive;
        walk.traversal = true;
        let w = define(&walk);
        assert!(w.ok());
        assert_eq!(w.stage, Stage::Exit);
        assert_eq!(w.note, Note::Complete);
    }

    #[test]
    fn copy_gate_kills_silent_truncation() {
        let s = silent_scene();
        let o = define(&s);
        assert_eq!(o.stage, Stage::Copy);
        assert_eq!(o.note, Note::SilentTrunc);
        assert_eq!(o.code, EXIT_FILE);
        assert_eq!(o.value, 4096);
        assert!(!o.default);
        assert_eq!(o.shortfall(&s), 4096);
        assert_eq!(Tier::of(&o), Tier::Tail);
        assert!(o.note.defect_family());
        let m = message(&s, &o);
        assert!(m.contains("silent truncation"));
        assert_eq!(m.as_str(), "silent truncation at 'mnt/logs/day.bin' (4096 of 8192 bytes, no error reported)");
        // 同一个半件现场只要有错误码就不再算静默截断——两闸必须分家。
        let real = write_scene(Tool::Gnu);
        assert_eq!(define(&real).stage, Stage::Werr);
        assert_ne!(define(&s).stage, define(&real).stage);
        // 写满了自然不算截断。
        let full = s.clone_written(8192);
        assert_eq!(define(&full).note, Note::Complete);
    }

    #[test]
    fn werr_gate_splits_the_two_wordings_and_keeps_the_bytes() {
        let gnu = write_scene(Tool::Gnu);
        let o = define(&gnu);
        assert_eq!(o.stage, Stage::Werr);
        assert_eq!(o.note, Note::ErrorWriting);
        assert_eq!(o.code, EXIT_FILE);
        assert_eq!(o.value, 4096);
        assert!(o.default);
        assert!(o.attributed());
        assert_eq!(Tier::of(&o), Tier::Aborted);
        assert_eq!(
            message(&gnu, &o).as_str(),
            "cp: error writing 'mnt/logs/day.bin': No space left on device"
        );
        let bb = write_scene(Tool::Busybox);
        let b = define(&bb);
        assert_eq!(b.stage, Stage::Werr);
        assert_eq!(b.note, Note::WriteError);
        assert_eq!(b.code, EXIT_FILE);
        assert_eq!(b.value, 4096);
        assert!(b.default);
        assert_ne!(b.seal(), o.seal());
        assert_ne!(message(&bb, &b), message(&gnu, &o));
        // 配额满/IO 错不是 ENOSPC：同闸、同码，但归因位必须假。
        for e in [ERR_EDQUOT, ERR_EIO, ERR_EACCES, ERR_EPERM] {
            let f = define(&gnu.clone_errno(e));
            assert_eq!(f.note, Note::ForeignErr);
            assert_eq!(f.stage, Stage::Werr);
            assert!(!f.default);
            assert_eq!(f.code, EXIT_FILE);
            assert!(!message(&gnu.clone_errno(e), &f).contains(ENOSPC_TAIL));
        }
    }

    #[test]
    fn cerr_gate_is_the_busybox_black_hole() {
        let gnu = close_scene(Tool::Gnu);
        let o = define(&gnu);
        assert_eq!(o.stage, Stage::ClErr);
        assert_eq!(o.note, Note::CloseEnospc);
        assert_eq!(o.code, EXIT_FILE);
        assert_eq!(o.value, 8192);
        assert!(o.default);
        assert!(o.abort_consistent(&gnu));
        assert_eq!(o.shortfall(&gnu), 0);
        assert_eq!(
            message(&gnu, &o).as_str(),
            "cp: failed to close 'mnt/db.dump': No space left on device"
        );
        // busybox 丢 close 返回值：现场红过，结论却是码 0。
        let bb = close_scene(Tool::Busybox);
        let b = define(&bb);
        assert_eq!(b.stage, Stage::Exit);
        assert_eq!(b.note, Note::CloseIgnored);
        assert_eq!(b.code, EXIT_OK);
        assert_eq!(b.value, 8192);
        assert!(!b.default);
        assert!(!b.attributed());
        assert_eq!(Tier::of(&b), Tier::Warned);
        assert!(b.ok());
        assert_eq!(
            message(&bb, &b).as_str(),
            "close error dropped by busybox for 'mnt/db.dump' (8192 bytes of deferred blocks unaccounted)"
        );
        assert_eq!(warning(&bb, &b), message(&bb, &b));
        // 邻居 errno 走 close 闸时仍报 close 那句，尾巴不是满盘。
        let io = define(&close_scene(Tool::Gnu).clone_errno(ERR_EIO));
        assert_eq!(io.note, Note::ForeignErr);
        assert_eq!(io.stage, Stage::ClErr);
        assert!(!io.default);
        assert!(message(&close_scene(Tool::Gnu).clone_errno(ERR_EIO), &io)
            .contains(GNU_CLOSE_TAIL));
    }

    #[test]
    fn pres_gate_never_changes_bytes_and_never_aborts() {
        // 多位同时失败按 mode→mtime→xattr→sparse 的固定次序取首位。
        assert_eq!(define(&tail_scene(PRES_MODE)).note, Note::PresMode);
        assert_eq!(define(&tail_scene(PRES_TIME)).note, Note::PresTime);
        assert_eq!(define(&tail_scene(PRES_XATTR)).note, Note::PresXattr);
        assert_eq!(define(&tail_scene(PRES_SPARSE)).note, Note::PresSparse);
        let all = tail_scene(PRES_MODE | PRES_TIME | PRES_XATTR | PRES_SPARSE);
        let o = define(&all);
        assert_eq!(o.note, Note::PresMode);
        assert_eq!(o.stage, Stage::Pres);
        assert_eq!(o.code, EXIT_FILE);
        assert_eq!(o.value, 8192);
        assert_eq!(o.shortfall(&all), 0);
        assert!(!o.default);
        assert!(o.note.tail_family());
        assert_eq!(Tier::of(&o), Tier::Tail);
        // 数据已经到位：尾段的 ENOSPC 仍不算中止。
        let mut enospc = tail_scene(PRES_SPARSE);
        enospc.errno_seen = ERR_ENOSPC;
        let e = define(&enospc);
        assert_eq!(e.stage, Stage::Pres);
        assert_eq!(e.code, EXIT_FILE);
        assert!(!e.default);
        // 尾段落点必须在 close 闸之后。
        assert!(Stage::Pres.order() > Stage::ClErr.order());
        assert_eq!(
            message(&tail_scene(PRES_MODE), &define(&tail_scene(PRES_MODE))).as_str(),
            "cp: preserving permissions for 'mnt/cfg.conf': Operation not permitted"
        );
    }

    #[test]
    fn exit_gate_is_the_only_zero_for_non_usage_cases() {
        let w = warn_scene(Tool::Gnu);
        let o = define(&w);
        assert_eq!(o.stage, Stage::Exit);
        assert_eq!(o.note, Note::WarnSparse);
        assert_eq!(o.code, EXIT_OK);
        assert_eq!(o.value, 8192);
        assert!(o.ok());
        assert_eq!(Tier::of(&o), Tier::Warned);
        assert!(message(&w, &o).starts_with(SPARSE_HEAD));
        assert_eq!(warning(&w, &o), message(&w, &o));
        // 成功且无声：message 通道空，warning 通道也空。
        let clean = Shape::blank();
        let c = define(&clean);
        assert!(message(&clean, &c).is_empty());
        assert!(warning(&clean, &c).is_empty());
        assert_eq!(c.note, Note::Complete);
    }

    #[test]
    fn full_disk_exit_code_table_across_both_tools() {
        // GNU：满盘时 1（写错/close 错/建件错都是 1），只有纯告警与全件落定是 0。
        assert_eq!(define(&write_scene(Tool::Gnu)).code, 1);
        assert_eq!(define(&close_scene(Tool::Gnu)).code, 1);
        assert_eq!(define(&create_scene(Tool::Gnu, ERR_ENOSPC)).code, 1);
        assert_eq!(define(&warn_scene(Tool::Gnu)).code, 0);
        assert_eq!(define(&Shape::blank()).code, 0);
        assert_eq!(define(&usage_scene(Tool::Gnu)).code, 2);
        // busybox：写错仍退 1，close 错被吞则恒 0。
        assert_eq!(define(&write_scene(Tool::Busybox)).code, 1);
        assert_eq!(define(&create_scene(Tool::Busybox, ERR_ENOSPC)).code, 1);
        assert_eq!(define(&close_scene(Tool::Busybox)).code, 0);
        assert_eq!(define(&usage_scene(Tool::Busybox)).code, 1);
        // busybox 那格若被"纠正"成退 1，就是替工具补了它没做的归因——Estate 先撞。
        let bb = slot(7);
        let mut fixed = bb;
        fixed.claim.code = EXIT_FILE;
        assert_eq!(judge(&fixed), Fault::Estate);
        // 尾段红过就绝不许 0：close 被吞 + chmod 失败仍退 1（pres 闸在 exit 闸之前）。
        let mut drop = close_scene(Tool::Busybox);
        drop.mode = CopyMode::Preserve;
        drop.preserved = PRES_P;
        drop.pres_failed = PRES_MODE;
        drop.errno_seen = ERR_EPERM;
        let d = define(&drop);
        assert_eq!(d.stage, Stage::Pres);
        assert_eq!(d.code, EXIT_FILE);
        assert_ne!(d.note, Note::CloseIgnored);
    }

    #[test]
    fn enospc_and_write_error_are_a_paired_anchor() {
        // errno=28 与 write==-1 成对出现才有归因力：只有红标记没 errno 的形状不许进判据。
        let mut naked = write_scene(Tool::Gnu);
        naked.errno_seen = ERR_NONE;
        assert!(!naked.valid());
        // errno 0 本身在合法值表里，缺的是"成对"的那一半，不是取值域。
        assert!(naked.errno_legal());
        // close 红却没 errno 同理。
        let mut naked_close = close_scene(Tool::Gnu);
        naked_close.errno_seen = ERR_NONE;
        assert!(!naked_close.valid());
        // 成对的锚：write_err + ENOSPC → 中止族文案 + 归因位真。
        let pair = write_scene(Tool::Gnu);
        assert!(pair.valid());
        assert_eq!(pair.errno_seen, ERR_ENOSPC);
        assert!(pair.write_err);
        let o = define(&pair);
        assert!(o.note.abort_family());
        assert!(o.default);
        // 本卡只引进不重判：errno 的大小档不改留档字节，只改文案落点。
        assert_eq!(o.value, pair.written);
        for e in [ERR_EACCES, ERR_EPERM, ERR_EISDIR, ERR_EDQUOT, ERR_EIO] {
            let f = define(&pair.clone_errno(e));
            assert_eq!(f.value, o.value);
            assert_eq!(f.code, o.code);
            assert_ne!(f.note, o.note);
            assert!(!f.default);
        }
        assert!(slot(3).shape.errno_seen == ERR_ENOSPC && slot(3).shape.write_err);
    }

    #[test]
    fn cross_card_anchors_stay_out_of_this_cards_lane() {
        // F11681 的锚：errno 与 (返回, errno) 成对，本卡只引结论。
        assert_eq!(ERR_ENOSPC, 28);
        assert_eq!(strerror(ERR_ENOSPC), "No space left on device");
        // F11683 的块算术：本卡不数块——payload 与块大小无关。
        let mut s = write_scene(Tool::Gnu);
        s.holes = 0;
        assert_eq!(s.payload(), 8192);
        assert_eq!(s.geometry().apparent, 8192);
        // 画像的 bsize 只服务 free_bytes，不改本卡的字节记账。
        let ps = profiles();
        let mut i = 0usize;
        let mut sizes = [0u32; 3];
        while i < ps.len() {
            if let Some(p) = ps.get(i) {
                if let Some(n) = sizes.get_mut(i) {
                    *n = p.bsize;
                }
            }
            i += 1;
        }
        assert_eq!(sizes, [4096, 1024, 512]);
        assert_eq!(s.payload(), slot(3).shape.payload());
        assert_eq!(slot(3).actual().shortfall(&slot(3).shape), 4096);
    }

    #[test]
    fn gate_order_is_never_rewritten_by_a_later_fault() {
        // 写红 + 尾段红：werr 先落，尾段无从改写。
        let mut both = write_scene(Tool::Gnu);
        both.mode = CopyMode::Archive;
        both.preserved = PRES_ALL;
        both.pres_failed = PRES_TIME;
        let o = define(&both);
        assert_eq!(o.stage, Stage::Werr);
        assert_eq!(o.note, Note::ErrorWriting);
        assert!(o.default);
        assert_eq!(o.value, both.written);
        // close 红 + 尾段红：cerr 先落（GNU 先 close 再设属性）。
        let mut tc = close_scene(Tool::Gnu);
        tc.mode = CopyMode::Archive;
        tc.preserved = PRES_ALL;
        tc.pres_failed = PRES_TIME;
        let c = define(&tc);
        assert_eq!(c.stage, Stage::ClErr);
        assert_eq!(c.note, Note::CloseEnospc);
        // 建件失败 + 目录源：open 闸先短路，stat 闸无从表态。
        let mut cd = create_scene(Tool::Gnu, ERR_ENOSPC);
        cd.src_dir = true;
        assert_eq!(define(&cd).stage, Stage::Open);
        // 用法错压过一切（连 errno 都不看）。
        let mut u = usage_scene(Tool::Gnu);
        u.create_ok = false;
        u.errno_seen = ERR_ENOSPC;
        u.src_dir = true;
        let r = define(&u);
        assert_eq!(r.stage, Stage::Open);
        assert_eq!(r.note, Note::BadOption);
        assert_eq!(r.code, EXIT_HARD);
        // 静默截断（copy 闸）不被 werr 闸认领：没有错误码就没有 ENOSPC 归因。
        let s = silent_scene();
        assert_eq!(define(&s).stage, Stage::Copy);
        assert_eq!(define(&s).note, Note::SilentTrunc);
        assert!(!define(&s).default);
        assert!(define_ok());
    }

    #[test]
    fn define_is_a_total_function_on_the_corpus() {
        let cs = cases();
        let mut i = 0usize;
        let mut stages = [0u32; 7];
        while i < CORPUS {
            let c = match cs.get(i) {
                Some(v) => *v,
                None => panic!("语料下标越界 {}", i),
            };
            assert!(c.shape.valid());
            let a = define(&c.shape);
            assert_eq!(a, c.actual());
            // 留档字节：建不出件的那一闸恒 0，其余各闸就是已落的那部分。
            assert_eq!(a.value, c.shape.written);
            assert_eq!(a.ok(), a.code == EXIT_OK && a.note.success());
            assert_eq!(a.stage.failure(), a.stage != Stage::Exit);
            if let Some(n) = stages.get_mut(usize::from(a.stage.order())) {
                *n += 1;
            }
            i += 1;
        }
        // 七闸的落点分布：语料没踩到 copy 闸（静默截断只在探针形状里出现）。
        assert_eq!(stages.iter().sum::<u32>(), CORPUS as u32);
        assert_eq!(stages.get(usize::from(Stage::Copy.order())), Some(&0));
        assert_eq!(stages.get(usize::from(Stage::Exit.order())), Some(&3));
        assert_eq!(stages.get(usize::from(Stage::Werr.order())), Some(&7));
        assert_eq!(stages.get(usize::from(Stage::Open.order())), Some(&1));
    }

    // ------------------------------------------------------------------
    // 语料 / 三份陈述 / 对判
    // ------------------------------------------------------------------

    #[test]
    fn corpus_tags_are_unique_and_the_replicas_carry_the_suffix() {
        let cs = cases();
        assert_eq!(cs.len(), CORPUS);
        let mut i = 0usize;
        while i < CORPUS {
            let c = match cs.get(i) {
                Some(v) => *v,
                None => panic!("语料越界 {}", i),
            };
            let t = match WANT_TAGS.get(i) {
                Some(v) => *v,
                None => panic!("标签表越界 {}", i),
            };
            assert_eq!(c.tag(), t);
            assert!(!c.shape.tag.is_empty());
            assert!(!c.shape.dst.is_empty());
            assert_eq!(c.tag().ends_with(SWAP_SUFFIX), i >= ACCEPT);
            let mut j = i + 1;
            while j < CORPUS {
                if let Some(u) = WANT_TAGS.get(j) {
                    assert_ne!(*u, t, "标签 {} 重复", t);
                }
                j += 1;
            }
            i += 1;
        }
        assert_eq!(WANT_TAGS.first(), Some(&"gnu-plain-complete"));
        assert_eq!(WANT_TAGS.get(3), Some(&"gnu-r-mid-copy-enospc"));
        assert_eq!(WANT_TAGS.get(8), Some(&"estate-swap"));
        assert_eq!(WANT_TAGS.last(), Some(&"default-swap"));
    }

    #[test]
    fn want_tables_are_a_second_statement_of_the_claims() {
        let cs = cases();
        let mut i = 0usize;
        while i < CORPUS {
            let c = match cs.get(i) {
                Some(v) => *v,
                None => panic!("语料越界 {}", i),
            };
            let a = c.actual();
            assert_eq!(WANT_STAGE.get(i), Some(&a.stage), "槽 {} 的阶段两表不一致", i);
            assert_eq!(WANT_CODE.get(i), Some(&a.code), "槽 {} 的码两表不一致", i);
            assert_eq!(WANT_VALUE.get(i), Some(&a.value), "槽 {} 的字节两表不一致", i);
            assert_eq!(WANT_NOTE.get(i), Some(&a.note), "槽 {} 的文案两表不一致", i);
            assert_eq!(WANT_DEFAULT.get(i), Some(&a.default), "槽 {} 的归因两表不一致", i);
            if i < ACCEPT {
                // 活槽：申报 == 期望 == 实跑，三份陈述必须圆回同一点。
                assert_eq!(WANT_STAGE.get(i), Some(&c.claim.stage));
                assert_eq!(WANT_CODE.get(i), Some(&c.claim.code));
                assert_eq!(WANT_VALUE.get(i), Some(&c.claim.value));
                assert_eq!(WANT_NOTE.get(i), Some(&c.claim.note));
                assert_eq!(WANT_DEFAULT.get(i), Some(&c.claim.default));
                assert_eq!(c.judge(), Fault::Ok);
                assert!(c.ok());
            } else {
                // 复刻槽：申报与实跑恰好差在 WANT_DIFF 写死的那几维上。
                assert_eq!(
                    dims_off(&a, &c.claim),
                    match WANT_DIFF.get(i) { Some(v) => *v, None => 0 },
                    "槽 {} 的差分掩码与写死表不符",
                    i
                );
                assert_ne!(c.judge(), Fault::Ok);
                assert!(!c.ok());
            }
            i += 1;
        }
        assert_eq!(WANT_CODE, [0, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1]);
        assert_eq!(WANT_VALUE, [8192, 8192, 0, 4096, 8192, 2048, 0, 4096, 4096, 4096, 4096, 4096, 4096, 4096]);
        assert_eq!(WANT_DIFF.iter().filter(|d| **d == 0).count(), ACCEPT);
        assert!(corpus_ok());
    }

    #[test]
    fn judge_returns_estate_before_anything_else() {
        // 实际中止 + 成功申报 → Estate，即使阶段/码/字节/文案/归因全同时压不住。
        let red = slot(8);
        assert!(red.actual().failed());
        assert!(red.claim.success());
        assert_eq!(judge(&red), Fault::Estate);
        // 补齐任一单维都不够：成败互报永远先撞。
        let mut one = red;
        one.claim.stage = red.actual().stage;
        assert_eq!(judge(&one), Fault::Estate);
        let mut two = red;
        two.claim.default = red.actual().default;
        assert_eq!(judge(&two), Fault::Estate);
        // 反过来：成功实际 + 失败申报 → 同为 Estate。
        let green = slot(0);
        let mut g2 = green;
        g2.claim = Claim {
            stage: Stage::Werr,
            code: 1,
            value: 4096,
            note: Note::ErrorWriting,
            default: true,
        };
        assert_eq!(judge(&g2), Fault::Estate);
        // 把红文案那一维补上，成败翻面后缺陷才退到阶段维。
        let mut three = red;
        three.claim.note = red.actual().note;
        assert!(!three.claim.success());
        assert_eq!(judge(&three), Fault::Stage);
        // 五维全补齐才转绿。
        let mut healed = red;
        healed.claim = claim_of(&red.actual());
        assert_eq!(judge(&healed), Fault::Ok);
    }

    #[test]
    fn judge_single_dim_injections_land_on_their_own_fault() {
        let cs = cases();
        let mut i = 0usize;
        while i < ACCEPT {
            let c = match cs.get(i) {
                Some(v) => *v,
                None => panic!("语料越界 {}", i),
            };
            let a = c.actual();
            let mut bad = c;
            bad.claim.value = a.value + 1;
            assert_eq!(judge(&bad), Fault::Value, "槽 {} 的字节维没被单独抓到", i);
            let mut bad = c;
            let swap = kin_note(a.note);
            bad.claim.note = swap;
            if swap == a.note {
                // 槽 4 现场就是 failed-to-close，同族里没有第二种可换；跳过而不是造一个跨族注入。
                assert_eq!(a.note, Note::CloseEnospc, "槽 {} 的同族注解本该只有一种", i);
            } else {
                assert_eq!(judge(&bad), Fault::Note, "槽 {} 的文案维没被单独抓到", i);
            }
            let mut bad = c;
            bad.claim.default = !a.default;
            assert_eq!(judge(&bad), Fault::Default, "槽 {} 的归因维没被单独抓到", i);
            let mut bad = c;
            bad.claim.stage = if a.stage == Stage::Open { Stage::Exit } else { Stage::Open };
            assert_eq!(judge(&bad), Fault::Stage, "槽 {} 的阶段维没被单独抓到", i);
            // 退出码单维注入：码改了会连带翻成败，那正是 Estate 优先的活证据。
            let mut bad = c;
            bad.claim.code = a.code + 2;
            let f = judge(&bad);
            assert!(f == Fault::Code || f == Fault::Estate);
            i += 1;
        }
        assert!(judge_ok());
    }

    #[test]
    fn dims_off_and_dim_fault_reproduce_the_judge_order() {
        let cs = cases();
        let mut i = 0usize;
        while i < CORPUS {
            let c = match cs.get(i) {
                Some(v) => *v,
                None => panic!("语料越界 {}", i),
            };
            let m = match WANT_DIFF.get(i) {
                Some(v) => *v,
                None => panic!("差分表越界 {}", i),
            };
            let a = c.actual();
            assert_eq!(dims_off(&a, &c.claim), m, "槽 {} 的差分掩码对不上", i);
            assert_eq!(dim_fault(m), judge(&c), "槽 {} 的独立判序与 judge() 分家", i);
            assert_eq!(dim_fault(0), Fault::Ok);
            i += 1;
        }
        assert_eq!(D_ESTATE, 1);
        assert_eq!(D_STAGE, 2);
        assert_eq!(D_CODE, 4);
        assert_eq!(D_VALUE, 8);
        assert_eq!(D_NOTE, 16);
        assert_eq!(D_DEFAULT, 32);
        assert_eq!(dim_popcount(0), 0);
        assert_eq!(dim_popcount(D_DATA_ALL), 5);
        assert_eq!(dim_popcount(D_ESTATE | D_DATA_ALL), 6);
        assert_eq!(dim_popcount(0xff), 8);
        assert_eq!(dim_popcount(D_ESTATE | D_VALUE), 2);
    }

    #[test]
    fn six_replicas_share_one_scene_but_six_distinct_claims() {
        let base = slot(3);
        let base_seal = base.actual().seal();
        let mut i = ACCEPT;
        while i < CORPUS {
            let c = slot(i);
            assert!(same_scene(&c.shape, &base.shape), "槽 {} 偷改了现场", i);
            assert_eq!(c.actual().seal(), base_seal, "槽 {} 的现场结论漂移", i);
            assert_eq!(c.tag(), match WANT_TAGS.get(i) { Some(t) => *t, None => "" });
            assert_ne!(c.shape.seal(), base.shape.seal(), "槽 {} 与活槽形状撞车", i);
            let mut j = i + 1;
            while j < CORPUS {
                let d = slot(j);
                assert_ne!(c.claim, d.claim, "复刻槽 {} 与 {} 申报相同", i, j);
                j += 1;
            }
            i += 1;
        }
        // 抹平各自那一维即转绿：六条判据条条在实判。
        let mut k = 0usize;
        while k < REJECT {
            let c = match cases().get(ACCEPT + k) {
                Some(v) => *v,
                None => panic!("复刻槽越界"),
            };
            let a = c.actual();
            let mut healed = c;
            if k == 0 {
                healed.claim = claim_of(&a);
            } else {
                match k {
                    1 => healed.claim.stage = a.stage,
                    2 => healed.claim.code = a.code,
                    3 => healed.claim.value = a.value,
                    4 => healed.claim.note = a.note,
                    _ => healed.claim.default = a.default,
                }
            }
            assert_eq!(judge(&healed), Fault::Ok, "抹平第 {} 维仍红：判序优先级有问题", k);
            k += 1;
        }
        assert!(reds_ok());
    }

    #[test]
    fn case_accessors_and_bounds_agree() {
        assert_eq!(cases().len(), CORPUS);
        let mut i = 0usize;
        while i < CORPUS {
            let a = match case_at(i) {
                Some(v) => v,
                None => panic!("槽 {} 取不到", i),
            };
            let b = match cases().get(i) {
                Some(v) => *v,
                None => panic!("槽 {} 组不进表", i),
            };
            assert_eq!(a, b);
            assert_eq!(a.shape, match SHAPES.get(i) { Some(v) => *v, None => panic!("SHAPES 越界") });
            assert_eq!(a.claim, match CLAIMS.get(i) { Some(v) => *v, None => panic!("CLAIMS 越界") });
            assert_eq!(a.actual(), define(&a.shape));
            assert_eq!(a.judge(), judge(&a));
            assert_eq!(a.ok(), a.judge() == Fault::Ok);
            i += 1;
        }
        assert!(case_at(CORPUS).is_none());
        assert!(case_at(CORPUS + 7).is_none());
        assert!(case_at(usize::MAX).is_none());
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(CORPUS - 1), Err(CP_ENOBS));
        assert_eq!(cover(0), Err(CP_ENOBS));
        assert_eq!(cover(usize::MAX), Err(CP_ENOBS));
        assert_eq!(Case::blank().shape, Shape::blank());
        assert_eq!(Case::blank().claim, Claim::blank());
        assert_eq!(Case::blank().tag(), "");
    }

    #[test]
    fn expected_table_splits_live_and_dead_slots() {
        let mut i = 0usize;
        while i < ACCEPT {
            assert_eq!(expected(i), Some(Fault::Ok), "活槽 {} 的期望不是 Ok", i);
            assert_eq!(ACCEPT_SLOTS.get(i), Some(&i));
            i += 1;
        }
        let mut j = 0usize;
        while j < REJECT {
            let want = match REJECT_FAULTS.get(j) {
                Some(v) => *v,
                None => panic!("缺陷表越界"),
            };
            assert_eq!(expected(ACCEPT + j), Some(want));
            assert_eq!(REJECT_SLOTS.get(j), Some(&(ACCEPT + j)));
            j += 1;
        }
        assert_eq!(ACCEPT_SLOTS.len() + REJECT_SLOTS.len(), CORPUS);
        assert!(expected(CORPUS).is_none());
        assert!(expected(CORPUS + 1).is_none());
        assert!(expected(usize::MAX).is_none());
        assert_eq!(ACCEPT + REJECT, CORPUS);
        assert!(Claim::blank().success());
        assert!(!Claim { stage: Stage::Werr, code: 1, value: 0, note: Note::ErrorWriting, default: true }.success());
        assert!(Claim { stage: Stage::Open, code: 0, value: 0, note: Note::CloseIgnored, default: false }.success());
    }

    // ------------------------------------------------------------------
    // 矩阵 / 报表 / 全跑
    // ------------------------------------------------------------------

    #[test]
    fn matrix_of_projects_the_four_documented_tiers() {
        let m = Matrix::of();
        assert_eq!(m.slots(), CORPUS);
        assert_eq!(m.goods(), ACCEPT);
        assert!(m.covered());
        assert_eq!(m.perfect_rows(), 3);
        let complete = match m.row(Tier::Complete) {
            Some(v) => v,
            None => panic!("完成档缺失"),
        };
        let warned = match m.row(Tier::Warned) {
            Some(v) => v,
            None => panic!("告警档缺失"),
        };
        let tail = match m.row(Tier::Tail) {
            Some(v) => v,
            None => panic!("尾段档缺失"),
        };
        let aborted = match m.row(Tier::Aborted) {
            Some(v) => v,
            None => panic!("中止档缺失"),
        };
        assert_eq!((complete.total, complete.good), (1, 1));
        assert_eq!((warned.total, warned.good), (2, 2));
        assert_eq!((tail.total, tail.good), (2, 2));
        assert_eq!((aborted.total, aborted.good), (9, 3));
        assert_eq!(aborted.red(), 6);
        assert!(aborted.reds_are_replicas());
        assert_eq!(complete.red(), 0);
        assert_eq!(warned.red(), 0);
        assert_eq!(tail.red(), 0);
        assert_eq!(m.render().as_str(), concat!(
            "complete total=1 good=1 red=0\n",
            "warned total=2 good=2 red=0\n",
            "tail total=2 good=2 red=0\n",
            "aborted total=9 good=3 red=6\n",
        ));
        assert_eq!(m.rows.len(), TIER_COUNT);
        assert!(matrix_ok());
    }

    #[test]
    fn matrix_blank_and_lying_rows_are_caught() {
        let b = Matrix::blank();
        assert!(!b.covered());
        assert_eq!(b.slots(), 0);
        assert_eq!(b.goods(), 0);
        assert_eq!(b.perfect_rows(), 0);
        assert_eq!(b.row(Tier::Aborted), Some(Row::blank()));
        assert!(b.rows.iter().all(|r| r.total == 0 && r.good == 0));
        // 空档就是没覆盖：任何一档 total 归零都必须翻红。
        let mut one = Matrix::of();
        if let Some(r) = one.rows.get_mut(Tier::Complete.index()) {
            r.total = 0;
        }
        assert!(!one.covered());
        // 谎报的绿：good 超过 total 的行不许被算成全对档。
        let lie = Matrix {
            rows: [
                Row { total: 1, good: 3 },
                Row { total: 2, good: 2 },
                Row { total: 2, good: 2 },
                Row { total: 9, good: 3 },
            ],
        };
        assert!(lie.covered());
        assert_eq!(lie.goods(), 10);
        assert_eq!(lie.slots(), 14);
        assert_eq!(lie.perfect_rows(), 2);
        assert_eq!(lie.rows.first().map(|r| r.red()), Some(1));
        assert!(!match lie.rows.first() {
            Some(r) => r.perfect(),
            None => true,
        });
        assert_eq!(lie.render().lines().count(), TIER_COUNT);
    }

    #[test]
    fn row_helpers_render_the_paired_counters() {
        let r = Row { total: 9, good: 3 };
        assert_eq!(r.red(), 6);
        assert!(!r.perfect());
        assert!(r.reds_are_replicas());
        assert_eq!(r.render(Tier::Aborted).as_str(), "aborted total=9 good=3 red=6\n");
        let p = Row { total: 2, good: 2 };
        assert!(p.perfect());
        assert_eq!(p.red(), 0);
        assert!(p.reds_are_replicas());
        assert_eq!(p.render(Tier::Warned).as_str(), "warned total=2 good=2 red=0\n");
        let e = Row::blank();
        assert!(!e.perfect());
        assert_eq!(e.red(), 0);
        assert_eq!(e.render(Tier::Tail).as_str(), "tail total=0 good=0 red=0\n");
        // 只有 0 或整六格的红才叫"红全来自复刻槽"；夹生红必须翻红。
        assert!(!Row { total: 5, good: 3 }.reds_are_replicas());
        assert!(Row { total: 6, good: 0 }.reds_are_replicas());
        // good > total 是坏统计：red() 饱和回 total，不回绕。
        assert_eq!(Row { total: 1, good: 8 }.red(), 1);
        assert_eq!(Row { total: 0, good: 4 }.red(), 0);
    }

    #[test]
    fn report_rate_is_rounded_to_the_mille() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => panic!("全绿语料跑不出报表"),
        };
        assert_eq!(r.total, CORPUS as u32);
        assert_eq!(r.accept, ACCEPT as u32);
        assert_eq!(r.reject, REJECT as u32);
        assert_eq!(r.complete, 1);
        assert_eq!(r.warned, 2);
        assert_eq!(r.tail, 2);
        assert_eq!(r.aborted, 3);
        assert_eq!(r.matched, 14);
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.rate(), (14 * 1000 + 14 / 2) / 14);
        // 929 那一档的推导：(13*1000 + 7) / 14。
        let red = Report { matched: 13, ..r };
        assert_eq!(red.rate(), (13 * 1000 + 7) / 14);
        assert_eq!(red.rate(), 929);
        assert_eq!(red.rate(), match VALS.last() { Some(v) => *v, None => 0 });
        // 千分位的四舍五入方向：0.5 档向上。
        assert_eq!(Report { total: 3, matched: 2, ..r }.rate(), 667);
        assert_eq!(Report { total: 7, matched: 1, ..r }.rate(), 143);
        assert_eq!(Report { total: 14, matched: 0, ..r }.rate(), 0);
        assert_eq!(Report { total: 0, matched: 0, ..r }.rate(), 0);
        assert_eq!(Report { total: 0, matched: 14, ..r }.rate(), 0);
        assert_eq!(Report { total: 1, matched: 1, ..r }.rate(), 1000);
    }

    #[test]
    fn report_gate_and_green_need_both_counters() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => panic!("全绿语料跑不出报表"),
        };
        assert!(r.green());
        assert_eq!(r.gate(), Ok(()));
        // 掉一格 matched 就红：gate 报计数错，且绿不再成立。
        let one = Report { matched: 13, ..r };
        assert!(!one.green());
        assert_eq!(one.gate(), Err(CP_ECOUNT));
        let short = Report { total: 13, matched: 13, ..r };
        assert!(!short.green());
        assert_eq!(short.gate(), Err(CP_ECOUNT));
        // matched 超过 total 是坏申报：green() 要求两者相等。
        let over = Report { matched: 15, ..r };
        assert!(!over.green());
        assert_eq!(over.gate(), Err(CP_ECOUNT));
        assert_eq!(r.zero_cells(), r.complete + r.warned);
        assert_eq!(r.nonzero_cells(), r.tail + r.aborted);
        assert_eq!(r.zero_cells() + r.nonzero_cells(), r.accept);
        assert_eq!(r.zero_cells(), 3);
        assert_eq!(r.nonzero_cells(), 5);
    }

    #[test]
    fn render_ok_is_the_verbatim_pinning_of_green() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => panic!("全绿语料跑不出报表"),
        };
        assert_eq!(r.render().as_str(), RENDER_OK);
        assert_eq!(
            RENDER_OK,
            "cp-abort corpus=14 accept=8 reject=6 complete=1 warned=2 tail=2 aborted=3 match=14 rate=1000ppm\n"
        );
        assert!(RENDER_OK.ends_with("rate=1000ppm\n"));
        assert_eq!(RENDER_OK.lines().count(), 1);
        let inj = match run_with(Some((0, Fault::Code))) {
            Ok(v) => v,
            Err(_) => panic!("注入报表跑不出来"),
        };
        assert_eq!(inj.render().as_str(), concat!(
            "cp-abort corpus=14 accept=7 reject=7 complete=0 warned=2 tail=2 aborted=3 match=13 rate=929ppm\n"
        ));
        assert_ne!(inj.render().as_str(), RENDER_OK);
    }

    #[test]
    fn run_with_injection_moves_the_rate_off_a_thousand() {
        let green = match run() {
            Ok(v) => v,
            Err(_) => panic!("全绿报表缺失"),
        };
        let inj = match run_with(Some((0, Fault::Code))) {
            Ok(v) => v,
            Err(_) => panic!("注入报表缺失"),
        };
        assert_eq!(inj.matched, green.matched - 1);
        assert_eq!(inj.accept, green.accept - 1);
        assert_eq!(inj.reject, green.reject + 1);
        assert_eq!(inj.complete, green.complete - 1);
        assert_eq!(inj.warned, green.warned);
        assert_eq!(inj.rate(), 929);
        assert_eq!(inj.gate(), Err(CP_ECOUNT));
        assert!(!inj.green());
        assert_eq!(inj.total, green.total);
        // 注入"本来就期望的那一种"缺陷：matched 不掉——它是算出来的，不是硬编码。
        let same = match run_with(Some((8, Fault::Estate))) {
            Ok(v) => v,
            Err(_) => panic!("注入报表缺失"),
        };
        assert_eq!(same.matched, green.matched);
        assert_eq!(same.rate(), 1000);
        assert_eq!(same.gate(), Ok(()));
        // 把红槽说成另一种缺陷：accept 不变，matched 仍掉一格。
        let wrong = match run_with(Some((8, Fault::Value))) {
            Ok(v) => v,
            Err(_) => panic!("注入报表缺失"),
        };
        assert_eq!(wrong.accept, green.accept);
        assert_eq!(wrong.matched, green.matched - 1);
        assert_eq!(wrong.rate(), 929);
        // 注入 Ok 到红槽：该槽不再计入中止档的绿数。
        let heal = match run_with(Some((8, Fault::Ok))) {
            Ok(v) => v,
            Err(_) => panic!("注入报表缺失"),
        };
        assert_eq!(heal.accept, green.accept + 1);
        assert_eq!(heal.aborted, green.aborted + 1);
        assert_eq!(heal.matched, green.matched - 1);
        assert_eq!(heal.zero_cells() + heal.nonzero_cells(), heal.accept);
    }

    #[test]
    fn run_with_out_of_range_slot_reports_the_slot_code() {
        assert_eq!(run_with(Some((CORPUS, Fault::Code))).err(), Some(CP_ESLOT));
        assert_eq!(run_with(Some((CORPUS + 1, Fault::Ok))).err(), Some(CP_ESLOT));
        assert_eq!(run_with(Some((usize::MAX, Fault::Bal))).err(), Some(CP_ESLOT));
        assert!(run_with(Some((CORPUS - 1, Fault::Code))).is_ok());
        assert!(run_with(Some((0, Fault::Code))).is_ok());
        // None 就是不注入：报表与 run() 逐字段相同。
        let plain = match run_with(None) {
            Ok(v) => v,
            Err(_) => panic!("无注入报表跑不出来"),
        };
        let green = match run() {
            Ok(v) => v,
            Err(_) => panic!("全绿报表跑不出来"),
        };
        assert_eq!(plain.total, green.total);
        assert_eq!(plain.matched, green.matched);
        assert_eq!(plain.accept, green.accept);
        assert_eq!(plain.reject, green.reject);
        assert_eq!(plain.complete, green.complete);
        assert_eq!(plain.warned, green.warned);
        assert_eq!(plain.tail, green.tail);
        assert_eq!(plain.aborted, green.aborted);
        assert_eq!(plain.rate(), green.rate());
        assert_eq!(plain.gate(), green.gate());
        assert_eq!(plain.render().as_str(), green.render().as_str());
        assert_eq!(run().map(|r| r.total), Ok(CORPUS as u32));
    }

    // ---- 阶段闸门：open → stat → copy → werr → cerr → pres → exit ----

    #[test]
    fn gate_starts_empty_and_shows_its_three_fields() {
        let g = Gate::new();
        assert_eq!(g.steps(), 0);
        assert_eq!(g.last(), -1);
        assert!(!g.closed());
    }

    #[test]
    fn gate_admits_any_stage_as_the_first_step() {
        // 本卡闸门拦的是**回退**，不是"首步必须落在 open"。任何一闸都能作
        // 首步落地，落地后 last 立刻变成那一闸的序——所以首步本身不是一张
        // 通行证，轨迹才是。
        let mut i = 0usize;
        while i < GATE_LINE.len() {
            let s = match GATE_LINE.get(i) { Some(v) => *v, None => panic!("纵线越界") };
            let mut g = Gate::new();
            assert_eq!(g.step(s), Ok(()), "首步落在 {:?} 被拒", s);
            assert_eq!(g.steps(), 1);
            assert_eq!(g.last(), s.order() as i32);
            assert!(!g.closed());
            i += 1;
        }
        assert_eq!(GATE_LINE.len(), 7);
    }

    #[test]
    fn gate_walk_of_the_line_counts_every_observation() {
        let mut g = Gate::new();
        let mut i = 0usize;
        while i < GATE_LINE.len() {
            let s = match GATE_LINE.get(i) { Some(v) => *v, None => panic!("纵线越界") };
            assert_eq!(g.step(s), Ok(()));
            assert_eq!(g.steps(), (i as u32) * 2 + 1);
            assert_eq!(g.last(), s.order() as i32);
            // 同一闸重踩不算回退：复制循环里同一目的件会被反复观测。
            assert_eq!(g.step(s), Ok(()));
            assert_eq!(g.steps(), (i as u32) * 2 + 2);
            assert_eq!(g.last(), s.order() as i32);
            i += 1;
        }
        assert_eq!(g.steps(), 14);
        assert_eq!(g.last(), Stage::Exit.order() as i32);
        assert_eq!(g.close(), Ok(14));
    }

    #[test]
    fn gate_rejects_backward_steps_without_moving_the_trace() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::ClErr), Ok(()));
        let before_steps = g.steps();
        let before_last = g.last();
        // 已经判死在 close 段，又声称停在写段：拿文案倒推阶段的那类申报。
        assert_eq!(g.step(Stage::Werr), Err(CP_ESTAGE));
        assert_eq!(g.step(Stage::Copy), Err(CP_ESTAGE));
        assert_eq!(g.step(Stage::Open), Err(CP_ESTAGE));
        assert_eq!(g.steps(), before_steps, "被拒的步不该计入轨迹");
        assert_eq!(g.last(), before_last, "被拒的步不该改写 last");
        assert!(!g.closed());
        // 被拒之后仍可重踩同闸或继续前进。
        assert_eq!(g.step(Stage::ClErr), Ok(()));
        assert_eq!(g.step(Stage::Pres), Ok(()));
        assert_eq!(g.steps(), before_steps + 2);
        assert_eq!(g.last(), Stage::Pres.order() as i32);
    }

    #[test]
    fn gate_forward_jump_is_admitted_and_visible_in_the_count() {
        // 跳级不会被拒，但段数与 last 的差会把跳过的闸露出来：走了 2 步却
        // 停在第 7 闸，说明中间五闸没有观测。
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Open), Ok(()));
        assert_eq!(g.step(Stage::Exit), Ok(()));
        assert_eq!(g.last(), Stage::Exit.order() as i32);
        assert_eq!(g.steps(), 2);
        assert_ne!(g.steps(), g.last() as u32 + 1);
        // 而沿纵线走到同一闸，段数正好等于序 +1。
        let mut full = Gate::new();
        let mut i = 0usize;
        while i < Stage::Exit.order() as usize + 1 {
            let s = match GATE_LINE.get(i) { Some(v) => *v, None => panic!("纵线越界") };
            assert_eq!(full.step(s), Ok(()));
            i += 1;
        }
        assert_eq!(full.steps(), 7);
        assert_eq!(full.last(), full.steps() as i32 - 1);
    }

    #[test]
    fn gate_close_happens_once_and_then_every_step_is_refused() {
        let mut g = Gate::new();
        assert_eq!(g.close(), Ok(0));
        assert!(g.closed());
        assert_eq!(g.close(), Err(CP_ESTATE));
        assert_eq!(g.step(Stage::Exit), Err(CP_ESTATE));
        assert_eq!(g.step(Stage::Open), Err(CP_ESTATE));
        assert_eq!(g.steps(), 0);
        assert_eq!(g.last(), -1);
        // 关闸后的拒绝是"状态错"，不再是"阶段错"：闸已经不再是观测窗口。
        let mut walked = Gate::new();
        let mut i = 0usize;
        while i < GATE_LINE.len() {
            let s = match GATE_LINE.get(i) { Some(v) => *v, None => panic!("纵线越界") };
            assert_eq!(walked.step(s), Ok(()));
            i += 1;
        }
        assert_eq!(walked.close(), Ok(7));
        assert_eq!(walked.step(Stage::Open), Err(CP_ESTATE));
        assert_eq!(walked.steps(), 7);
        assert_eq!(walked.last(), Stage::Exit.order() as i32);
    }

    #[test]
    fn gate_walk_of_every_live_slot_lands_on_its_claimed_stage() {
        let mut i = 0usize;
        while i < CORPUS {
            let c = slot(i);
            let o = c.actual();
            let mut g = Gate::new();
            let mut walked = 0usize;
            while walked <= o.stage.order() as usize {
                let s = match GATE_LINE.get(walked) { Some(v) => *v, None => panic!("纵线越界") };
                assert_eq!(g.step(s), Ok(()));
                walked += 1;
            }
            assert_eq!(g.steps(), o.stage.order() as u32 + 1, "槽 {} 的闸距不对", i);
            assert_eq!(g.last(), o.stage.order() as i32);
            // 落闸之前的闸在这一槽上都是回退（CP_ESTAGE）；落闸之后的仍是前进。
            let mut b = 0usize;
            while b < GATE_LINE.len() {
                let s = match GATE_LINE.get(b) { Some(v) => *v, None => panic!("纵线越界") };
                if s.order() < o.stage.order() {
                    assert_eq!(g.step(s), Err(CP_ESTAGE), "槽 {} 允许回跳到 {:?}", i, s);
                } else {
                    assert_eq!(g.step(s), Ok(()), "槽 {} 拦住了同闸重踩或前进", i);
                }
                b += 1;
            }
            // 回跳被吞、同闸与 forward 照收：整圈下来正好 8 步（纵线 7 步 + 落闸重踩 1 次）。
            assert_eq!(g.close(), Ok(8), "槽 {} 的整圈步数不对", i);
            assert_eq!(g.step(Stage::Open), Err(CP_ESTATE), "关闸后仍放行");
            i += 1;
        }
        assert!(gate_ok());
    }

    // ---- 账本：登记、对账、冻结、重放 ----

    #[test]
    fn ledger_new_is_empty_but_still_has_a_fingerprint() {
        let l = Ledger::new();
        assert_eq!(l.len(), 0);
        assert!(l.is_empty());
        assert_eq!(l.ok_count(), 0);
        assert_eq!(l.err_count(), 0);
        assert_eq!(l.rejects(), 0);
        assert!(!l.is_frozen());
        assert_eq!(l.root(), 0);
        assert!(l.sample(0).is_none());
        assert_ne!(l.seal(), 0, "空账本的 FNV 基不该被当成指纹");
        assert_eq!(l.audit(), Err(CP_EBAL), "空账本不该通过对账");
    }

    #[test]
    fn ledger_register_demands_the_exact_next_slot() {
        let mut l = Ledger::new();
        assert_eq!(
            l.register(Sample { slot: 0, op: OP_OPEN, ok: true, code: 0, value: 14 }),
            Ok(())
        );
        assert_eq!(l.len(), 1);
        assert_eq!(l.rejects(), 0);
        // 插队：len=1 时报 slot 2 一律拒。
        assert_eq!(
            l.register(Sample { slot: 2, op: OP_OPEN, ok: true, code: 0, value: 8 }),
            Err(CP_ESLOT)
        );
        // 回头补登同样被拒：账本不给人补洞的机会。
        assert_eq!(
            l.register(Sample { slot: 0, op: OP_OPEN, ok: true, code: 0, value: 8 }),
            Err(CP_ESLOT)
        );
        assert_eq!(l.len(), 1, "被拒的登记不该涨格数");
        assert_eq!(l.rejects(), 2);
        let got = match l.sample(0) { Some(v) => v, None => panic!("首格丢了") };
        assert_eq!(got.slot, 0);
        assert_eq!(got.op, OP_OPEN);
        assert!(got.ok);
        assert_eq!(got.code, 0);
        assert_eq!(got.value, 14);
        assert!(l.sample(1).is_none(), "被拒的格子不该能被读出来");
    }

    #[test]
    fn ledger_out_of_range_slot_is_refused_and_counted() {
        // 判序是"冻结 → 插队 → 越界"：len=0 时报 slot 14 先撞的是插队那条。
        let mut l = Ledger::new();
        assert_eq!(
            l.register(Sample { slot: SAMPLES as u8, op: OP_CORPUS, ok: true, code: 0, value: 0 }),
            Err(CP_ESLOT)
        );
        assert_eq!(l.register(Sample { slot: u8::MAX, op: OP_CORPUS, ok: false, code: 1, value: 0 }), Err(CP_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.len(), 0);
        // 满账之后，"正好下一格"（slot == len == 14）才真正走到越界那一条。
        let mut full = build_ledger();
        assert_eq!(full.len(), SAMPLES);
        assert_eq!(
            full.register(Sample { slot: SAMPLES as u8, op: OP_CORPUS, ok: true, code: 0, value: 14 }),
            Err(CP_ESLOT)
        );
        assert_eq!(full.len(), SAMPLES);
        assert_eq!(full.rejects(), 1);
        assert_eq!(full.audit(), Ok(()), "对账不看拒绝数——见篡改那条");
    }

    #[test]
    fn frozen_ledger_refuses_even_a_well_formed_row() {
        // 冻结优先：未冻结时完全合法的登记（slot == len 且没越界），冻结后被拒。
        let mut half = Ledger::new();
        assert_eq!(
            half.register(Sample { slot: 0, op: OP_OPEN, ok: true, code: 0, value: 14 }),
            Ok(())
        );
        let root = match half.freeze() {
            Ok(v) => v,
            Err(_) => panic!("三格都填不满的账本冻结不了"),
        };
        assert!(half.is_frozen());
        assert_eq!(half.root(), root);
        assert_eq!(
            half.register(Sample { slot: 1, op: OP_STAT, ok: true, code: 0, value: 8 }),
            Err(CP_ESLOT)
        );
        assert_eq!(half.len(), 1);
        assert_eq!(half.rejects(), 1);
        // 二次冻结走的是另一条拒绝：CP_EBAL，同样计数。
        assert_eq!(half.freeze(), Err(CP_EBAL));
        assert_eq!(half.rejects(), 2);
        assert_eq!(half.root(), root, "二次冻结不该改写已封的根");
        assert!(half.sample(1).is_none(), "被冻结挡下的格子不该存在");
        assert_eq!(half.audit(), Err(CP_EBAL), "只登了一格的账本过不了配平");
    }

    #[test]
    fn count_cell_maps_a_result_onto_a_row() {
        let good = count_cell(3, OP_COPY, Ok(7));
        assert_eq!(good.slot, 3);
        assert_eq!(good.op, OP_COPY);
        assert!(good.ok);
        assert_eq!(good.code, 0);
        assert_eq!(good.value, 7);
        let bad = count_cell(4, OP_WERR, Err(CP_ECOUNT));
        assert_eq!(bad.slot, 4);
        assert_eq!(bad.op, OP_WERR);
        assert!(!bad.ok);
        assert_eq!(bad.code, CP_ECOUNT);
        assert_eq!(bad.value, 0, "败格必须归零，否则错误码会被当成观测值");
        let blank = Sample::blank();
        assert_eq!(blank.slot, 0);
        assert_eq!(blank.op, 0);
        assert!(!blank.ok);
        assert_eq!(blank.code, 0);
        assert_eq!(blank.value, 0);
    }

    #[test]
    fn build_ledger_lays_out_eight_counts_then_six_defect_codes() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), ACCEPT);
        assert_eq!(l.err_count(), REJECT);
        assert_eq!(l.rejects(), 0, "建账本身不该有拒绝");
        assert_eq!(l.audit(), Ok(()));
        let want_vals = [14u32, 8, 6, 1, 2, 2, 3, 929];
        let want_ops: [u8; 8] =
            [OP_CORPUS, OP_CORPUS, OP_CORPUS, OP_OPEN, OP_STAT, OP_PRES, OP_WERR, OP_REPLAY];
        let mut i = 0usize;
        while i < 8 {
            let s = match l.sample(i) { Some(v) => v, None => panic!("账本第 {} 格缺失", i) };
            assert_eq!(s.slot as usize, i);
            assert_eq!(s.op, match want_ops.get(i) { Some(v) => *v, None => 0 });
            assert!(s.ok, "前 8 格是计数，不该是败格");
            assert_eq!(s.code, 0);
            assert_eq!(s.value, match want_vals.get(i) { Some(v) => *v, None => 0 });
            i += 1;
        }
        assert_eq!(want_vals, VALS, "账本值表与 VALS 是同一份陈述");
        while i < SAMPLES {
            let s = match l.sample(i) { Some(v) => v, None => panic!("账本第 {} 格缺失", i) };
            assert_eq!(s.slot as usize, i);
            assert_eq!(s.op, OP_CERR);
            assert!(!s.ok, "后 6 格是缺陷，不该是成格");
            assert_eq!(s.value, 0);
            assert_eq!(
                Some(s.code),
                match REJECT_CODES.get(i - ACCEPT) { Some(v) => Some(*v), None => None },
                "第 {} 格的缺陷码落错了位置",
                i
            );
            i += 1;
        }
        assert_eq!(l.sample(SAMPLES - 1).map(|s| s.slot), Some(13));
        assert!(l.sample(SAMPLES).is_none());
    }

    #[test]
    fn audit_names_the_exact_cell_that_lied() {
        // 成格值偏离 VALS。
        let mut a = build_ledger();
        if let Some(r) = a.rows.get_mut(0) { r.value = 15; }
        assert_eq!(a.audit(), Err(CP_EVALUE));
        // 成格的红档率被改一个 ppm。
        let mut b = build_ledger();
        if let Some(r) = b.rows.get_mut(7) { r.value = 930; }
        assert_eq!(b.audit(), Err(CP_EVALUE));
        // 成格却带错误码。
        let mut c = build_ledger();
        if let Some(r) = c.rows.get_mut(2) { r.code = CP_ECOUNT; }
        assert_eq!(c.audit(), Err(CP_EVALUE));
        // 败格值没归零。
        let mut d = build_ledger();
        if let Some(r) = d.rows.get_mut(8) { r.value = 7; }
        assert_eq!(d.audit(), Err(CP_EVALUE));
        // 败格码换成了另一种缺陷码。
        let mut e = build_ledger();
        if let Some(r) = e.rows.get_mut(9) { r.code = CP_ESLOT; }
        assert_eq!(e.audit(), Err(CP_EVALUE));
        // 槽位号与格位错位。
        let mut f = build_ledger();
        if let Some(r) = f.rows.get_mut(4) { r.slot = 9; }
        assert_eq!(f.audit(), Err(CP_ESLOT));
        // 成败配平被破（把一格败格说成成格）。
        let mut g = build_ledger();
        if let Some(r) = g.rows.get_mut(13) { r.ok = true; }
        assert_eq!(g.audit(), Err(CP_EBAL));
        // 少一格也是配平破。
        let mut h = build_ledger();
        h.len = SAMPLES - 1;
        assert_eq!(h.audit(), Err(CP_EBAL));
        // 原始账本仍是绿的。
        assert_eq!(build_ledger().audit(), Ok(()));
    }

    #[test]
    fn poke_of_an_unaudited_field_survives_audit_but_not_replay() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => panic!("冻结失败"),
        };
        assert_eq!(l.replay(), Ok(root));
        // 篡改一格的操作码：audit 的对账面上没有 op 这一列，所以照样通过；
        // 但指纹把 op 折进去了，重放当场翻脸。
        if let Some(r) = l.rows.get_mut(6) { r.op = OP_COPY; }
        assert_eq!(l.audit(), Ok(()), "op 不在对账面上——这是审计的已知盲区");
        assert_ne!(l.seal(), root);
        assert_eq!(l.replay(), Err(CP_ESEAL));
        // 还原之后重放又绿：说明 replay 认的是内容，不是对象身份。
        if let Some(r) = l.rows.get_mut(6) { r.op = OP_WERR; }
        assert_eq!(l.seal(), root);
        assert_eq!(l.replay(), Ok(root));
    }

    #[test]
    fn rejects_are_folded_into_the_fingerprint_but_not_into_the_audit() {
        let mut l = build_ledger();
        let clean = l.seal();
        assert_eq!(l.rejects(), 0);
        l.rejects = 3;
        assert_eq!(l.audit(), Ok(()), "对账完全无视拒绝数");
        assert_ne!(l.seal(), clean, "拒绝数必须进指纹");
        assert_eq!(l.replay(), Err(CP_ESEAL), "篡改拒绝计数必须被重放抓到");
        l.rejects = 0;
        assert_eq!(l.seal(), clean);
        assert_eq!(l.replay(), Ok(clean));
    }

    #[test]
    fn ledger_seal_reads_every_field_of_every_counted_row() {
        let base = build_ledger();
        let clean = base.seal();
        let mut l = base;
        if let Some(r) = l.rows.get_mut(0) { r.slot = 9; }
        assert_ne!(l.seal(), clean, "槽位号不进指纹");
        l = base;
        if let Some(r) = l.rows.get_mut(0) { r.op = OP_COPY; }
        assert_ne!(l.seal(), clean, "操作码不进指纹");
        l = base;
        if let Some(r) = l.rows.get_mut(0) { r.ok = false; }
        assert_ne!(l.seal(), clean, "成败位不进指纹");
        l = base;
        if let Some(r) = l.rows.get_mut(0) { r.code = 1; }
        assert_ne!(l.seal(), clean, "错误码不进指纹");
        l = base;
        if let Some(r) = l.rows.get_mut(13) { r.value = 1; }
        assert_ne!(l.seal(), clean, "观测值不进指纹");
        l = base;
        l.len = SAMPLES - 1;
        assert_ne!(l.seal(), clean, "格数不进指纹");
        assert_eq!(base.seal(), clean, "同一本账两次指纹必须一致");
    }

    #[test]
    fn rows_beyond_the_len_never_enter_the_fingerprint() {
        let mut l = Ledger::new();
        let empty = l.seal();
        assert_ne!(empty, 0);
        if let Some(r) = l.rows.get_mut(5) {
            r.value = 4242;
            r.ok = true;
        }
        assert_eq!(l.seal(), empty, "len 之外的空格不该被折进指纹");
        assert!(l.sample(5).is_none());
        assert_eq!(
            l.register(Sample { slot: 0, op: OP_OPEN, ok: true, code: 0, value: 14 }),
            Ok(())
        );
        assert_ne!(l.seal(), empty);
        assert_eq!(l.err_count(), 0);
        assert_eq!(l.ok_count(), 1);
    }

    // ---- 释义（detail）与文案通道（message / warning）----

    #[test]
    fn detail_is_pinned_verbatim_for_the_eight_live_slots() {
        let mut i = 0usize;
        while i < ACCEPT {
            let c = slot(i);
            let w = match DETAIL_LIVE.get(i) { Some(v) => *v, None => panic!("钉版缺第 {} 行", i) };
            let d = detail(&c);
            assert_eq!(d.as_str(), w, "第 {} 槽释义漂移", i);
            assert!(d.ends_with('\n'));
            assert_eq!(d.matches('\n').count(), 1, "释义必须是一整行");
            assert_eq!(d.matches(" tool=").count(), 1);
            assert_eq!(d.matches(" stage=").count(), 1);
            assert_eq!(d.matches(" exit=").count(), 1);
            assert_eq!(d.matches(" bytes=").count(), 1);
            assert_eq!(d.matches(" abort=").count(), 1);
            assert_eq!(d.matches(" note=").count(), 1);
            assert!(d.starts_with(c.shape.dst), "释义开头就是目的路径");
            i += 1;
        }
        assert_eq!(DETAIL_LIVE.iter().filter(|s| s.contains("abort=true")).count(), 3);
        assert_eq!(DETAIL_LIVE.iter().filter(|s| s.contains("msg=\"")).count(), 7);
    }

    #[test]
    fn detail_renders_the_scene_not_the_claim() {
        let live = match DETAIL_LIVE.get(3) { Some(v) => *v, None => panic!("钉版缺失") };
        let mut i = ACCEPT;
        while i < CORPUS {
            let c = slot(i);
            assert_eq!(detail(&c).as_str(), live, "复刻槽 {} 的释义被申报带跑了", i);
            // 六条复刻申报都与活槽 3 的申报不同，释义却只有一句：detail() 里没有申报的位置。
            assert_ne!(c.claim, slot(3).claim, "复刻槽 {} 的申报与活槽撞车", i);
            i += 1;
        }
        // 把申报抹平成现场，释义一字不变——它本来就只读现场。
        let naked = Case { shape: slot(3).shape, claim: claim_of(&slot(3).actual()) };
        assert_eq!(detail(&naked).as_str(), live);
    }

    #[test]
    fn message_and_warning_split_the_two_channels() {
        let mut noisy = 0usize;
        let mut i = 0usize;
        while i < CORPUS {
            let c = slot(i);
            let o = c.actual();
            let m = message(&c.shape, &o);
            let w = warning(&c.shape, &o);
            if o.note == Note::Complete {
                assert!(m.is_empty() && w.is_empty(), "全绿槽不该有一行输出");
            } else if o.note == Note::WarnSparse || o.note == Note::CloseIgnored {
                assert!(!w.is_empty(), "告警档该有告警行");
                assert_eq!(m.as_str(), w.as_str(), "两条通道对告警档必须同文");
                noisy += 1;
            } else {
                assert!(!m.is_empty(), "中止档该有结论行");
                assert!(w.is_empty(), "中止文案不该走 warning 通道");
            }
            i += 1;
        }
        assert_eq!(noisy, 2, "语料里只有稀疏告警与 close-ignored 两句属告警档");
    }

    #[test]
    fn the_error_channel_carries_the_exact_tool_sentence() {
        let g = write_scene(Tool::Gnu);
        let go = define(&g);
        assert_eq!(go.note, Note::ErrorWriting);
        assert_eq!(
            message(&g, &go).as_str(),
            msg_gnu_error_writing(g.dst, ERR_ENOSPC).as_str()
        );
        let b = write_scene(Tool::Busybox);
        let bo = define(&b);
        assert_eq!(bo.note, Note::WriteError);
        assert_eq!(
            message(&b, &bo).as_str(),
            msg_bb_write_error(b.dst, ERR_ENOSPC).as_str()
        );
        // 文案换了，字节与退出码没换：两家都只报"写不下"这一件事。
        assert_eq!(bo.value, go.value);
        assert_eq!(bo.code, go.code);
        // close 段：GNU 有句子，busybox 把这一句整个吞掉。
        let gc = close_scene(Tool::Gnu);
        let gco = define(&gc);
        assert_eq!(gco.note, Note::CloseEnospc);
        assert_eq!(
            message(&gc, &gco).as_str(),
            msg_gnu_failed_to_close(gc.dst, ERR_ENOSPC).as_str()
        );
        assert!(warning(&gc, &gco).is_empty(), "failed to close 是结论不是告警");
        let bc = close_scene(Tool::Busybox);
        let bco = define(&bc);
        assert_eq!(bco.note, Note::CloseIgnored);
        assert_eq!(
            message(&bc, &bco).as_str(),
            msg_close_ignored(bc.dst, bc.deferred_lost()).as_str()
        );
        assert_eq!(warning(&bc, &bco).as_str(), message(&bc, &bco).as_str());
        // 静默截断是本卡自研句：不带 cp: 前缀，也不带 errno 尾巴。
        let naked = silent_scene();
        let no = define(&naked);
        assert_eq!(no.note, Note::SilentTrunc);
        let txt = message(&naked, &no);
        assert!(!txt.starts_with("cp: "));
        assert!(txt.contains("no error reported"));
        assert!(!txt.contains("No space left"), "自研句不该挂 errno 尾巴");
        assert!(txt.contains(naked.dst));
    }

    #[test]
    fn message_of_a_replica_follows_the_actual_note_not_the_claimed_one() {
        let mut i = ACCEPT;
        while i < CORPUS {
            let c = slot(i);
            let o = c.actual();
            let txt = message(&c.shape, &o);
            assert_eq!(
                txt.as_str(),
                msg_gnu_error_writing(c.shape.dst, ERR_ENOSPC).as_str(),
                "复刻槽 {} 的文案跟着申报跑了",
                i
            );
            let swapped = message(&c.shape, &Outcome { note: c.claim.note, ..o });
            if c.claim.note == o.note {
                assert_eq!(swapped.as_str(), txt.as_str());
            } else {
                assert_ne!(swapped.as_str(), txt.as_str(), "槽 {} 的坏 note 若不换句，这一维就没在实判", i);
            }
            i += 1;
        }
    }

    // ---- 四路注入与判据基元 ----

    /// 取注入通道一位（越界当作 false，宁可让测试炸在断言上）。
    fn channel(v: &[bool; 4], i: usize) -> bool {
        match v.get(i) { Some(x) => *x, None => false }
    }

    #[test]
    fn inject_four_reports_all_four_channels_green() {
        let out = inject_four();
        assert!(channel(&out, 0), "表路静默退化");
        assert!(channel(&out, 1), "判路静默退化");
        assert!(channel(&out, 2), "渲染路静默退化");
        assert!(channel(&out, 3), "账本路静默退化");
        assert_eq!(out.iter().filter(|x| **x).count(), 4);
        assert!(!out.iter().any(|x| !*x));
    }

    #[test]
    fn each_injection_channel_is_the_documented_conjunction() {
        let out = inject_four();
        assert_eq!(
            channel(&out, 0),
            name_ok() && enums_ok() && profile_ok() && shape_ok(),
            "表路的合取式与注入结果不一致"
        );
        assert_eq!(
            channel(&out, 1),
            define_ok() && abort_ok() && judge_ok() && matrix_ok(),
            "判路的合取式与注入结果不一致"
        );
        assert_eq!(
            channel(&out, 2),
            corpus_ok() && slots_ok() && reds_ok() && message_ok() && detail_ok(),
            "渲染路的合取式与注入结果不一致"
        );
        let mut l = build_ledger();
        let audited = l.audit() == Ok(());
        let sealed = match l.freeze() {
            Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
            Err(_) => false,
        };
        assert_eq!(
            channel(&out, 3),
            gate_ok() && ledger_ok() && audited && sealed,
            "账本路的合取式与注入结果不一致"
        );
        // 冻结一次是干净的：拒绝计数仍为 0，第二次冻结才会把它打破。
        assert_eq!(l.rejects(), 0);
        assert_eq!(l.freeze(), Err(CP_EBAL));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.audit(), Ok(()));
    }

    #[test]
    fn every_private_predicate_holds_on_the_shipped_card() {
        assert!(name_ok());
        assert!(enums_ok());
        assert!(profile_ok());
        assert!(holes_ok());
        assert!(shape_ok());
        assert!(define_ok());
        assert!(abort_ok());
        assert!(judge_ok());
        assert!(matrix_ok());
        assert!(corpus_ok());
        assert!(slots_ok());
        assert!(reds_ok());
        assert!(message_ok());
        assert!(detail_ok());
        assert!(gate_ok());
        assert!(ledger_ok());
    }

    #[test]
    fn inject_four_channels_are_pairwise_independent_in_coverage() {
        // 四路各自的谓词集合互不重叠：同一条谓词不该在两条通道里都被指望。
        let ch0 = ["name_ok", "enums_ok", "profile_ok", "shape_ok"];
        let ch1 = ["define_ok", "abort_ok", "judge_ok", "matrix_ok"];
        let ch2 = ["corpus_ok", "slots_ok", "reds_ok", "message_ok", "detail_ok"];
        let ch3 = ["gate_ok", "ledger_ok"];
        let mut total = 0usize;
        total += ch0.len();
        total += ch1.len();
        total += ch2.len();
        total += ch3.len();
        assert_eq!(total, 15, "注入分路的谓词清单与合取式对不上");
        assert_eq!(ch0.len() + ch1.len() + ch2.len() + ch3.len() + 1, 16, "holes_ok 只进 CP03，不进注入分路");
        let out = inject_four();
        assert_eq!(out.iter().filter(|x| **x).count(), 4);
    }

    // ---- 十三项自检 ----

    #[test]
    fn the_card_ships_thirteen_checks_and_all_are_green() {
        let s = run_unx11682_checks();
        assert_eq!(s.len(), 13);
        assert!(!s.is_empty());
        assert!(s.all_passed());
        assert_eq!(s.passed_count(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(s.tally(), (13usize, 0usize));
        assert!(!s.truncated());
        assert_eq!(s.domain, "unx11682_cp_enospc_abort");
        assert!(s.get(0).is_some());
        assert!(s.get(12).is_some());
        assert!(s.get(13).is_none());
        let mut seen = 0usize;
        for c in s.iter() {
            assert!(c.passed, "自检 {} 是红的", c.name);
            assert_eq!(c.detail, "static", "自检 {} 该走静态明细通道", c.name);
            assert!(c.name.starts_with("CP"), "自检名 {} 没落在本卡前缀上", c.name);
            seen += 1;
        }
        assert_eq!(seen, 13);
    }

    #[test]
    fn the_thirteen_check_names_are_pinned_verbatim() {
        let want: [&str; 13] = [
            "CP01-名称与段基址",
            "CP02-枚举与七闸",
            "CP03-挂载画像与稀疏几何",
            "CP04-形状合法域",
            "CP05-写回七闸纵线",
            "CP06-中止三元组归因",
            "CP07-申报对判",
            "CP08-矩阵统计",
            "CP09-语料与三份陈述",
            "CP10-槽位与红档显式",
            "CP11-文案逐字",
            "CP12-释义与阶段闸",
            "CP13-冻结重放四注入",
        ];
        let s = run_unx11682_checks();
        assert_eq!(s.len(), want.len());
        let mut i = 0usize;
        while i < want.len() {
            let w = match want.get(i) { Some(v) => *v, None => panic!("名字表越界") };
            let got = match s.get(i) { Some(v) => v, None => panic!("第 {} 条自检缺失", i + 1) };
            assert_eq!(got.name, w, "第 {} 条自检的名字对不上", i + 1);
            i += 1;
        }
        // 名字互不相同，且编号连续。
        let mut a = 0usize;
        while a < want.len() {
            let mut b = a + 1;
            while b < want.len() {
                if let (Some(x), Some(y)) = (want.get(a), want.get(b)) {
                    assert_ne!(x, y, "自检名重复：{} 与 {}", x, y);
                }
                b += 1;
            }
            a += 1;
        }
        assert_eq!(want.get(12), Some(&"CP13-冻结重放四注入"));
        assert_eq!(want.get(0), Some(&"CP01-名称与段基址"));
    }

    #[test]
    fn check_verdicts_track_the_underlying_predicates() {
        // 十三条自检的红绿由谓词决定：这里逐条比对，防止某条 add 传了常量 true。
        let s = run_unx11682_checks();
        let want: [bool; 13] = [
            name_ok(),
            enums_ok(),
            profile_ok() && holes_ok(),
            shape_ok(),
            define_ok(),
            abort_ok(),
            judge_ok(),
            matrix_ok(),
            corpus_ok(),
            slots_ok() && reds_ok(),
            message_ok(),
            detail_ok() && gate_ok(),
            ledger_ok() && inject_four().iter().all(|x| *x),
        ];
        let mut i = 0usize;
        while i < 13 {
            let got = match s.get(i) { Some(v) => v, None => panic!("第 {} 条自检缺失", i + 1) };
            assert_eq!(
                got.passed,
                match want.get(i) { Some(v) => *v, None => false },
                "第 {} 条自检的红绿与谓词不符",
                i + 1
            );
            i += 1;
        }
        assert!(want.iter().all(|x| *x), "有条谓词在本机上是红的");
    }

    #[test]
    fn cover_rejects_a_short_corpus() {
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(CORPUS - 1), Err(CP_ENOBS));
        assert_eq!(cover(ACCEPT), Err(CP_ENOBS));
        assert_eq!(cover(0), Err(CP_ENOBS));
        assert_eq!(cover(CORPUS + 1), Err(CP_ENOBS));
    }
}
