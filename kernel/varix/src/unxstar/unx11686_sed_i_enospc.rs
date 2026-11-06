//! UNX-F11686 sed -i 轮换 ENOSPC 原件保护判据
//!
//! `sed -i` 不是"改这个文件"，而是"**另写一个文件，再把名字换过来**"。这条
//! 轮换纵线（rotate）决定了本卡的全部难度：满盘时它红的方式和 `cp`/`dd` 完全
//! 不同——**一个字节都不许落在原件上，但原件的名字可能已经没了**。
//!
//! ```text
//! $ sed -i.bak 's/a/b/' cfg.conf     # 盘满
//! sed: couldn't write 1 output file to No space left on device
//! $ ls
//! cfg.conf.bak                        ← 原件改名成了备份，新件没写成，
//!                                        cfg.conf 这个名字已经不存在
//! ```
//!
//! 上面四行是本卡的心脏。GNU sed 对 `-i=SUFFIX` 的实现是**先 `rename(原件 →
//! 原件+SUFFIX)`，再建临时件写新内容**。`rename` 不占数据块，所以在满盘上
//! **照样成功**；失败发生在后面的写出段。于是"原件不动"这句安全承诺，在带备份
//! 后缀时必须重新读一遍：*内容*一个字没少（全在 `.bak` 里），*名字*却已经不
//! 在原地。运维脚本按 `[ -f cfg.conf ]` 判断"失败即原样"，在带后缀的轮换里
//! **必然读到假**。本卡把这件事拆成两个独立数字：`value` = 原件名下留档字节，
//! `backup_bytes` = 备份件里的字节，两者不许互相顶替。
//!
//! 另一半承诺是**临时清理**。GNU sed 在临时件里写，失败路径上必须把
//! `sedXXXXXX` 摘掉；busybox sed 的清理挂在 `rename` 成功之后，**失败时临时件
//! 就留在盘上了**。于是同一现场（写出 ENOSPC）在两个工具上是两个磁盘终态：
//! GNU 留 0 字节残痕，busybox 留 `written` 字节残痕。本卡把残痕做成 `leftover()`
//! 纯函数（只由"停在哪道闸 + 哪个工具"决定），并让它进判据——因为满盘之后的
//! `sedXXXXXX` 本身就在继续吃那点剩余空间，这是二次事故的正宗来源。
//!
//! 第三条轴是**inode 与硬链接**。轮换的本质是换 inode：新件替换旧件，指向旧
//! inode 的硬链接**留在旧内容上**。`sed -i` 之后 `ln` 出来的第二个名字不再是
//! 同一份数据。这正是 F11464（原位编辑正判据）钉的语义，本卡不重判它"换没换
//! 对"，只在 ENOSPC 这一侧要求：**没写成新件的时候，硬链接一个都不许断**
//! （旧 inode 还在原件名下）。硬链接数因此进 `Shape`，成为成功族里区分
//! `link-cut` 与 `complete` 的唯一依据。
//!
//! 第四条轴是**属性尾段**。sed 在 `rename` 之前把原件的 owner/mode 搬到临时件
//! 上（`fchown`/`fchmod`）。GNU 对这一步的失败**不撤回轮换**——名字照换，退出
//! 码照报 4；busybox 则直接放弃，把写完的临时件丢在原地。于是"码非 0"在这里
//! **不等于**"没改"，这是审计脚本最容易读反的一格，本卡把它单列一道闸。
//!
//! 退出码也是本卡的独立结论：GNU sed 的 I/O 类失败退 **4**（不是 1），用法错退
//! **2**，内部错退 **3**；busybox sed 一律退 **1**。本卡把"码 4"当 ENOSPC 轮换
//! 中止的锚位，把"码 2/1"当用法族锚位——把 cp 卡的 1/2 语义搬过来就是错位。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 九道闸，秩从 **0** 起算；`Gate` 只接受"前进到不小于上一秩"。
//!
//! `argv → open → backup → mktemp → write → close → attr → rename → exit`
//!
//! 1. `argv`（秩 0）：选项本身。`--follow-symlinks` 不带 `-i` 在这里就红
//!    （`can't follow symlink without -I/--in-place`），选项不认也在这里红。
//!    这一闸排最前是为了钉死：用法错的行**文件系统一次都没碰**，原件名字、
//!    备份、临时件三样全是初始态。把它放到 backup 之后就等于允许"用法错却
//!    已经把原件改名了"这种鬼话通过。
//! 2. `open`（秩 1）：源件 `open` + `fstat`。sed 先拿到原件的 fd 和 stat，
//!    后面所有改名/搬属性都以这份 stat 为准。源打不开（`ENOENT`/`EACCES`）
//!    在这里停，**原件不动**是唯一结论。
//! 3. `backup`（秩 2）：`-i=SUFFIX` 的 `rename(原件 → 原件+SUFFIX)`。**必须
//!    排在 mktemp 之前**——GNU 的实现就是这个顺序，而它正是"满盘上备份照样
//!    成功"的原因。反过来排，就会把"临时件都还没建、原件却已经没名了"这一
//!    格算不出来，`value==0 && leftover==0` 的现场凭空消失。
//! 4. `mktemp`（秩 3）：`mkstemp` 建临时件。建不出来（目录满/不可写）在这里
//!    停。这一闸是残痕的**起点**：秩 < 3 失败 ⇒ 盘上没有 `sedXXXXXX`，
//!    秩 ≥ 3 失败 ⇒ 可能有，此时清理责任才落到工具头上。
//! 5. `write`（秩 4）：把编辑结果写进临时件。收到 ENOSPC 就在这里停，
//!    `written < want` 是这一闸的专属指纹（延迟分配不在这里，见 close）。
//! 6. `close`（秩 5）：`fflush`/`fsync` 才爆的现场。**必须在 write 之后**：
//!    写都红了就不会走到 close；反过来 close 红的前提是写全绿（`written ==
//!    want`）。两闸一倒序，延迟分配的丢块永远记不到 close 头上。
//! 7. `attr`（秩 6）：owner/mode 搬迁。**必须在 rename 之前**（GNU 的实现如此），
//!    且在 close 之后（没有落定的数据谈什么属性）。这一闸的两种工具分歧
//!    （撤回轮换 vs 照常落位）只能靠"它排在 rename 前"才表达得出来。
//! 8. `rename`（秩 7）：临时件换成原件的名字。目录项要新块时这一步**真的会
//!    ENOSPC**（数据块满不等于目录能扩），此时新件已完整写完、原件也已改名
//!    或不改名——本卡把它单列一闸，因为"写成功了却落不了位"既不是写错也不是
//!    归因缺失，混进 write 就把目录面和数据面搅成一锅。
//! 9. `exit`（秩 8）：退出码是前面所有闸的聚合，只能最后算。任何"码 0"的申报
//!    都要在这里回核：**backup/mktemp/write/close/rename 任一段红过，码不许 0**；
//!    `attr` 一维例外（GNU 照常落位，但码是 4 不是 0）。
//!
//! 为什么这条线不能倒：本卡的判据全部建立在"停在哪一闸 ⇒ 盘上是哪三态"这个
//! 映射上。闸序一乱，`value`（原件名下字节）、`backup_bytes`（备份件字节）、
//! `leftover`（临时件残痕）三个数就同时失去唯一解，"原件不动"与"临时清理"两半
//! 承诺都无从判起。
//!
//! ## 关键差异（本卡深度所在）
//!
//! * **与 F11681（总锚）**：总锚只给那一次 `write()`/`fsync()`/`close()` 的
//!   (返回值, errno) 对。本卡不重判 errno 与返回值是否成对、不判短写与满失败的
//!   区分、不判 EFBIG/EDQUOT/EBADF/EIO 的分档；本卡的 `errno_seen` 只是把总锚
//!   结论**引进来**。总锚不知道的事情——原件名字还在不在、备份里有多少字节、
//!   `sedXXXXXX` 清没清、退出码是 4 还是 2——全归本卡。
//! * **与 F11682（cp 中止）**：cp 是"往目的件里写"，满盘就留一个半件，
//!   源件永远不动。sed -i 是"写一个件再把名字换过来"，**满盘时源件的名字本身
//!   就是被操作对象**（带后缀时被 rename 走）。两卡对同一次满盘给的数字不同族：
//!   cp 的 `value` 是"目的件留了多少"，本卡的 `value` 是"原件名下还剩多少"。
//!   把 cp 的语义搬进本卡，就会得出"失败 ⇒ 目的件半截 ⇒ 字节在 0 与 size 之间"
//!   这种在本卡根本不存在的中间态。
//! * **与 F11685（sort 临时件）**：sort 的临时件是**中途副产品**，满了就报
//!   `sort: No space left on device` 且退 **2**（它把临时盘满当用法级硬错），
//!   原件从来没被"换名"过。本卡的临时件是**目的件的前身**，满盘后原名字可能
//!   消失，码是 4。两卡的临时件字节数不许合并统计。
//! * **与 F11688（touch/mkdir）**：那一卡管"建一个空东西"（0 字节也可能 28）；
//!   本卡的 mktemp 闸虽然同为建件，但建出来之后必须写满全量，判据落在"残痕
//!   清理"而不是"零字节留痕"。
//! * **与 F11690（rename）**：那一卡判"同盘 rename 不吃数据块"。本卡的 backup
//!   闸与 rename 闸都是 `rename(2)`，但本卡**不重判**它的空间语义（只在目录
//!   真的扩不出来时接受 ENOSPC），本卡判的是这次 rename **把原件的名字移走了**
//!   这一后果归谁。
//! * **与 F11464（联签面）**：F11464 判"原位替换语义账"（新件落位、inode 更换、
//!   权限继承）——正判据。本卡判"轮换在满盘上失败时，原件保护与临时清理是否
//!   兑现"——反判据。两卡共用同一套 inode/属性语义，本卡**不得**把成功族的
//!   落位语义判成自己的功劳，只在 `hardlinks > 1` 成功格上挂 `link-cut` 这一
//!   呈现档，与 F11464 的 inode 账对接。
//! * **本卡的排他职责**：① 原件名下留档字节（`value`）；② 备份件字节
//!   （`backup_bytes`）；③ 临时件残痕字节（`leftover`）；④ 三个数与闸序的
//!   唯一映射；⑤ GNU 码 4 / busybox 码 1 这一工具面差异；⑥ `-i=SUFFIX` 的
//!   "先改名后写"造成的"内容在、名不在"这一格。
//! * **谁判死谁放行**：现场是 ENOSPC、停在 mktemp/write/close/rename 任一闸、
//!   码与工具同族、且三字节数与闸序自洽 → 放行。现场是 ENOSPC 却申报"码 0 且
//!   原件原样"（busybox 属性撤回除外，那一格由本卡显式标注为"未落位"），或
//!   申报 `written < want` 却停在 close 闸，或 GNU 失败却留残痕 → 判死。
//!
//! 判据六维：结束阶段（gate）、退出码（code）、原件名下字节（value）、文案落点
//! （note）、轮换归因（default：是否 ENOSPC 打断了轮换）、成败申报（estate）。
//! 判序固定：状态 → 阶段 → 退出码 → 字节 → 注解 → 归因。
//!
//! 语料 14 槽：8 槽申报正确（落位 2、有话 1、属性 1、用法 1、中止 3），6 槽复刻
//! 同一"无后缀轮换写出中途满盘"现场、各挂一维错报（estate/stage/code/value/
//! note/default）。账本 14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码，冻结后重放
//! 同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11686）。
pub const SEG_BASE: u32 = 0x6d320;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 申报正确的槽数。
pub const ACCEPT: usize = 8;

/// 申报错误的槽数。
pub const REJECT: usize = 6;

/// 档位数。
pub const TIER_COUNT: usize = 4;

// ---------------------------------------------------------------------------
// errno（只引进 F11681 的结论，本卡不重判）
// ---------------------------------------------------------------------------

/// 设备无空间（`No space left on device`），本卡唯一的轮换中止源。
pub const ERR_ENOSPC: i32 = 28;
/// 没有那个文件或目录（`No such file or directory`），源件打不开的常见邻居。
pub const ERR_ENOENT: i32 = 2;
/// 权限不足（`Permission denied`），源不可读或临时目录不可写。
pub const ERR_EACCES: i32 = 13;
/// 操作不允许（`Operation not permitted`），fchown 失败的落点。
pub const ERR_EPERM: i32 = 1;
/// 配额超限（`Quota exceeded`），红但不是 ENOSPC。
pub const ERR_EDQUOT: i32 = 122;
/// 设备错（`Input/output error`），也不是 ENOSPC。
pub const ERR_EIO: i32 = 5;
/// 没有错误。
pub const ERR_NONE: i32 = 0;

/// 合法 errno 集合的大小（0/28/2/13/1/122/5 七个）。
pub const ERRNO_SET_SIZE: usize = 7;

// ---------------------------------------------------------------------------
// 退出码（sed 家族：GNU 的 I/O 错是 4，不是 cp 的 1）
// ---------------------------------------------------------------------------

/// 全绿。
pub const EXIT_OK: u32 = 0;
/// busybox sed 的一切失败。
pub const EXIT_BB: u32 = 1;
/// GNU sed 用法错（`sed --help` 手册里的 2）。
pub const EXIT_USAGE: u32 = 2;
/// GNU sed 内部错（本卡只用它做"不许出现在判据里"的反面锚）。
pub const EXIT_INTERNAL: u32 = 3;
/// GNU sed 无法处理输入/输出（本卡的中止锚位）。
pub const EXIT_IO: u32 = 4;

/// 单个字节维的合法上界。
pub const SIZE_MAX: u32 = 0x0800_0000;

/// 临时件模板名（GNU sed 用 `mkstemp`，模板落在**输出件同目录**）。
pub const TMP_TEMPLATE: &str = "sedXXXXXX";

// ---------------------------------------------------------------------------
// 属性搬迁位（sed -i 在 rename 之前做的事）
// ---------------------------------------------------------------------------

/// 不搬任何属性。
pub const A_NONE: u8 = 0;
/// 搬 owner/group（`fchown`）。
pub const A_OWNER: u8 = 1;
/// 搬权限位（`fchmod`）。
pub const A_MODE: u8 = 2;
/// 搬迁全集。
///
/// 注意**没有 mtime 这一维**：`sed -i` 写的是新 inode，新件的时间戳是写入时刻，
/// GNU 从不回填原件的 mtime。把 `cp -p` 的时间戳语义搬进本卡是错位（那一维归
/// F11464 的权限继承账，且它的结论是"不保留"）。
pub const A_ALL: u8 = A_OWNER | A_MODE;
/// 非 root 时的实际上限（`fchown` 由内核拒绝，GNU 跳过）。
pub const A_NONROOT: u8 = A_MODE;
/// 属性位个数。
pub const A_BIT_COUNT: usize = 2;

// ---------------------------------------------------------------------------
// 错误码（13 个，SEG_BASE + 0x01..0x0d）
// ---------------------------------------------------------------------------

/// 阶段申报不符。
pub const SI_ESTAGE: u32 = SEG_BASE + 0x01;
/// 退出码申报不符。
pub const SI_ECODE: u32 = SEG_BASE + 0x02;
/// 原件名下字节申报不符。
pub const SI_EVALUE: u32 = SEG_BASE + 0x03;
/// 文案落点申报不符。
pub const SI_ENOTE: u32 = SEG_BASE + 0x04;
/// 轮换归因申报不符。
pub const SI_EDEFAULT: u32 = SEG_BASE + 0x05;
/// 归因破断：文案、三字节数与退出码不同族。
pub const SI_EABORT: u32 = SEG_BASE + 0x06;
/// 成败互报。
pub const SI_ESTATE: u32 = SEG_BASE + 0x07;
/// 计数不符。
pub const SI_ECOUNT: u32 = SEG_BASE + 0x08;
/// 槽位非法。
pub const SI_ESLOT: u32 = SEG_BASE + 0x09;
/// 指纹不符。
pub const SI_ESEAL: u32 = SEG_BASE + 0x0a;
/// 无观测。
pub const SI_ENOBS: u32 = SEG_BASE + 0x0b;
/// 矩阵不平。
pub const SI_EMATRIX: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const SI_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码（账本格的操作归类：对应九道闸 + 语料 + 重放）
// ---------------------------------------------------------------------------

/// 命令行/选项段。
pub const OP_ARGV: u8 = 0x01;
/// 源件打开与 fstat 段。
pub const OP_OPEN: u8 = 0x02;
/// 备份改名段。
pub const OP_BACKUP: u8 = 0x03;
/// 临时件创建段。
pub const OP_MKTEMP: u8 = 0x04;
/// 轮换写出段。
pub const OP_WRITE: u8 = 0x05;
/// 落定（flush/fsync）段。
pub const OP_CLOSE: u8 = 0x06;
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
        s.push_str(dec_text((v as i64).unsigned_abs() as u32).as_str());
        s
    } else {
        dec_text(v as u32)
    }
}

/// 布尔文本。
fn bool_text(v: bool) -> &'static str {
    if v {
        "yes"
    } else {
        "no"
    }
}

/// 段基址的十六进制文本（判据名里回显用）。
fn seg_text() -> String {
    hex_text(SEG_BASE, 5)
}

// ---------------------------------------------------------------------------
// strerror
// ---------------------------------------------------------------------------

/// errno → glibc 逐字文本。
pub fn strerror(code: i32) -> &'static str {
    match code {
        ERR_ENOSPC => "No space left on device",
        ERR_ENOENT => "No such file or directory",
        ERR_EACCES => "Permission denied",
        ERR_EPERM => "Operation not permitted",
        ERR_EDQUOT => "Quota exceeded",
        ERR_EIO => "Input/output error",
        _ => "",
    }
}

/// 本卡只承认七个 errno。
pub fn errno_legal(code: i32) -> bool {
    code == ERR_NONE
        || code == ERR_ENOSPC
        || code == ERR_ENOENT
        || code == ERR_EACCES
        || code == ERR_EPERM
        || code == ERR_EDQUOT
        || code == ERR_EIO
}

// ---------------------------------------------------------------------------
// 文案（逐字复刻真实报错）
// ---------------------------------------------------------------------------

/// GNU/busybox 的诊断前缀。
fn sed_prefix(tool: Tool) -> String {
    let mut s = String::from("sed: ");
    let _ = tool;
    s
}

/// GNU sed：读不到源件。
/// `sed: can't read cfg.conf: No such file or directory`
pub fn msg_gnu_cant_read(path: &str, code: i32) -> String {
    let mut s = String::from("sed: can't read ");
    s.push_str(path);
    s.push_str(": ");
    s.push_str(strerror(code));
    s
}

/// GNU sed：不认的长选项。
/// `sed: unrecognized option '--follow-symlink'`
pub fn msg_gnu_unrecognized(opt: &str) -> String {
    let mut s = String::from("sed: unrecognized option '");
    s.push_str(opt);
    s.push('\'');
    s
}

/// GNU sed：短选项非法。
/// `sed: invalid option -- 'Z'`
pub fn msg_gnu_invalid_short(c: &str) -> String {
    let mut s = String::from("sed: invalid option -- '");
    s.push_str(c);
    s.push('\'');
    s
}

/// GNU sed：`--follow-symlinks` 没配 `-i`。
/// `sed: can't follow symlink without -I/--in-place`
pub fn msg_gnu_follow_without_i() -> String {
    String::from("sed: can't follow symlink without -I/--in-place")
}

/// GNU sed：源不是普通件（`-i` 语义下无法轮换）。
/// `sed: couldn't edit /dev/stdin: not a regular file`
pub fn msg_gnu_not_regular(path: &str) -> String {
    let mut s = String::from("sed: couldn't edit ");
    s.push_str(path);
    s.push_str(": not a regular file");
    s
}

/// GNU sed：`-i=SUFFIX` 的备份改名失败。
/// `sed: can't rename cfg.conf to cfg.conf.bak: Permission denied`
pub fn msg_gnu_backup_fail(src: &str, bak: &str, code: i32) -> String {
    let mut s = String::from("sed: can't rename ");
    s.push_str(src);
    s.push_str(" to ");
    s.push_str(bak);
    s.push_str(": ");
    s.push_str(strerror(code));
    s
}

/// GNU sed：临时件建不出来。
/// `sed: couldn't open temporary file cfg.d/sedXXXXXX: No space left on device`
pub fn msg_gnu_mktemp(dir: &str, code: i32) -> String {
    let mut s = String::from("sed: couldn't open temporary file ");
    s.push_str(dir);
    s.push('/');
    s.push_str(TMP_TEMPLATE);
    s.push_str(": ");
    s.push_str(strerror(code));
    s
}

/// GNU sed：轮换写出失败（复数按 GNU 的 `%d output file%s` 模板展开）。
/// `sed: couldn't write 1 output file to No space left on device`
pub fn msg_gnu_write_fail(files: u32, code: i32) -> String {
    let mut s = String::from("sed: couldn't write ");
    s.push_str(dec_text(files).as_str());
    s.push_str(" output file");
    if files != 1 {
        s.push('s');
    }
    s.push_str(" to ");
    s.push_str(strerror(code));
    s
}

/// GNU sed：`close`/`fsync` 才爆。
/// `sed: couldn't close temporary file cfg.d/sedXXXXXX: No space left on device`
pub fn msg_gnu_close_fail(dir: &str, code: i32) -> String {
    let mut s = String::from("sed: couldn't close temporary file ");
    s.push_str(dir);
    s.push('/');
    s.push_str(TMP_TEMPLATE);
    s.push_str(": ");
    s.push_str(strerror(code));
    s
}

/// GNU sed：属性搬迁失败（owner）。
/// `sed: can't chown cfg.conf: Operation not permitted`
pub fn msg_gnu_chown(path: &str, code: i32) -> String {
    let mut s = String::from("sed: can't chown ");
    s.push_str(path);
    s.push_str(": ");
    s.push_str(strerror(code));
    s
}

/// GNU sed：属性搬迁失败（mode）。
/// `sed: can't chmod cfg.conf: Operation not permitted`
pub fn msg_gnu_chmod(path: &str, code: i32) -> String {
    let mut s = String::from("sed: can't chmod ");
    s.push_str(path);
    s.push_str(": ");
    s.push_str(strerror(code));
    s
}

/// GNU sed：改名落位失败。
/// `sed: can't rename cfg.d/sedXXXXXX to cfg.conf: No space left on device`
pub fn msg_gnu_rename_fail(tmp: &str, out: &str, code: i32) -> String {
    let mut s = String::from("sed: can't rename ");
    s.push_str(tmp);
    s.push_str(" to ");
    s.push_str(out);
    s.push_str(": ");
    s.push_str(strerror(code));
    s
}

/// busybox sed：不认的选项。
/// `sed: unrecognized option: 'Z'`
pub fn msg_bb_unrecognized(opt: &str) -> String {
    let mut s = String::from("sed: unrecognized option: '");
    s.push_str(opt);
    s.push('\'');
    s
}

/// busybox sed：打不开源件。
/// `sed: can't open file cfg.conf: No such file or directory`
pub fn msg_bb_cannot_open(path: &str, code: i32) -> String {
    let mut s = String::from("sed: can't open file ");
    s.push_str(path);
    s.push_str(": ");
    s.push_str(strerror(code));
    s
}

/// busybox sed：读源失败。
/// `sed: can't read cfg.conf: Is a directory`（体例与 GNU 同文，码不同）
pub fn msg_bb_cant_read(path: &str, code: i32) -> String {
    let mut s = String::from("sed: can't read ");
    s.push_str(path);
    s.push_str(": ");
    s.push_str(strerror(code));
    s
}

/// busybox sed：临时件建不出来。
/// `sed: can't create temp file for cfg.conf: No space left on device`
pub fn msg_bb_temp_fail(path: &str, code: i32) -> String {
    let mut s = String::from("sed: can't create temp file for ");
    s.push_str(path);
    s.push_str(": ");
    s.push_str(strerror(code));
    s
}

/// busybox sed：写出失败。
/// `sed: write error to cfg.d/sedXXXXXX: No space left on device`
pub fn msg_bb_write_error(path: &str, code: i32) -> String {
    let mut s = String::from("sed: write error to ");
    s.push_str(path);
    s.push_str(": ");
    s.push_str(strerror(code));
    s
}

/// busybox sed：改名失败。
/// `sed: can't rename cfg.d/sedXXXXXX to cfg.conf: No space left on device`
pub fn msg_bb_rename_fail(tmp: &str, out: &str, code: i32) -> String {
    let mut s = String::from("sed: can't rename ");
    s.push_str(tmp);
    s.push_str(" to ");
    s.push_str(out);
    s.push_str(": ");
    s.push_str(strerror(code));
    s
}

/// busybox sed：属性搬迁失败。
/// `sed: can't chmod cfg.conf: Permission denied`
pub fn msg_bb_chmod(path: &str, code: i32) -> String {
    let mut s = String::from("sed: can't chmod ");
    s.push_str(path);
    s.push_str(": ");
    s.push_str(strerror(code));
    s
}

/// 静默轮换：少了字节却没有报错——本卡判死。
pub fn msg_silent_rotate(path: &str, wrote: u32, want: u32) -> String {
    let mut s = String::from("sed: silent rotation on ");
    s.push_str(path);
    s.push_str(" wrote ");
    s.push_str(dec_text(wrote).as_str());
    s.push_str(" of ");
    s.push_str(dec_text(want).as_str());
    s.push_str(" bytes with no diagnostic");
    s
}

/// 临时件残痕：失败后盘上留着的 `sedXXXXXX`。
pub fn msg_residue_left(dir: &str, bytes: u32) -> String {
    let mut s = String::from("residue ");
    s.push_str(dir);
    s.push('/');
    s.push_str(TMP_TEMPLATE);
    s.push_str(" bytes=");
    s.push_str(dec_text(bytes).as_str());
    s
}

/// 归因破断：文案、三字节数、退出码不同族。
pub fn msg_rotate_broken(note: &str, value: u32, leftover: u32, code: u32) -> String {
    let mut s = String::from("sed: rotation attribution broken note=");
    s.push_str(note);
    s.push_str(" orig=");
    s.push_str(dec_text(value).as_str());
    s.push_str(" residue=");
    s.push_str(dec_text(leftover).as_str());
    s.push_str(" exit=");
    s.push_str(dec_text(code).as_str());
    s
}

/// 邻居 errno 造成的中止（红，但不是 ENOSPC）。
pub fn msg_foreign_stop(path: &str, code: i32) -> String {
    let mut s = String::from("sed: ");
    s.push_str(path);
    s.push_str(": ");
    s.push_str(strerror(code));
    s
}

// ---------------------------------------------------------------------------
// 工具面
// ---------------------------------------------------------------------------

/// 哪一套 `sed`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// GNU sed 4.9（in-place 走 mkstemp + rename）。
    Gnu,
    /// busybox sed（in-place 走 tempname + rename，清理挂在成功后）。
    Busybox,
}

impl Tool {
    /// 文本。
    pub fn text(self) -> &'static str {
        match self {
            Tool::Gnu => "gnu-sed",
            Tool::Busybox => "busybox-sed",
        }
    }

    /// 用法错退出码：GNU 2，busybox 1（busybox 不分用法族与文件族）。
    pub fn usage_exit(self) -> u32 {
        match self {
            Tool::Gnu => EXIT_USAGE,
            Tool::Busybox => EXIT_BB,
        }
    }

    /// I/O 失败退出码：GNU 4，busybox 1。
    pub fn io_exit(self) -> u32 {
        match self {
            Tool::Gnu => EXIT_IO,
            Tool::Busybox => EXIT_BB,
        }
    }

    /// 是否在失败路径上主动 `unlink` 临时件。
    ///
    /// GNU 在写失败/close 失败/改名失败三条路上都摘临时件；busybox 的摘除挂在
    /// `rename` 之后，失败时 `sedXXXXXX` 就留在目录里继续占那点剩余空间。
    pub fn cleans_temp_on_fail(self) -> bool {
        match self {
            Tool::Gnu => true,
            Tool::Busybox => false,
        }
    }

    /// 属性搬迁失败时是否**照常落位**（只报错、不撤回轮换）。
    pub fn rotates_after_attr_fail(self) -> bool {
        match self {
            Tool::Gnu => true,
            Tool::Busybox => false,
        }
    }

    /// 是否检查 `close`/`fsync` 的返回值。
    pub fn reports_close_error(self) -> bool {
        match self {
            Tool::Gnu => true,
            Tool::Busybox => false,
        }
    }

    /// 是否支持 `-i=SUFFIX` 的"先改名后备份"语义。
    pub fn supports_backup_suffix(self) -> bool {
        match self {
            Tool::Gnu => true,
            Tool::Busybox => false,
        }
    }

    /// 是否支持 `--follow-symlinks`。
    pub fn supports_follow_symlinks(self) -> bool {
        match self {
            Tool::Gnu => true,
            Tool::Busybox => false,
        }
    }

    /// 是否尝试搬 owner（非 root 时内核会拒）。
    pub fn attempts_chown(self) -> bool {
        match self {
            Tool::Gnu => true,
            Tool::Busybox => false,
        }
    }

    /// 属性搬迁的上限。
    pub fn attr_ceiling(self) -> u8 {
        match self {
            Tool::Gnu => A_ALL,
            Tool::Busybox => A_MODE,
        }
    }

    /// 源件非普通件时在哪个闸报（GNU 在 open 段 stat 后报，busybox 打开才红）。
    pub fn diagnoses_type_at_open(self) -> bool {
        match self {
            Tool::Gnu => true,
            Tool::Busybox => false,
        }
    }
}

/// 轮换模式（`sed` 命令行 `-i` 族的形态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotate {
    /// 不带 `-i`：结果走标准输出，本卡只在用法闸判它。
    None,
    /// `-i`（无后缀）：临时件直接换名，不留备份。
    Bare,
    /// `-i=SUFFIX`：先把原件改名成备份，再写临时件。
    Suffix,
}

impl Rotate {
    /// 文本。
    pub fn name(self) -> &'static str {
        match self {
            Rotate::None => "no-inplace",
            Rotate::Bare => "inplace",
            Rotate::Suffix => "inplace-suffix",
        }
    }

    /// 是否进入轮换纵线（backup 闸起算）。
    pub fn rotates(self) -> bool {
        match self {
            Rotate::None => false,
            Rotate::Bare | Rotate::Suffix => true,
        }
    }

    /// 是否会先把原件改名走（本卡 `value` 语义的分水岭）。
    pub fn moves_original(self) -> bool {
        match self {
            Rotate::Suffix => true,
            Rotate::None | Rotate::Bare => false,
        }
    }

    /// 是否会建临时件。
    pub fn uses_tempfile(self) -> bool {
        match self {
            Rotate::None => false,
            Rotate::Bare | Rotate::Suffix => true,
        }
    }
}

/// 属性位文本。
pub fn attr_name(bit: u8) -> &'static str {
    match bit {
        A_OWNER => "owner",
        A_MODE => "mode",
        _ => "none",
    }
}

/// `want` 是否落在 `cap` 之内。
pub fn attr_subset(want: u8, cap: u8) -> bool {
    want & !cap == 0
}

/// 属性位计数。
pub fn attr_popcount(mut bits: u8) -> usize {
    let mut n = 0usize;
    let mut k = 0usize;
    while k < A_BIT_COUNT {
        if let Some(b) = ATTR_BITS.get(k) {
            if bits & *b != 0 {
                n += 1;
            }
        }
        if bits != 0 {
            bits >>= 1;
        }
        k += 1;
    }
    n
}

/// 三个属性位的枚举表。
pub const ATTR_BITS: [u8; A_BIT_COUNT] = [A_OWNER, A_MODE];

// ---------------------------------------------------------------------------
// 阶段（九道闸）
// ---------------------------------------------------------------------------

/// 轮换结束的落点闸。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 命令行/选项（秩 0）。
    Argv,
    /// 源件打开与 fstat（秩 1）。
    Open,
    /// 备份改名（秩 2）。
    Backup,
    /// 临时件创建（秩 3）。
    Mktemp,
    /// 轮换写出（秩 4）。
    Write,
    /// 落定 flush/fsync（秩 5）。
    Close,
    /// 属性搬迁（秩 6）。
    Attr,
    /// 改名落位（秩 7）。
    Rename,
    /// 退出码聚合（秩 8）。
    Exit,
}

impl Stage {
    /// 次序（纵线不可倒）。
    pub fn order(self) -> u8 {
        match self {
            Stage::Argv => 0,
            Stage::Open => 1,
            Stage::Backup => 2,
            Stage::Mktemp => 3,
            Stage::Write => 4,
            Stage::Close => 5,
            Stage::Attr => 6,
            Stage::Rename => 7,
            Stage::Exit => 8,
        }
    }

    /// 文本。
    pub fn name(self) -> &'static str {
        match self {
            Stage::Argv => "argv",
            Stage::Open => "open",
            Stage::Backup => "backup",
            Stage::Mktemp => "mktemp",
            Stage::Write => "write",
            Stage::Close => "close",
            Stage::Attr => "attr",
            Stage::Rename => "rename",
            Stage::Exit => "exit",
        }
    }

    /// 是否失败落点（Exit 之外都算某道闸拦下）。
    pub fn failure(self) -> bool {
        match self {
            Stage::Exit => false,
            _ => true,
        }
    }

    /// 是否原件语义闸（停在这里会决定原件名下还有没有字节）。
    ///
    /// `attr` 不算：属性错不改内容；`exit` 不算：那是聚合，不是现场。
    pub fn orig_plane(self) -> bool {
        match self {
            Stage::Argv
            | Stage::Open
            | Stage::Backup
            | Stage::Mktemp
            | Stage::Write
            | Stage::Close
            | Stage::Rename => true,
            Stage::Attr | Stage::Exit => false,
        }
    }

    /// 是否可挂 ENOSPC 归因的闸。
    pub fn abort_plane(self) -> bool {
        match self {
            Stage::Backup | Stage::Mktemp | Stage::Write | Stage::Close | Stage::Rename => true,
            Stage::Argv | Stage::Open | Stage::Attr | Stage::Exit => false,
        }
    }

    /// 是否临时件可能已经在盘上的闸（残痕责任的起点）。
    pub fn residue_plane(self) -> bool {
        match self {
            Stage::Mktemp | Stage::Write | Stage::Close | Stage::Attr | Stage::Rename => true,
            Stage::Argv | Stage::Open | Stage::Backup | Stage::Exit => false,
        }
    }

    /// 秩 → 阶段（闸门推进用）。
    pub fn at(order: u8) -> Option<Stage> {
        match order {
            0 => Some(Stage::Argv),
            1 => Some(Stage::Open),
            2 => Some(Stage::Backup),
            3 => Some(Stage::Mktemp),
            4 => Some(Stage::Write),
            5 => Some(Stage::Close),
            6 => Some(Stage::Attr),
            7 => Some(Stage::Rename),
            8 => Some(Stage::Exit),
            _ => None,
        }
    }
}

/// 九道闸的枚举表（判据遍历用）。
pub const STAGES: [Stage; 9] = [
    Stage::Argv,
    Stage::Open,
    Stage::Backup,
    Stage::Mktemp,
    Stage::Write,
    Stage::Close,
    Stage::Attr,
    Stage::Rename,
    Stage::Exit,
];

// ---------------------------------------------------------------------------
// 文案落点
// ---------------------------------------------------------------------------

/// 文案落点（现场形态的分类，对应真实诊断）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 全量落位、名字未换、无话说。
    Complete,
    /// 落位成功，但原件有硬链接被斩断（码 0，语义账有话）。
    LinkCut,
    /// 落位成功且链接语义已还原（`--follow-symlinks`）。
    SymlinkRestored,
    /// `-i=SUFFIX` 的备份改名被拒（权限/目录面，但不是 ENOSPC）。
    BackupFail,
    /// `-i=SUFFIX` 的备份改名被目录面 ENOSPC 拒掉（扩不出目录块）。
    BackupEnospc,
    /// 临时件建不出来（`couldn't open temporary file`）。
    MktempFail,
    /// 目录不可写造成的建件失败（红但不是 ENOSPC）。
    TempHomeless,
    /// 写出段 ENOSPC（`couldn't write N output file(s) to ...`）。
    WriteEnospc,
    /// 落定段才爆（`couldn't close temporary file`）。
    CloseEnospc,
    /// 改名段目录扩不出来（`can't rename ... to ...`）。
    RenameEnospc,
    /// 落定段红却被工具吞掉（busybox 码 0）。
    CloseSwallowed,
    /// 属性段：owner 没搬成。
    AttrOwner,
    /// 属性段：mode 没搬成。
    AttrMode,
    /// 选项不认（用法族）。
    BadOption,
    /// 源件打不开。
    CantRead,
    /// `--follow-symlinks` 未配 `-i`。
    FollowNoI,
    /// 源不是普通件，无从轮换。
    NotRegular,
    /// 少了字节却没有报错——静默轮换，判死。
    SilentRotate,
    /// 邻居 errno 造成的中止（红，但不是 ENOSPC）。
    ForeignErr,
    /// 归因破断：文案/三字节数/退出码不同族。
    RotateMismatch,
}

impl Note {
    /// 文本。
    pub fn name(self) -> &'static str {
        match self {
            Note::Complete => "complete",
            Note::LinkCut => "link-cut",
            Note::SymlinkRestored => "symlink-restored",
            Note::BackupFail => "backup-fail",
            Note::BackupEnospc => "backup-enospc",
            Note::MktempFail => "mktemp-fail",
            Note::TempHomeless => "temp-homeless",
            Note::WriteEnospc => "write-enospc",
            Note::CloseEnospc => "close-enospc",
            Note::RenameEnospc => "rename-enospc",
            Note::CloseSwallowed => "close-swallowed",
            Note::AttrOwner => "attr-owner",
            Note::AttrMode => "attr-mode",
            Note::BadOption => "bad-option",
            Note::CantRead => "cant-read",
            Note::FollowNoI => "follow-without-inplace",
            Note::NotRegular => "not-regular",
            Note::SilentRotate => "silent-rotate",
            Note::ForeignErr => "foreign-err",
            Note::RotateMismatch => "rotate-mismatch",
        }
    }

    /// 是否工具自认成功（退出码 0 的那一族）。
    pub fn success(self) -> bool {
        match self {
            Note::Complete | Note::LinkCut | Note::SymlinkRestored | Note::CloseSwallowed => true,
            _ => false,
        }
    }

    /// 是否 ENOSPC 轮换中止族文案。
    pub fn abort_family(self) -> bool {
        match self {
            Note::BackupEnospc
            | Note::MktempFail
            | Note::WriteEnospc
            | Note::CloseEnospc
            | Note::RenameEnospc => true,
            _ => false,
        }
    }

    /// 是否属性尾段族（内容账已定，只有搬迁失败）。
    pub fn tail_family(self) -> bool {
        match self {
            Note::AttrOwner | Note::AttrMode => true,
            _ => false,
        }
    }

    /// 是否前置诊断族（还没开始搬字节就停）。
    pub fn prelude_family(self) -> bool {
        match self {
            Note::BadOption | Note::CantRead | Note::FollowNoI | Note::NotRegular
            | Note::TempHomeless | Note::BackupFail => true,
            _ => false,
        }
    }

    /// 是否本卡判死的异常形态。
    pub fn defect_family(self) -> bool {
        match self {
            Note::SilentRotate | Note::ForeignErr | Note::RotateMismatch => true,
            _ => false,
        }
    }

    /// 是否"码 0 但现场有话"。
    pub fn noisy_success(self) -> bool {
        match self {
            Note::LinkCut | Note::SymlinkRestored | Note::CloseSwallowed => true,
            _ => false,
        }
    }

    /// 是否真的换成了新 inode（落位族）。
    pub fn landed(self) -> bool {
        match self {
            Note::Complete | Note::LinkCut | Note::SymlinkRestored => true,
            _ => false,
        }
    }

    /// 尾段族对应的属性位。
    pub fn attr_bit(self) -> u8 {
        match self {
            Note::AttrOwner => A_OWNER,
            Note::AttrMode => A_MODE,
            _ => A_NONE,
        }
    }

    /// 落点归属的阶段（本卡唯一的"文案 ⇒ 闸"反查表）。
    pub fn stage(self) -> Stage {
        match self {
            Note::Complete | Note::CloseSwallowed => Stage::Exit,
            Note::LinkCut | Note::SymlinkRestored => Stage::Rename,
            Note::BadOption | Note::FollowNoI => Stage::Argv,
            Note::CantRead | Note::NotRegular => Stage::Open,
            Note::BackupFail | Note::BackupEnospc => Stage::Backup,
            Note::MktempFail | Note::TempHomeless => Stage::Mktemp,
            Note::WriteEnospc | Note::SilentRotate => Stage::Write,
            Note::CloseEnospc => Stage::Close,
            Note::AttrOwner | Note::AttrMode => Stage::Attr,
            Note::RenameEnospc => Stage::Rename,
            Note::ForeignErr | Note::RotateMismatch => Stage::Exit,
        }
    }
}

/// 二十个落点的枚举表（判据遍历用）。
pub const NOTES: [Note; 20] = [
    Note::Complete,
    Note::LinkCut,
    Note::SymlinkRestored,
    Note::BackupFail,
    Note::BackupEnospc,
    Note::MktempFail,
    Note::TempHomeless,
    Note::WriteEnospc,
    Note::CloseEnospc,
    Note::RenameEnospc,
    Note::CloseSwallowed,
    Note::AttrOwner,
    Note::AttrMode,
    Note::BadOption,
    Note::CantRead,
    Note::FollowNoI,
    Note::NotRegular,
    Note::SilentRotate,
    Note::ForeignErr,
    Note::RotateMismatch,
];

// ---------------------------------------------------------------------------
// 档位
// ---------------------------------------------------------------------------

/// 结果档位（按实际结果投影，与本卡立卡理由同构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 全量落位且无声（码 0、inode 已换、无硬链接牵挂）。
    Rotated,
    /// 码 0 但现场有话（斩硬链接、还原链接、落定被吞）。
    Noisy,
    /// 码非 0 且不是 ENOSPC 轮换中止（用法/类型/备份/属性/邻居）。
    Halted,
    /// ENOSPC 打断了轮换（归因命中）。
    Aborted,
}

impl Tier {
    /// 由结果投影。
    pub fn of(o: &Outcome) -> Tier {
        if o.default {
            Tier::Aborted
        } else if o.ok() {
            match o.note {
                Note::Complete => Tier::Rotated,
                _ => Tier::Noisy,
            }
        } else {
            Tier::Halted
        }
    }

    /// 次序。
    pub fn index(self) -> usize {
        match self {
            Tier::Rotated => 0,
            Tier::Noisy => 1,
            Tier::Halted => 2,
            Tier::Aborted => 3,
        }
    }

    /// 文本。
    pub fn name(self) -> &'static str {
        match self {
            Tier::Rotated => "rotated",
            Tier::Noisy => "noisy",
            Tier::Halted => "halted",
            Tier::Aborted => "aborted",
        }
    }

    /// 是否成功档（退出码 0）。
    pub fn success(self) -> bool {
        match self {
            Tier::Rotated | Tier::Noisy => true,
            Tier::Halted | Tier::Aborted => false,
        }
    }

    /// 是否中止档。
    pub fn aborted(self) -> bool {
        match self {
            Tier::Aborted => true,
            _ => false,
        }
    }
}

/// 四档的枚举表。
pub const TIERS: [Tier; TIER_COUNT] = [Tier::Rotated, Tier::Noisy, Tier::Halted, Tier::Aborted];

// ---------------------------------------------------------------------------
// 申报缺陷
// ---------------------------------------------------------------------------

/// 一槽申报与实际现场的偏差种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 一致。
    Ok,
    /// 阶段错。
    Stage,
    /// 退出码错。
    Code,
    /// 原件名下字节错。
    Value,
    /// 文案落点错。
    Note,
    /// 轮换归因错。
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
    pub fn code(self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Stage => SI_ESTAGE,
            Fault::Code => SI_ECODE,
            Fault::Value => SI_EVALUE,
            Fault::Note => SI_ENOTE,
            Fault::Default => SI_EDEFAULT,
            Fault::Abort => SI_EABORT,
            Fault::Estate => SI_ESTATE,
            Fault::Count => SI_ECOUNT,
            Fault::Slot => SI_ESLOT,
            Fault::Seal => SI_ESEAL,
            Fault::Nobs => SI_ENOBS,
            Fault::Matrix => SI_EMATRIX,
            Fault::Bal => SI_EBAL,
        }
    }

    /// 是否非零。
    pub fn alive(self) -> bool {
        !matches!(self, Fault::Ok)
    }
}

/// 缺陷短名（渲染用）。
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

/// 十四个缺陷种类的枚举表。
pub const FAULTS: [Fault; 14] = [
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

// ---------------------------------------------------------------------------
// 输出目录画像（本卡结构闸的载体：临时件落在**输出件同目录**）
// ---------------------------------------------------------------------------

/// `sed -i` 的落点目录画像。
///
/// 本卡的空间账不在"数据块还剩多少"这一维上收口——那一维归 F11681/C3；本卡要
/// 的是**目录能不能加一个名字**：备份改名与最终改名都要动目录项。满盘目录扩不
/// 出来时，`rename` 会真返 ENOSPC，这就是 `rename` 闸存在的全部理由。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirProfile {
    /// 目录名。
    pub dir: &'static str,
    /// 块大小（字节）。
    pub bsize: u32,
    /// 数据空闲块。
    pub data_free: u32,
    /// 目录还能不能扩（htree 索引可扩；ext2 静态目录扩不了）。
    pub dir_can_grow: bool,
    /// 是否延迟分配（close 才爆的根源）。
    pub delalloc: bool,
    /// 目录是否可写（mkstemp 的前提）。
    pub writable: bool,
}

impl DirProfile {
    /// 空白画像。
    pub const fn blank() -> DirProfile {
        DirProfile {
            dir: "",
            bsize: 0,
            data_free: 0,
            dir_can_grow: false,
            delalloc: false,
            writable: false,
        }
    }

    /// 空闲数据字节（乘法溢出视为坏画像）。
    pub fn free_bytes(&self) -> Option<u32> {
        self.bsize.checked_mul(self.data_free)
    }

    /// 是否结构良好。
    pub fn well_formed(&self) -> bool {
        if self.dir.is_empty() {
            return false;
        }
        if self.bsize != 512 && self.bsize != 1024 && self.bsize != 4096 {
            return false;
        }
        if self.free_bytes().is_none() {
            return false;
        }
        self.data_free <= 0x0800_0000
    }

    /// 数据面是否已满。
    pub fn data_full(&self) -> bool {
        self.data_free == 0
    }

    /// 目录能否新增一个名字（备份/落位两次改名都要它）。
    pub fn can_add_entry(&self) -> bool {
        self.writable && (self.data_free > 0 || self.dir_can_grow)
    }

    /// `mkstemp` 能否成事。
    pub fn can_mktemp(&self) -> bool {
        self.writable && self.can_add_entry()
    }

    /// close 是否可能迟到报错。
    pub fn close_can_fail(&self) -> bool {
        self.delalloc
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.dir);
        h = fold_word(h, self.bsize as u64);
        h = fold_word(h, self.data_free as u64);
        h = fold_word(h, self.dir_can_grow as u64);
        h = fold_word(h, self.delalloc as u64);
        match self.free_bytes() {
            Some(v) => fold_word(fold_word(h, self.writable as u64), v as u64),
            None => fold_word(fold_word(h, self.writable as u64), 0),
        }
    }
}

/// 三块被测目录：满盘可扩、满盘不可扩、半满不可写。
pub fn dir_profiles() -> [DirProfile; 3] {
    [
        DirProfile {
            dir: "cfg.d",
            bsize: 4096,
            data_free: 0,
            dir_can_grow: true,
            delalloc: true,
            writable: true,
        },
        DirProfile {
            dir: "legacy.d",
            bsize: 1024,
            data_free: 0,
            dir_can_grow: false,
            delalloc: false,
            writable: true,
        },
        DirProfile {
            dir: "ro.d",
            bsize: 512,
            data_free: 8,
            dir_can_grow: true,
            delalloc: false,
            writable: false,
        },
    ]
}

// ---------------------------------------------------------------------------
// 形状（本卡输入的全部维度）
// ---------------------------------------------------------------------------

/// 一次 `sed -i` 轮换的形状：工具 + 模式 + 三个字节数 + 九道闸的现场结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// 目录。
    pub dir: &'static str,
    /// 原件路径（本卡路径，轮换的靶子）。
    pub file: &'static str,
    /// 备份后缀（`-i=SUFFIX` 的 SUFFIX；`rotate != Suffix` 时为空）。
    pub suffix: &'static str,
    /// 哪一套 `sed`。
    pub tool: Tool,
    /// `-i` 形态。
    pub rotate: Rotate,
    /// 原件字节数（`fstat` 的 `st_size`）。
    pub size: u32,
    /// 本轮换应有的输出字节数（编辑后尺寸，与 `size` 可以不等）。
    pub want: u32,
    /// 已写进临时件的字节数。
    pub written: u32,
    /// 选项/参数本身是否被拒。
    pub argv_bad: bool,
    /// 是否给了 `--follow-symlinks`。
    pub follow_symlinks: bool,
    /// 源件是否打开成功（含 `fstat`）。
    pub open_ok: bool,
    /// 源是否不是普通件（目录/FIFO/设备）。
    pub not_regular: bool,
    /// 备份改名是否失败。
    pub backup_err: bool,
    /// 临时件是否建成。
    pub mktemp_ok: bool,
    /// 写出段是否收到错误。
    pub write_err: bool,
    /// 落定段（flush/fsync）是否报错。
    pub close_err: bool,
    /// 属性搬迁是否报错。
    pub attr_err: bool,
    /// 改名落位是否失败。
    pub rename_err: bool,
    /// 现场 errno（从 F11681 引进的结论，本卡不重判）。
    pub errno_seen: i32,
    /// 本轮要求搬迁的属性位。
    pub want_attrs: u8,
    /// 实际失败的属性位。
    pub attr_failed: u8,
    /// 原件硬链接数（>1 时轮换必然斩断它们）。
    pub hardlinks: u8,
    /// 原件本身是否是指向别处的符号链接。
    pub is_symlink: bool,
    /// 调用者是否 root（决定 `fchown` 成败的呈现）。
    pub root_caller: bool,
}

impl Shape {
    /// 默认形状：GNU `-i` 无后缀、建件成功、无错误、不搬属性。
    pub const fn blank() -> Shape {
        Shape {
            tag: "",
            dir: "cfg.d",
            file: "cfg.conf",
            suffix: "",
            tool: Tool::Gnu,
            rotate: Rotate::Bare,
            size: 0,
            want: 0,
            written: 0,
            argv_bad: false,
            follow_symlinks: false,
            open_ok: true,
            not_regular: false,
            backup_err: false,
            mktemp_ok: true,
            write_err: false,
            close_err: false,
            attr_err: false,
            rename_err: false,
            errno_seen: ERR_NONE,
            want_attrs: A_NONE,
            attr_failed: A_NONE,
            hardlinks: 1,
            is_symlink: false,
            root_caller: false,
        }
    }

    /// 换"已写字节"的副本（谓词把同一形状推到不同档位复算）。
    pub const fn clone_written(&self, w: u32) -> Shape {
        Shape {
            tag: self.tag,
            dir: self.dir,
            file: self.file,
            suffix: self.suffix,
            tool: self.tool,
            rotate: self.rotate,
            size: self.size,
            want: self.want,
            written: w,
            argv_bad: self.argv_bad,
            follow_symlinks: self.follow_symlinks,
            open_ok: self.open_ok,
            not_regular: self.not_regular,
            backup_err: self.backup_err,
            mktemp_ok: self.mktemp_ok,
            write_err: self.write_err,
            close_err: self.close_err,
            attr_err: self.attr_err,
            rename_err: self.rename_err,
            errno_seen: self.errno_seen,
            want_attrs: self.want_attrs,
            attr_failed: self.attr_failed,
            hardlinks: self.hardlinks,
            is_symlink: self.is_symlink,
            root_caller: self.root_caller,
        }
    }

    /// 换"现场 errno"的副本（把同一现场推到邻居档）。
    pub const fn clone_errno(&self, e: i32) -> Shape {
        Shape {
            tag: self.tag,
            dir: self.dir,
            file: self.file,
            suffix: self.suffix,
            tool: self.tool,
            rotate: self.rotate,
            size: self.size,
            want: self.want,
            written: self.written,
            argv_bad: self.argv_bad,
            follow_symlinks: self.follow_symlinks,
            open_ok: self.open_ok,
            not_regular: self.not_regular,
            backup_err: self.backup_err,
            mktemp_ok: self.mktemp_ok,
            write_err: self.write_err,
            close_err: self.close_err,
            attr_err: self.attr_err,
            rename_err: self.rename_err,
            errno_seen: e,
            want_attrs: self.want_attrs,
            attr_failed: self.attr_failed,
            hardlinks: self.hardlinks,
            is_symlink: self.is_symlink,
            root_caller: self.root_caller,
        }
    }

    /// 换"工具面"的副本（GNU/busybox 对照复算）。
    pub const fn clone_tool(&self, t: Tool) -> Shape {
        Shape {
            tag: self.tag,
            dir: self.dir,
            file: self.file,
            suffix: self.suffix,
            tool: t,
            rotate: self.rotate,
            size: self.size,
            want: self.want,
            written: self.written,
            argv_bad: self.argv_bad,
            follow_symlinks: self.follow_symlinks,
            open_ok: self.open_ok,
            not_regular: self.not_regular,
            backup_err: self.backup_err,
            mktemp_ok: self.mktemp_ok,
            write_err: self.write_err,
            close_err: self.close_err,
            attr_err: self.attr_err,
            rename_err: self.rename_err,
            errno_seen: self.errno_seen,
            want_attrs: self.want_attrs,
            attr_failed: self.attr_failed,
            hardlinks: self.hardlinks,
            is_symlink: self.is_symlink,
            root_caller: self.root_caller,
        }
    }

    /// 换"轮换模式"的副本（无后缀 / 带后缀 对照复算）。
    pub const fn clone_rotate(&self, r: Rotate, suffix: &'static str) -> Shape {
        Shape {
            tag: self.tag,
            dir: self.dir,
            file: self.file,
            suffix: suffix,
            tool: self.tool,
            rotate: r,
            size: self.size,
            want: self.want,
            written: self.written,
            argv_bad: self.argv_bad,
            follow_symlinks: self.follow_symlinks,
            open_ok: self.open_ok,
            not_regular: self.not_regular,
            backup_err: self.backup_err,
            mktemp_ok: self.mktemp_ok,
            write_err: self.write_err,
            close_err: self.close_err,
            attr_err: self.attr_err,
            rename_err: self.rename_err,
            errno_seen: self.errno_seen,
            want_attrs: self.want_attrs,
            attr_failed: self.attr_failed,
            hardlinks: self.hardlinks,
            is_symlink: self.is_symlink,
            root_caller: self.root_caller,
        }
    }

    /// 备份件路径（`file` + `suffix`）。
    pub fn backup_path(&self) -> String {
        let mut s = String::from(self.file);
        s.push_str(self.suffix);
        s
    }

    /// 临时件路径。
    pub fn temp_path(&self) -> String {
        let mut s = String::from(self.dir);
        s.push('/');
        s.push_str(TMP_TEMPLATE);
        s
    }

    /// 本轮是否已经把原件改名走（`-i=SUFFIX` 且备份改名成功）。
    ///
    /// 这是本卡 `value` 语义的分水岭：改名一走，"原件不动"就**不可能**以
    /// "原名字还在"的形式兑现，只能以"内容在备份件里"的形式兑现。
    pub fn original_moved(&self) -> bool {
        self.rotate.moves_original() && !self.backup_err
    }

    /// 未落位时"原件名下应得的字节"。
    pub fn untouched_bytes(&self) -> u32 {
        if self.original_moved() {
            0
        } else {
            self.size
        }
    }

    /// 是否属于"本卡可判的轮换"（进了 backup 闸之后）。
    pub fn rotates(&self) -> bool {
        self.rotate.rotates()
    }

    /// 形状是否合法可判（各维必须自洽，不自洽的形状不进判据）。
    pub fn valid(&self) -> bool {
        if !errno_legal(self.errno_seen) {
            return false;
        }
        if self.size > SIZE_MAX || self.want > SIZE_MAX || self.written > SIZE_MAX {
            return false;
        }
        if self.written > self.want {
            return false;
        }
        if self.hardlinks == 0 || self.hardlinks > 3 {
            return false;
        }
        if self.is_symlink && !self.follow_symlinks {
            // 符号链接未被跟随时的替换语义归 F11464，本卡不收这种形状。
            return false;
        }
        if self.follow_symlinks && !self.tool.supports_follow_symlinks() {
            return false;
        }
        if self.is_symlink && self.hardlinks > 1 {
            return false;
        }
        if self.rotate == Rotate::Suffix && self.suffix.is_empty() {
            return false;
        }
        if self.rotate != Rotate::Suffix && !self.suffix.is_empty() {
            return false;
        }
        if self.rotate == Rotate::Suffix && !self.tool.supports_backup_suffix() {
            return false;
        }
        if self.rotate == Rotate::None && !self.follow_symlinks {
            return false;
        }
        if !attr_subset(self.want_attrs, self.tool.attr_ceiling()) {
            return false;
        }
        if !attr_subset(self.attr_failed, self.want_attrs) {
            return false;
        }
        if self.attr_err != (self.attr_failed != A_NONE) {
            return false;
        }
        if self.attr_err && attr_popcount(self.attr_failed) != 1 {
            // 两位一起塌就落不进单一文案落点，本卡不收（真实 sed 也只逐条报）。
            return false;
        }
        if self.attr_failed & A_OWNER != 0 && self.root_caller {
            return false;
        }
        if self.argv_bad {
            if self.written != 0 {
                return false;
            }
            if self.backup_err || self.write_err || self.close_err || self.attr_err
                || self.rename_err
            {
                return false;
            }
            if self.errno_seen != ERR_NONE {
                return false;
            }
        }
        if !self.open_ok {
            if self.written != 0 || self.errno_seen == ERR_NONE {
                return false;
            }
            if self.backup_err || self.write_err || self.close_err || self.attr_err
                || self.rename_err
                || self.not_regular
            {
                return false;
            }
        }
        if self.not_regular {
            if self.written != 0 || self.backup_err || self.write_err || self.close_err
                || self.attr_err
                || self.rename_err
                || self.argv_bad
            {
                return false;
            }
        }
        if self.backup_err {
            if self.written != 0 || self.write_err || self.close_err || self.attr_err
                || self.rename_err
                || self.errno_seen == ERR_NONE
            {
                return false;
            }
        }
        if !self.mktemp_ok {
            if !self.rotate.uses_tempfile() || self.written != 0 {
                return false;
            }
            if self.errno_seen == ERR_NONE || self.backup_err {
                return false;
            }
            if self.write_err || self.close_err || self.attr_err || self.rename_err {
                return false;
            }
        }
        if self.write_err {
            if !self.rotate.uses_tempfile() || !self.mktemp_ok {
                return false;
            }
            if self.written >= self.want || self.errno_seen == ERR_NONE {
                return false;
            }
            if self.close_err || self.attr_err || self.rename_err || self.backup_err {
                return false;
            }
        }
        if self.close_err {
            if !self.rotate.uses_tempfile() || !self.mktemp_ok {
                return false;
            }
            if self.write_err || self.attr_err || self.rename_err || self.backup_err {
                return false;
            }
            if self.written != self.want || self.errno_seen != ERR_ENOSPC {
                return false;
            }
        }
        if self.attr_err {
            if !self.rotate.uses_tempfile() || !self.mktemp_ok {
                return false;
            }
            if self.write_err || self.close_err || self.rename_err || self.backup_err {
                return false;
            }
            if self.written != self.want || self.errno_seen == ERR_NONE {
                return false;
            }
        }
        if self.rename_err {
            if !self.rotate.uses_tempfile() || !self.mktemp_ok {
                return false;
            }
            if self.write_err || self.close_err || self.attr_err || self.backup_err {
                return false;
            }
            if self.written != self.want || self.errno_seen != ERR_ENOSPC {
                return false;
            }
        }
        true
    }

    /// 指纹：对全部判定字段敏感。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_str(h, self.dir);
        h = fold_str(h, self.file);
        h = fold_str(h, self.suffix);
        h = fold_str(h, self.tool.text());
        h = fold_str(h, self.rotate.name());
        h = fold_word(h, self.size as u64);
        h = fold_word(h, self.want as u64);
        h = fold_word(h, self.written as u64);
        h = fold_word(h, self.argv_bad as u64);
        h = fold_word(h, self.follow_symlinks as u64);
        h = fold_word(h, self.open_ok as u64);
        h = fold_word(h, self.not_regular as u64);
        h = fold_word(h, self.backup_err as u64);
        h = fold_word(h, self.mktemp_ok as u64);
        h = fold_word(h, self.write_err as u64);
        h = fold_word(h, self.close_err as u64);
        h = fold_word(h, self.attr_err as u64);
        h = fold_word(h, self.rename_err as u64);
        h = fold_i32(h, self.errno_seen);
        h = fold_word(h, self.want_attrs as u64);
        h = fold_word(h, self.attr_failed as u64);
        h = fold_word(h, self.hardlinks as u64);
        h = fold_word(h, self.is_symlink as u64);
        fold_word(h, self.root_caller as u64)
    }
}

// ---------------------------------------------------------------------------
// 结果
// ---------------------------------------------------------------------------

/// 一次判定的结果（六维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 结束阶段（最后拦下这次轮换的闸）。
    pub stage: Stage,
    /// 退出码（0/1/2/4）。
    pub code: u32,
    /// 原件名下留档字节。
    pub value: u32,
    /// 文案落点。
    pub note: Note,
    /// 归因位：ENOSPC 是否打断了轮换。
    pub default: bool,
}

impl Outcome {
    /// 空白结果。
    pub const fn blank() -> Outcome {
        Outcome {
            stage: Stage::Exit,
            code: EXIT_OK,
            value: 0,
            note: Note::Complete,
            default: false,
        }
    }

    /// 是否成功（码 0 且文案属成功族）。
    pub fn ok(&self) -> bool {
        self.code == EXIT_OK && self.note.success()
    }

    /// 是否失败。
    pub fn failed(&self) -> bool {
        !self.ok()
    }

    /// 是否归因命中：ENOSPC 打断轮换，且文案与退出码同族。
    pub fn attributed(&self) -> bool {
        self.default && self.note.abort_family() && self.code != EXIT_OK && self.failed()
    }

    /// 新内容是否真的落位（原件名字指向编辑后的字节）。
    pub fn landed(&self) -> bool {
        self.note.landed()
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

/// 备份件里的字节（`-i=SUFFIX` 兑现的是"内容"而不是"名字"）。
///
/// 只在真的越过备份闸之后才有值：备份那一步是 `rename`，不占数据块，满盘照做。
pub fn backup_bytes(s: &Shape, o: &Outcome) -> u32 {
    if s.rotate.moves_original() && !s.backup_err && o.stage.order() >= Stage::Mktemp.order() {
        s.size
    } else {
        0
    }
}

/// 原件名下是否还压着改名前那份内容。
///
/// 判的是"**这个名字下**还有没有改名前的字节"，不是"用户还有没有数据"：带后缀
/// 那一格内容整体搬进了 `.bak`，所以这里必须报 false——运维脚本按
/// `[ -f cfg.conf ]` 判断"失败即原样"，读到的就是这个 false。
pub fn orig_intact(s: &Shape, o: &Outcome) -> bool {
    o.value == s.size && !rotation_landed(s, o) && !s.original_moved()
}

/// 新内容是否真的挂到了原件名字下。
///
/// 除了三个"落位族"文案，还包含 GNU 属性段那一格：它码非 0、文案在报错，但
/// `rename` 照做——于是**内容已经换了、退出码却是 4**。这一格是本卡"码非 0
/// 不等于没改"的实体来源，因此内容守恒必须按"已落位"记账，不能按报错记账。
pub fn rotation_landed(s: &Shape, o: &Outcome) -> bool {
    o.landed() || (o.note.tail_family() && s.tool.rotates_after_attr_fail())
}

/// 临时件残痕字节（本卡"临时清理"那一半承诺）。
///
/// 纯函数：只由"停在哪道闸 + 哪个工具 + 已写多少"决定。秩 ≤ 备份闸时盘上根本
/// 没有 `sedXXXXXX`；GNU 在失败路径上摘临时件，busybox 不摘——于是同一现场的
/// 残痕是两个数，而这两个数都不许由申报方随手填。
pub fn leftover(s: &Shape, o: &Outcome) -> u32 {
    if o.ok() {
        return 0;
    }
    if o.stage.order() < Stage::Mktemp.order() {
        return 0;
    }
    if o.stage == Stage::Mktemp {
        // 建件都没成，谈不上残痕。
        return 0;
    }
    if s.tool.cleans_temp_on_fail() {
        0
    } else {
        s.written
    }
}

/// 盘上是否存在临时件残痕。
pub fn temp_present(s: &Shape, o: &Outcome) -> bool {
    leftover(s, o) > 0
}

/// 落定段被工具吞掉时**未归因**的延迟块字节。
pub fn deferred_lost(s: &Shape, o: &Outcome) -> u32 {
    if s.close_err && !s.tool.reports_close_error() && o.note == Note::CloseSwallowed {
        s.want
    } else {
        0
    }
}

/// 内容守恒和：原件名下 + 备份件。
///
/// 本卡的红线算术——**轮换没落位时这个和必须等于原件原尺寸**，少一个字节就是
/// "`sed -i` 把用户的数据弄丢了"。落位时：带后缀的旧内容整体搬进了备份件，和
/// 等于 `want + size`；无后缀的旧 inode 被 `rename` 顶掉，和只等于 `want`。
pub fn content_sum(s: &Shape, o: &Outcome) -> u32 {
    o.value.saturating_add(backup_bytes(s, o))
}

/// 落位/未落位各自应有的内容总量。
pub fn content_expect(s: &Shape, o: &Outcome) -> u32 {
    if rotation_landed(s, o) {
        if s.rotate.moves_original() {
            s.want.saturating_add(s.size)
        } else {
            s.want
        }
    } else {
        s.size
    }
}

/// 内容守恒判据：留档和是否等于应有和。
pub fn content_balanced(s: &Shape, o: &Outcome) -> bool {
    if o.ok() && deferred_lost(s, o) > 0 {
        // 表观尺寸对、物理块丢了——本卡不许用表观和掩盖这一格。
        return false;
    }
    content_sum(s, o) == content_expect(s, o)
}

/// 取结果里的原件字节（与 `content_sum` 同址，单列是为了把"哪一个数是原件
/// 名下"这件事在调用点上写明白）。
pub fn orig_bytes(o: &Outcome) -> u32 {
    o.value
}

/// 未落位时该由 `value` 承载的字节数（`define` 与判据共用同一算法）。
pub fn halt_value(s: &Shape) -> u32 {
    s.untouched_bytes()
}

/// 是否"停在原件语义闸"（闸序 + 结果联合判据用）。
pub fn decides_value(o: &Outcome) -> bool {
    o.stage.orig_plane()
}

// ---------------------------------------------------------------------------
// 定义纵线（闸门次序：argv → open → backup → mktemp → write → close → attr
//                    → rename → exit）
// ---------------------------------------------------------------------------

/// 判定一次 `sed -i` 轮换在满盘现场的收尾结论。
///
/// 每一闸只在上一闸放行后才执行；任何一段拦下就立刻定案，后面的段不再有权改写
/// 它——这是"原件名下字节"能被反查的唯一前提。两条与 `cp` 卡不同的心跳：
///
/// * **备份闸在写闸之前**。`rename` 不占数据块，所以满盘上它先成功，把原件的
///   名字移走；随后写闸才红。于是"带后缀失败"这一格的 `value` 是 0，不是 size。
/// * **属性闸不改内容**。GNU 搬 owner 失败照样改名落位（码 4、内容已换），
///   busybox 直接放弃（码 1、临时件留痕）。两工具在这一闸分道，是"码非 0 不等于
///   没改"的实体来源。
pub fn define(s: &Shape) -> Outcome {
    // ---- argv：选项本身 ----
    if s.argv_bad {
        return Outcome {
            stage: Stage::Argv,
            code: s.tool.usage_exit(),
            value: s.size,
            note: Note::BadOption,
            default: false,
        };
    }
    if !s.rotate.rotates() {
        // 本卡只收"`--follow-symlinks` 没配 `-i`"这一种非轮换形状。
        return Outcome {
            stage: Stage::Argv,
            code: s.tool.usage_exit(),
            value: s.size,
            note: Note::FollowNoI,
            default: false,
        };
    }

    // ---- open：源件 open + fstat ----
    if !s.open_ok {
        return Outcome {
            stage: Stage::Open,
            code: s.tool.io_exit(),
            value: s.size,
            note: Note::CantRead,
            default: false,
        };
    }
    if s.not_regular {
        return Outcome {
            stage: Stage::Open,
            code: s.tool.io_exit(),
            value: s.size,
            note: Note::NotRegular,
            default: false,
        };
    }

    // ---- backup：`-i=SUFFIX` 先把原件改名成备份 ----
    if s.backup_err {
        if s.errno_seen == ERR_ENOSPC {
            return Outcome {
                stage: Stage::Backup,
                code: s.tool.io_exit(),
                value: s.size,
                note: Note::BackupEnospc,
                default: true,
            };
        }
        return Outcome {
            stage: Stage::Backup,
            code: s.tool.io_exit(),
            value: s.size,
            note: Note::BackupFail,
            default: false,
        };
    }

    // 越过备份闸之后，带后缀的形状里原件名字已经不在了。
    let halt_value = s.untouched_bytes();

    // ---- mktemp：临时件 ----
    if !s.mktemp_ok {
        if s.errno_seen == ERR_ENOSPC {
            return Outcome {
                stage: Stage::Mktemp,
                code: s.tool.io_exit(),
                value: halt_value,
                note: Note::MktempFail,
                default: true,
            };
        }
        return Outcome {
            stage: Stage::Mktemp,
            code: s.tool.io_exit(),
            value: halt_value,
            note: Note::TempHomeless,
            default: false,
        };
    }

    // ---- write：把编辑结果写进临时件 ----
    if s.write_err {
        if s.errno_seen == ERR_ENOSPC {
            return Outcome {
                stage: Stage::Write,
                code: s.tool.io_exit(),
                value: halt_value,
                note: Note::WriteEnospc,
                default: true,
            };
        }
        return Outcome {
            stage: Stage::Write,
            code: s.tool.io_exit(),
            value: halt_value,
            note: Note::ForeignErr,
            default: false,
        };
    }
    // 少了字节却没有报错——静默轮换，判死。
    if s.rotate.uses_tempfile() && s.written < s.want && !s.close_err && !s.attr_err
        && !s.rename_err
    {
        return Outcome {
            stage: Stage::Write,
            code: s.tool.io_exit(),
            value: halt_value,
            note: Note::SilentRotate,
            default: false,
        };
    }

    // ---- close：flush/fsync 才爆的现场 ----
    if s.close_err && s.tool.reports_close_error() {
        return Outcome {
            stage: Stage::Close,
            code: s.tool.io_exit(),
            value: halt_value,
            note: Note::CloseEnospc,
            default: true,
        };
    }

    // ---- attr：owner/mode 搬迁 ----
    if s.attr_err {
        let note = match s.attr_failed {
            A_OWNER => Note::AttrOwner,
            A_MODE => Note::AttrMode,
            _ => Note::RotateMismatch,
        };
        if s.tool.rotates_after_attr_fail() {
            // GNU：属性没搬成，轮换照做——码非 0 而内容已落位。
            return Outcome {
                stage: Stage::Attr,
                code: s.tool.io_exit(),
                value: s.want,
                note: note,
                default: false,
            };
        }
        // busybox：放弃轮换，写完的临时件原地留下。
        return Outcome {
            stage: Stage::Attr,
            code: s.tool.io_exit(),
            value: halt_value,
            note: note,
            default: false,
        };
    }

    // ---- rename：临时件换成原件的名字 ----
    if s.rename_err {
        return Outcome {
            stage: Stage::Rename,
            code: s.tool.io_exit(),
            value: halt_value,
            note: Note::RenameEnospc,
            default: true,
        };
    }

    // ---- exit：退出码聚合 ----
    let mut note = Note::Complete;
    if s.close_err {
        // busybox 丢了 close 错误：表观落位，延迟块无人记账。
        note = Note::CloseSwallowed;
    } else if s.is_symlink && s.follow_symlinks {
        note = Note::SymlinkRestored;
    } else if s.hardlinks > 1 {
        note = Note::LinkCut;
    }
    Outcome {
        stage: Stage::Exit,
        code: EXIT_OK,
        value: s.want,
        note: note,
        default: false,
    }
}

/// 一次运行的诊断行（逐字复刻真实报错，按工具面选模板）。
pub fn message(s: &Shape, o: &Outcome) -> String {
    match o.note {
        Note::Complete | Note::LinkCut | Note::SymlinkRestored => String::new(),
        Note::BadOption => match s.tool {
            Tool::Gnu => msg_gnu_unrecognized("--follow-symlink"),
            Tool::Busybox => msg_bb_unrecognized("Z"),
        },
        Note::FollowNoI => msg_gnu_follow_without_i(),
        Note::CantRead => match s.tool {
            Tool::Gnu => msg_gnu_cant_read(s.file, s.errno_seen),
            Tool::Busybox => msg_bb_cannot_open(s.file, s.errno_seen),
        },
        Note::NotRegular => msg_gnu_not_regular(s.file),
        Note::BackupFail => msg_gnu_backup_fail(s.file, s.backup_path().as_str(), s.errno_seen),
        Note::BackupEnospc => msg_gnu_backup_fail(s.file, s.backup_path().as_str(), s.errno_seen),
        Note::MktempFail => msg_gnu_mktemp(s.dir, s.errno_seen),
        Note::TempHomeless => match s.tool {
            Tool::Gnu => msg_gnu_mktemp(s.dir, s.errno_seen),
            Tool::Busybox => msg_bb_temp_fail(s.file, s.errno_seen),
        },
        Note::WriteEnospc | Note::ForeignErr => match s.tool {
            Tool::Gnu => msg_gnu_write_fail(1, s.errno_seen),
            Tool::Busybox => msg_bb_write_error(s.temp_path().as_str(), s.errno_seen),
        },
        Note::SilentRotate => msg_silent_rotate(s.file, s.written, s.want),
        Note::CloseEnospc => msg_gnu_close_fail(s.dir, s.errno_seen),
        Note::CloseSwallowed => String::new(),
        Note::AttrOwner => msg_gnu_chown(s.file, s.errno_seen),
        Note::AttrMode => match s.tool {
            Tool::Gnu => msg_gnu_chmod(s.file, s.errno_seen),
            Tool::Busybox => msg_bb_chmod(s.file, s.errno_seen),
        },
        Note::RenameEnospc => match s.tool {
            Tool::Gnu => msg_gnu_rename_fail(s.temp_path().as_str(), s.file, s.errno_seen),
            Tool::Busybox => msg_bb_rename_fail(s.temp_path().as_str(), s.file, s.errno_seen),
        },
        Note::RotateMismatch => msg_rotate_broken(
            o.note.name(),
            o.value,
            leftover(s, o),
            o.code,
        ),
    }
}

/// 归因破断时的解释行（`abort_ok` 谓词与释义共用）。
pub fn msg_broken(s: &Shape, o: &Outcome) -> String {
    msg_rotate_broken(o.note.name(), o.value, leftover(s, o), o.code)
}

/// 残痕解释行。
pub fn msg_temp_residue(s: &Shape, o: &Outcome) -> String {
    let n = leftover(s, o);
    if n == 0 {
        return String::new();
    }
    msg_residue_left(s.dir, n)
}

/// 邻居 errno 的独立解释（本卡只引进，不重判）。
pub fn msg_foreign(s: &Shape, o: &Outcome) -> String {
    if o.default || s.errno_seen == ERR_NONE || s.errno_seen == ERR_ENOSPC {
        return String::new();
    }
    msg_foreign_stop(s.file, s.errno_seen)
}

/// 前缀（判据里回显用）。
pub fn sed_head() -> String {
    sed_prefix(Tool::Gnu)
}

/// 段基址文本。
pub fn seg_label() -> String {
    let mut s = String::from("0x");
    s.push_str(seg_text().as_str());
    s
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
    /// 原件名下字节。
    pub value: u32,
    /// 文案落点。
    pub note: Note,
    /// 归因位。
    pub default: bool,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim {
            stage: Stage::Exit,
            code: EXIT_OK,
            value: 0,
            note: Note::Complete,
            default: false,
        }
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

/// 8 个可正确申报的形状 + 6 个复刻形状（复刻自 `gnu-bare-write-enospc`）。
///
/// 活槽的字节账一律用 `size != want`：编辑会改变尺寸，于是"落位格报 `want`、
/// 中止格报 `size`/`0`"三件事不可能靠同一个数蒙过去。
pub const SHAPES: [Shape; CORPUS] = [
    // 0 —— 无后缀轮换全量落位（本卡正判据锚）。
    Shape {
        tag: "gnu-bare-rotated",
        dir: "cfg.d",
        file: "cfg.conf",
        suffix: "",
        tool: Tool::Gnu,
        rotate: Rotate::Bare,
        size: 3072,
        want: 4096,
        written: 4096,
        argv_bad: false,
        follow_symlinks: false,
        open_ok: true,
        not_regular: false,
        backup_err: false,
        mktemp_ok: true,
        write_err: false,
        close_err: false,
        attr_err: false,
        rename_err: false,
        errno_seen: ERR_NONE,
        want_attrs: A_ALL,
        attr_failed: A_NONE,
        hardlinks: 1,
        is_symlink: false,
        root_caller: true,
    },
    // 1 —— 带后缀轮换全量落位：新内容在原名下，旧内容整体在 `.bak` 里。
    Shape {
        tag: "gnu-suffix-rotated",
        dir: "cfg.d",
        file: "cfg.conf",
        suffix: ".bak",
        tool: Tool::Gnu,
        rotate: Rotate::Suffix,
        size: 3072,
        want: 4096,
        written: 4096,
        argv_bad: false,
        follow_symlinks: false,
        open_ok: true,
        not_regular: false,
        backup_err: false,
        mktemp_ok: true,
        write_err: false,
        close_err: false,
        attr_err: false,
        rename_err: false,
        errno_seen: ERR_NONE,
        want_attrs: A_ALL,
        attr_failed: A_NONE,
        hardlinks: 1,
        is_symlink: false,
        root_caller: true,
    },
    // 2 —— 落位成功但硬链接被斩（inode 已换，第二个名字还压在旧 inode 上）。
    Shape {
        tag: "gnu-bare-link-cut",
        dir: "cfg.d",
        file: "cfg.conf",
        suffix: "",
        tool: Tool::Gnu,
        rotate: Rotate::Bare,
        size: 3072,
        want: 4096,
        written: 4096,
        argv_bad: false,
        follow_symlinks: false,
        open_ok: true,
        not_regular: false,
        backup_err: false,
        mktemp_ok: true,
        write_err: false,
        close_err: false,
        attr_err: false,
        rename_err: false,
        errno_seen: ERR_NONE,
        want_attrs: A_ALL,
        attr_failed: A_NONE,
        hardlinks: 2,
        is_symlink: false,
        root_caller: true,
    },
    // 3 —— 属性尾段：owner 没搬成，GNU 照常落位，退出码却是 4。
    Shape {
        tag: "gnu-suffix-attr-owner",
        dir: "cfg.d",
        file: "cfg.conf",
        suffix: ".bak",
        tool: Tool::Gnu,
        rotate: Rotate::Suffix,
        size: 3072,
        want: 4096,
        written: 4096,
        argv_bad: false,
        follow_symlinks: false,
        open_ok: true,
        not_regular: false,
        backup_err: false,
        mktemp_ok: true,
        write_err: false,
        close_err: false,
        attr_err: true,
        rename_err: false,
        errno_seen: ERR_EPERM,
        want_attrs: A_ALL,
        attr_failed: A_OWNER,
        hardlinks: 1,
        is_symlink: false,
        root_caller: false,
    },
    // 4 —— busybox 用法族：选项不认，文件系统一次都没碰。
    Shape {
        tag: "bb-bad-option",
        dir: "cfg.d",
        file: "cfg.conf",
        suffix: "",
        tool: Tool::Busybox,
        rotate: Rotate::Bare,
        size: 512,
        want: 512,
        written: 0,
        argv_bad: true,
        follow_symlinks: false,
        open_ok: true,
        not_regular: false,
        backup_err: false,
        mktemp_ok: true,
        write_err: false,
        close_err: false,
        attr_err: false,
        rename_err: false,
        errno_seen: ERR_NONE,
        want_attrs: A_MODE,
        attr_failed: A_NONE,
        hardlinks: 1,
        is_symlink: false,
        root_caller: false,
    },
    // 5 —— 无后缀写出中途满盘：原件一个字没动（J1 的"原件不动"锚位）。
    Shape {
        tag: "gnu-bare-write-enospc",
        dir: "cfg.d",
        file: "cfg.conf",
        suffix: "",
        tool: Tool::Gnu,
        rotate: Rotate::Bare,
        size: 3072,
        want: 4096,
        written: 2048,
        argv_bad: false,
        follow_symlinks: false,
        open_ok: true,
        not_regular: false,
        backup_err: false,
        mktemp_ok: true,
        write_err: true,
        close_err: false,
        attr_err: false,
        rename_err: false,
        errno_seen: ERR_ENOSPC,
        want_attrs: A_ALL,
        attr_failed: A_NONE,
        hardlinks: 1,
        is_symlink: false,
        root_caller: true,
    },
    // 6 —— 带后缀写出中途满盘：名字已被 rename 走，原件名下 0 字节。
    Shape {
        tag: "gnu-suffix-write-enospc",
        dir: "cfg.d",
        file: "cfg.conf",
        suffix: ".bak",
        tool: Tool::Gnu,
        rotate: Rotate::Suffix,
        size: 3072,
        want: 4096,
        written: 2048,
        argv_bad: false,
        follow_symlinks: false,
        open_ok: true,
        not_regular: false,
        backup_err: false,
        mktemp_ok: true,
        write_err: true,
        close_err: false,
        attr_err: false,
        rename_err: false,
        errno_seen: ERR_ENOSPC,
        want_attrs: A_ALL,
        attr_failed: A_NONE,
        hardlinks: 1,
        is_symlink: false,
        root_caller: true,
    },
    // 7 —— 临时件都建不出来（目录已无数据块且扩不出目录块）。
    Shape {
        tag: "gnu-mktemp-enospc",
        dir: "legacy.d",
        file: "app.conf",
        suffix: "",
        tool: Tool::Gnu,
        rotate: Rotate::Bare,
        size: 1024,
        want: 1536,
        written: 0,
        argv_bad: false,
        follow_symlinks: false,
        open_ok: true,
        not_regular: false,
        backup_err: false,
        mktemp_ok: false,
        write_err: false,
        close_err: false,
        attr_err: false,
        rename_err: false,
        errno_seen: ERR_ENOSPC,
        want_attrs: A_ALL,
        attr_failed: A_NONE,
        hardlinks: 1,
        is_symlink: false,
        root_caller: true,
    },
    // 8..13 —— 复刻 `gnu-bare-write-enospc`，每槽只错一维。
    Shape {
        tag: "estate-swap",
        dir: "cfg.d",
        file: "cfg.conf",
        suffix: "",
        tool: Tool::Gnu,
        rotate: Rotate::Bare,
        size: 3072,
        want: 4096,
        written: 2048,
        argv_bad: false,
        follow_symlinks: false,
        open_ok: true,
        not_regular: false,
        backup_err: false,
        mktemp_ok: true,
        write_err: true,
        close_err: false,
        attr_err: false,
        rename_err: false,
        errno_seen: ERR_ENOSPC,
        want_attrs: A_ALL,
        attr_failed: A_NONE,
        hardlinks: 1,
        is_symlink: false,
        root_caller: true,
    },
    Shape {
        tag: "stage-swap",
        dir: "cfg.d",
        file: "cfg.conf",
        suffix: "",
        tool: Tool::Gnu,
        rotate: Rotate::Bare,
        size: 3072,
        want: 4096,
        written: 2048,
        argv_bad: false,
        follow_symlinks: false,
        open_ok: true,
        not_regular: false,
        backup_err: false,
        mktemp_ok: true,
        write_err: true,
        close_err: false,
        attr_err: false,
        rename_err: false,
        errno_seen: ERR_ENOSPC,
        want_attrs: A_ALL,
        attr_failed: A_NONE,
        hardlinks: 1,
        is_symlink: false,
        root_caller: true,
    },
    Shape {
        tag: "code-swap",
        dir: "cfg.d",
        file: "cfg.conf",
        suffix: "",
        tool: Tool::Gnu,
        rotate: Rotate::Bare,
        size: 3072,
        want: 4096,
        written: 2048,
        argv_bad: false,
        follow_symlinks: false,
        open_ok: true,
        not_regular: false,
        backup_err: false,
        mktemp_ok: true,
        write_err: true,
        close_err: false,
        attr_err: false,
        rename_err: false,
        errno_seen: ERR_ENOSPC,
        want_attrs: A_ALL,
        attr_failed: A_NONE,
        hardlinks: 1,
        is_symlink: false,
        root_caller: true,
    },
    Shape {
        tag: "value-swap",
        dir: "cfg.d",
        file: "cfg.conf",
        suffix: "",
        tool: Tool::Gnu,
        rotate: Rotate::Bare,
        size: 3072,
        want: 4096,
        written: 2048,
        argv_bad: false,
        follow_symlinks: false,
        open_ok: true,
        not_regular: false,
        backup_err: false,
        mktemp_ok: true,
        write_err: true,
        close_err: false,
        attr_err: false,
        rename_err: false,
        errno_seen: ERR_ENOSPC,
        want_attrs: A_ALL,
        attr_failed: A_NONE,
        hardlinks: 1,
        is_symlink: false,
        root_caller: true,
    },
    Shape {
        tag: "note-swap",
        dir: "cfg.d",
        file: "cfg.conf",
        suffix: "",
        tool: Tool::Gnu,
        rotate: Rotate::Bare,
        size: 3072,
        want: 4096,
        written: 2048,
        argv_bad: false,
        follow_symlinks: false,
        open_ok: true,
        not_regular: false,
        backup_err: false,
        mktemp_ok: true,
        write_err: true,
        close_err: false,
        attr_err: false,
        rename_err: false,
        errno_seen: ERR_ENOSPC,
        want_attrs: A_ALL,
        attr_failed: A_NONE,
        hardlinks: 1,
        is_symlink: false,
        root_caller: true,
    },
    Shape {
        tag: "default-swap",
        dir: "cfg.d",
        file: "cfg.conf",
        suffix: "",
        tool: Tool::Gnu,
        rotate: Rotate::Bare,
        size: 3072,
        want: 4096,
        written: 2048,
        argv_bad: false,
        follow_symlinks: false,
        open_ok: true,
        not_regular: false,
        backup_err: false,
        mktemp_ok: true,
        write_err: true,
        close_err: false,
        attr_err: false,
        rename_err: false,
        errno_seen: ERR_ENOSPC,
        want_attrs: A_ALL,
        attr_failed: A_NONE,
        hardlinks: 1,
        is_symlink: false,
        root_caller: true,
    },
];

/// 与 `SHAPES` 一一对应的申报。
///
/// 前 8 槽逐维等于现场；后 6 槽各只错一维，错的那维依次是
/// estate/stage/code/value/note/default。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim { stage: Stage::Exit, code: 0, value: 4096, note: Note::Complete, default: false },
    Claim { stage: Stage::Exit, code: 0, value: 4096, note: Note::Complete, default: false },
    Claim { stage: Stage::Exit, code: 0, value: 4096, note: Note::LinkCut, default: false },
    Claim { stage: Stage::Attr, code: 4, value: 4096, note: Note::AttrOwner, default: false },
    Claim { stage: Stage::Argv, code: 1, value: 512, note: Note::BadOption, default: false },
    Claim { stage: Stage::Write, code: 4, value: 3072, note: Note::WriteEnospc, default: true },
    Claim { stage: Stage::Write, code: 4, value: 0, note: Note::WriteEnospc, default: true },
    Claim { stage: Stage::Mktemp, code: 4, value: 1024, note: Note::MktempFail, default: true },
    Claim { stage: Stage::Exit, code: 0, value: 4096, note: Note::Complete, default: false },
    Claim { stage: Stage::Close, code: 4, value: 3072, note: Note::WriteEnospc, default: true },
    Claim { stage: Stage::Write, code: 2, value: 3072, note: Note::WriteEnospc, default: true },
    Claim { stage: Stage::Write, code: 4, value: 0, note: Note::WriteEnospc, default: true },
    Claim { stage: Stage::Write, code: 4, value: 3072, note: Note::CloseEnospc, default: true },
    Claim { stage: Stage::Write, code: 4, value: 3072, note: Note::WriteEnospc, default: false },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Stage, Fault::Code, Fault::Value, Fault::Note, Fault::Default];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] = [
    SI_ESTATE,
    SI_ESTAGE,
    SI_ECODE,
    SI_EVALUE,
    SI_ENOTE,
    SI_EDEFAULT,
];

/// 账本 8 个成功格的值（总数、成、败、落位档、有话档、停档、中止档、红档 ppm）。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 2, 1, 2, 3, 929];

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
        return Err(SI_ENOBS);
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

/// 对判：状态 → 阶段 → 退出码 → 原件字节 → 文案落点 → 归因位。
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

/// 逐槽释义（把三个字节数一并写进一行，供审计面读）。
pub fn detail(c: &Case) -> String {
    let o = c.actual();
    let msg = message(&c.shape, &o);
    let mut s = String::from(c.shape.file);
    s.push_str(" tool=");
    s.push_str(c.shape.tool.text());
    s.push_str(" mode=");
    s.push_str(c.shape.rotate.name());
    s.push_str(" stage=");
    s.push_str(o.stage.name());
    s.push_str(" exit=");
    s.push_str(dec_text(o.code).as_str());
    s.push_str(" orig=");
    s.push_str(dec_text(o.value).as_str());
    s.push_str(" backup=");
    s.push_str(dec_text(backup_bytes(&c.shape, &o)).as_str());
    s.push_str(" residue=");
    s.push_str(dec_text(leftover(&c.shape, &o)).as_str());
    s.push_str(" lost=");
    s.push_str(dec_text(deferred_lost(&c.shape, &o)).as_str());
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
/// 这份表是把 `SHAPES`/`CLAIMS` 用**另一种形式**再写一遍：申报表说的是"我声明
/// 这次轮换停在哪一闸"，期望表说的是"校验器按纵线实际会走到哪一闸"。两者一致
/// 才算数，任一处笔误都会在这里露出来。
const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Attr,
    Stage::Argv,
    Stage::Write,
    Stage::Write,
    Stage::Mktemp,
    Stage::Write,
    Stage::Write,
    Stage::Write,
    Stage::Write,
    Stage::Write,
    Stage::Write,
];

/// 14 槽的期望退出码。
const WANT_CODE: [u32; CORPUS] = [0, 0, 0, 4, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4];

/// 14 槽的期望原件名下字节。
const WANT_VALUE: [u32; CORPUS] = [
    4096, 4096, 4096, 4096, 512, 3072, 0, 1024, 3072, 3072, 3072, 3072, 3072, 3072,
];

/// 14 槽的期望文案落点。
const WANT_NOTE: [Note; CORPUS] = [
    Note::Complete,
    Note::Complete,
    Note::LinkCut,
    Note::AttrOwner,
    Note::BadOption,
    Note::WriteEnospc,
    Note::WriteEnospc,
    Note::MktempFail,
    Note::WriteEnospc,
    Note::WriteEnospc,
    Note::WriteEnospc,
    Note::WriteEnospc,
    Note::WriteEnospc,
    Note::WriteEnospc,
];

/// 14 槽的期望归因位。
const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, false, true, true, true, true, true, true, true, true, true,
];

/// 14 槽的期望档位下标（把档位也用第二种形式复写一遍）。
const WANT_NDX: [u8; CORPUS] = [0, 0, 1, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3];

/// 14 槽的期望备份件字节（第三份交叉陈述：只有越过备份闸的带后缀形状才有值）。
const WANT_BACKUP: [u32; CORPUS] = [
    0, 3072, 0, 3072, 0, 0, 3072, 0, 0, 0, 0, 0, 0, 0,
];

/// 14 槽的期望临时件残痕字节（GNU 全 0——本卡"临时清理"那一半承诺）。
const WANT_RESIDUE: [u32; CORPUS] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

/// 14 槽标签。
const WANT_TAGS: [&str; CORPUS] = [
    "gnu-bare-rotated",
    "gnu-suffix-rotated",
    "gnu-bare-link-cut",
    "gnu-suffix-attr-owner",
    "bb-bad-option",
    "gnu-bare-write-enospc",
    "gnu-suffix-write-enospc",
    "gnu-mktemp-enospc",
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

/// 本卡唯一被复刻的现场标签。
const BASE_TAG: &str = "gnu-bare-write-enospc";

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
/// 分档只看**实际结果**落进哪一档，申报正确与否单独计。本卡的档按
/// "归因位 + 退出码 + 文案族"投影，因此 busybox 吞掉 close 错误那一格落进
/// `noisy` 而不是 `aborted`——工具的结论本来就是码 0，把它硬塞进中止档就等于替
/// busybox 补了它没做的归因。属性失败但已落位那一格落进 `halted` 而不是
/// `aborted`：它码非 0，却没有 ENOSPC 归因。红档槽仍留在自己那一档里，
/// 覆盖面才看得见。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行（rotated/noisy/halted/aborted）。
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
        let mut s = String::new();
        let mut i = 0usize;
        while i < TIERS.len() {
            if let Some(t) = TIERS.get(i) {
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
    /// 落位档正确数。
    pub rotated: u32,
    /// 有话档正确数。
    pub noisy: u32,
    /// 停档正确数。
    pub halted: u32,
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
            Err(SI_ECOUNT)
        }
    }

    /// 码 0 档合计（退出码为成功的那两档）。
    pub fn zero_cells(&self) -> u32 {
        self.rotated + self.noisy
    }

    /// 码非 0 档合计。
    pub fn nonzero_cells(&self) -> u32 {
        self.halted + self.aborted
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("sed-i-rotate corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" rotated=");
        s.push_str(u32_text(self.rotated).as_str());
        s.push_str(" noisy=");
        s.push_str(u32_text(self.noisy).as_str());
        s.push_str(" halted=");
        s.push_str(u32_text(self.halted).as_str());
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
const RENDER_OK: &str = "sed-i-rotate corpus=14 accept=8 reject=6 rotated=2 noisy=1 halted=2 aborted=3 match=14 rate=1000ppm\n";

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 可注入一槽缺陷的全跑。
///
/// 注入的作用是把"期望表"本身也放进被测面：强行宣称某槽犯了另一种缺陷，报表
/// 就必须掉 matched、rate 随之离开 1000。若注入后仍报全绿，说明 matched 是
/// 硬编码的，而不是算出来的。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(SI_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut rotated = 0u32;
    let mut noisy = 0u32;
    let mut halted = 0u32;
    let mut aborted = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(SI_ENOBS),
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
            None => return Err(SI_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(&a) {
                Tier::Rotated => rotated += 1,
                Tier::Noisy => noisy += 1,
                Tier::Halted => halted += 1,
                Tier::Aborted => aborted += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, rotated, noisy, halted, aborted, matched })
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门。
///
/// 观测纵线是 `argv → open → backup → mktemp → write → close → attr → rename
/// → exit`，九段只能向前。回退（例如已经判死在落位段又声称停在写出段）说明申报
/// 者不是按闸顺序观测的，而是拿着文案倒推——这类申报在真实故障里最危险，因为
/// 文案看着完全合理，字节数也能对上，只有闸序暴露它没走过备份闸。
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
            return Err(SI_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(SI_ESTAGE);
        }
        self.last = o;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }

    /// 关闭，返回段数。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(SI_ESTATE);
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
            return Err(SI_ESLOT);
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

    /// 逐格对账：槽位连续、成格值等于 `VALS`、败格码等于 `REJECT_CODES`、
    /// 败格值必归零。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(SI_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SI_EBAL),
            };
            if s.slot as usize != i {
                return Err(SI_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SI_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SI_EVALUE);
                        }
                    }
                    None => return Err(SI_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(SI_EVALUE);
                }
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SI_EVALUE);
                        }
                    }
                    None => return Err(SI_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(SI_EBAL);
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

    /// 冻结。账不满十四维视为账不平；已冻结再冻结是状态违例并计入拒绝。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SI_ESTATE);
        }
        if self.len != SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SI_EBAL);
        }
        self.audit()?;
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
            Err(SI_ESEAL)
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
    let _ = l.register(count_cell(3, OP_BACKUP, r.map(|x| x.rotated)));
    let _ = l.register(count_cell(4, OP_OPEN, r.map(|x| x.noisy)));
    let _ = l.register(count_cell(5, OP_ARGV, r.map(|x| x.halted)));
    let _ = l.register(count_cell(6, OP_MKTEMP, r.map(|x| x.aborted)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_REPLAY, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_CLOSE,
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
/// 1. 表路——段基址、枚举、目录画像、形状合法域；
/// 2. 判路——九闸纵线、原件保护三元组、对判、矩阵；
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

/// Stage 由 note 反查的别名（供枚举谓词书写；与 Note::stage 同址）。
impl Note {
    /// 该落点是否有诊断行（成功族与吞错族无行）。
    pub fn has_message(self) -> bool {
        !matches!(
            self,
            Note::Complete | Note::LinkCut | Note::SymlinkRestored | Note::CloseSwallowed
        )
    }
}

// ---------------------------------------------------------------------------
// 判据谓词（私有；一格一闸，与 run_unx11686_checks 的十三格一一对应）
// ---------------------------------------------------------------------------

/// 闸门纵线的第二种陈述：把 `define` 的闸序摊成"第一个拦下的闸"。
///
/// `define` 说的是"走到这里就停"，这里说的是"按九道闸的顺序找第一个置错的闸"。
/// 两者一致才说明纵线不是靠某几个 `if` 的书写顺序蒙对的。特别地：
///
/// * 落定闸只在**工具会报 close 错误**时才算拦下——busybox 不报，于是它越过
///   落定闸继续走到退出闸，这一格的现场形态就是"表观落位、延迟块无人记账"。
/// * 静默轮换（写了但少字节且没人报错）挂在写出闸，因为盘上少的那一段正是
///   写出段欠下的。
fn first_flag(s: &Shape) -> u8 {
    if s.argv_bad || !s.rotate.rotates() {
        return Stage::Argv.order();
    }
    if !s.open_ok || s.not_regular {
        return Stage::Open.order();
    }
    if s.backup_err {
        return Stage::Backup.order();
    }
    if !s.mktemp_ok {
        return Stage::Mktemp.order();
    }
    if s.write_err {
        return Stage::Write.order();
    }
    if s.rotate.uses_tempfile()
        && s.written < s.want
        && !s.close_err
        && !s.attr_err
        && !s.rename_err
    {
        return Stage::Write.order();
    }
    if s.close_err && s.tool.reports_close_error() {
        return Stage::Close.order();
    }
    if s.attr_err {
        return Stage::Attr.order();
    }
    if s.rename_err {
        return Stage::Rename.order();
    }
    Stage::Exit.order()
}

/// 取一槽形状（谓词一律走 `.get()`，越界即判红）。
fn shape_at(i: usize) -> Option<Shape> {
    SHAPES.get(i).copied()
}

/// 两个字符串数组是否逐项相等（标签表的交叉核对用）。
fn tags_equal(a: &[&str], b: &[&str]) -> bool {
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

/// 一枚形状在给定位置上的退出码应当是几（用法族 vs 文件族的分界）。
fn expect_code_for(tool: Tool, note: Note) -> u32 {
    if note.prelude_family() && note != Note::BackupFail && note != Note::TempHomeless {
        return tool.usage_exit();
    }
    if note.noisy_success() {
        return EXIT_OK;
    }
    tool.io_exit()
}

/// SI01：卡名、段基址与十三个错误码。
///
/// 十三码必须**连续、升序、互不相同**，且都落在本卡段内；`Fault::code` 的映射
/// 与常量表逐一对齐；缺陷短名不许撞车。段基址写错会让两卡的码在同一数字上
/// 相遇，那时任何一份账都读不出是谁报的红。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d320 {
        return false;
    }
    if CORPUS != 14 || SAMPLES != 14 || ACCEPT != 8 || REJECT != 6 || TIER_COUNT != 4 {
        return false;
    }
    if ACCEPT + REJECT != CORPUS || SAMPLES != CORPUS {
        return false;
    }
    // 十三码按申报次序排成一列，逐格检查连续性。
    let codes = [
        SI_ESTAGE,
        SI_ECODE,
        SI_EVALUE,
        SI_ENOTE,
        SI_EDEFAULT,
        SI_EABORT,
        SI_ESTATE,
        SI_ECOUNT,
        SI_ESLOT,
        SI_ESEAL,
        SI_ENOBS,
        SI_EMATRIX,
        SI_EBAL,
    ];
    if codes.len() != 13 {
        return false;
    }
    let mut i = 0usize;
    while i < codes.len() {
        let c = match codes.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want = match SEG_BASE.checked_add(i as u32).and_then(|x| x.checked_add(1)) {
            Some(v) => v,
            None => return false,
        };
        if c != want {
            return false;
        }
        if c <= SEG_BASE {
            return false;
        }
        i += 1;
    }
    if codes.len() != 13 {
        return false;
    }
    // 两两互不相同（段内不许有两个码指向同一数字）。
    let mut a = 0usize;
    while a < 13 {
        let mut b = a + 1;
        while b < 13 {
            let x = match codes.get(a) {
                Some(v) => *v,
                None => return false,
            };
            let y = match codes.get(b) {
                Some(v) => *v,
                None => return false,
            };
            if x == y {
                return false;
            }
            b += 1;
        }
        a += 1;
    }
    // 十四个缺陷种类：`Ok` 必须是唯一的零码，其余十二个各领一码。
    let mut zero = 0usize;
    let mut i = 0usize;
    while i < FAULTS.len() {
        let f = match FAULTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if f.code() == 0 {
            zero += 1;
            if !matches!(f, Fault::Ok) {
                return false;
            }
        } else if f.code() <= SEG_BASE || f.code() > SI_EBAL {
            return false;
        }
        if fault_short(f).is_empty() {
            return false;
        }
        if f.alive() != (f.code() != 0) {
            return false;
        }
        i += 1;
    }
    if FAULTS.len() != 14 || zero != 1 {
        return false;
    }
    // 六个申报缺陷码互不相同，且都在本卡段内。
    let mut a = 0usize;
    while a < REJECT_CODES.len() {
        let x = match REJECT_CODES.get(a) {
            Some(v) => *v,
            None => return false,
        };
        if x <= SEG_BASE || x > SI_EBAL {
            return false;
        }
        let mut b = a + 1;
        while b < REJECT_CODES.len() {
            if let Some(y) = REJECT_CODES.get(b) {
                if *y == x {
                    return false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    if REJECT_CODES.len() != REJECT || REJECT_FAULTS.len() != REJECT {
        return false;
    }
    // 六个申报缺陷与六个缺陷码一一对应。
    let mut i = 0usize;
    while i < REJECT {
        let f = match REJECT_FAULTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let c = match REJECT_CODES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if f.code() != c || !f.alive() {
            return false;
        }
        i += 1;
    }
    // 段基址文本与本卡段一致。
    let label = seg_label();
    if !label.starts_with("0x6d32") {
        return false;
    }
    if sed_head().as_str() != "sed: " {
        return false;
    }
    true
}

/// SI02：四族枚举与九道闸的次序。
///
/// 这一格钉的是"符号系统不自相矛盾"：阶段秩必须一一对应、落点族互斥、档位投影
/// 全覆盖、工具面旗标的分叉彼此吻合。本卡最要紧的三处枚举级约定：
///
/// * `residue_plane` 从 `mktemp` 起算，**不含** `backup`——于是"备份闸上盘里还
///   没有 `sedXXXXXX`"这件事写进了类型，而不只是写在注释里。
/// * `abort_plane` 不含 `attr`——属性搬迁不占数据块，报的是邻居 errno。
/// * `Note::landed()` 只含三个落位族文案，落定被吞那一格算成功档（工具自己
///   报 0，替它补归因就是伪造现场）。
fn enums_ok() -> bool {
    if STAGES.len() != 9 || NOTES.len() != 20 || TIERS.len() != TIER_COUNT {
        return false;
    }
    // 阶段：秩 ↔ 枚举一一对应，且 `at` 是 `order` 的逆。
    let mut i = 0usize;
    while i < STAGES.len() {
        let s = match STAGES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.order() as usize != i {
            return false;
        }
        if s.name().is_empty() {
            return false;
        }
        let back = match Stage::at(s.order()) {
            Some(v) => v,
            None => return false,
        };
        if back != s {
            return false;
        }
        if Stage::at(9).is_some() {
            return false;
        }
        // 失败落点与退出聚合互斥。
        if s.failure() == (s == Stage::Exit) {
            return false;
        }
        i += 1;
    }
    // 闸序：备份在写之前，落定在写之后，属性在落位之前。
    if Stage::Backup.order() >= Stage::Write.order() {
        return false;
    }
    if Stage::Mktemp.order() <= Stage::Backup.order() {
        return false;
    }
    if Stage::Close.order() <= Stage::Write.order() {
        return false;
    }
    if Stage::Attr.order() >= Stage::Rename.order() {
        return false;
    }
    if Stage::Rename.order() >= Stage::Exit.order() {
        return false;
    }
    // 残痕责任从建件闸起算；备份闸上没有临时件。
    let mut i = 0usize;
    while i < STAGES.len() {
        let s = match STAGES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.residue_plane() && s.order() < Stage::Mktemp.order() {
            return false;
        }
        if s == Stage::Backup && s.residue_plane() {
            return false;
        }
        // 归因闸都在原件语义闸内（属性闸与退出闸不承载 ENOSPC 归因）。
        if s.abort_plane() && !s.orig_plane() {
            return false;
        }
        if s == Stage::Attr && (s.abort_plane() || s.orig_plane()) {
            return false;
        }
        if s == Stage::Exit && (s.abort_plane() || s.orig_plane() || s.residue_plane()) {
            return false;
        }
        i += 1;
    }
    // 落点：文本唯一、族互斥、属性位与尾段族一一对应。
    let mut i = 0usize;
    while i < NOTES.len() {
        let n = match NOTES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if n.name().is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < NOTES.len() {
            if let Some(m) = NOTES.get(j) {
                if m.name() == n.name() {
                    return false;
                }
            }
            j += 1;
        }
        let fam = n.success() as u32
            + n.abort_family() as u32
            + n.tail_family() as u32
            + n.prelude_family() as u32
            + n.defect_family() as u32;
        if fam != 1 {
            return false;
        }
        // 成功族里只有"无声落位"是干净档，其余三格都有话要说。
        if n.success() && n.noisy_success() != (n != Note::Complete) {
            return false;
        }
        if n.noisy_success() && !n.name().contains('-') {
            return false;
        }
        // 落位族只能是成功族；落定被吞那一格算成功但不算落位族的字节。
        if n.landed() && !n.success() {
            return false;
        }
        if n.attr_bit() != A_NONE && !n.tail_family() {
            return false;
        }
        if n.tail_family() && n.attr_bit() == A_NONE {
            return false;
        }
        if n.abort_family() && n.stage().abort_plane() == false {
            return false;
        }
        if n.stage() == Stage::Mktemp && !n.prelude_family() && !n.abort_family() {
            return false;
        }
        i += 1;
    }
    // 档位：下标一一对应，成功档恰是码 0 的两档。
    let mut i = 0usize;
    while i < TIERS.len() {
        let t = match TIERS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if t.index() != i {
            return false;
        }
        if t.success() != (i < 2) {
            return false;
        }
        if t.aborted() != matches!(t, Tier::Aborted) {
            return false;
        }
        if t.name().is_empty() {
            return false;
        }
        i += 1;
    }
    // 档位投影：归因位优先，其次码 0 分"无声/有话"，最后才是不归因的红。
    let probe = [
        Outcome { stage: Stage::Exit, code: EXIT_OK, value: 4096, note: Note::Complete, default: false },
        Outcome { stage: Stage::Exit, code: EXIT_OK, value: 4096, note: Note::LinkCut, default: false },
        Outcome { stage: Stage::Exit, code: EXIT_OK, value: 4096, note: Note::CloseSwallowed, default: false },
        Outcome { stage: Stage::Argv, code: EXIT_USAGE, value: 512, note: Note::BadOption, default: false },
        Outcome { stage: Stage::Write, code: EXIT_IO, value: 3072, note: Note::WriteEnospc, default: true },
        // 归因位压倒码 0：这是"工具说没事、账本说有事"的一格，必须进中止档。
        Outcome { stage: Stage::Exit, code: EXIT_OK, value: 0, note: Note::Complete, default: true },
    ];
    let want_tier = [
        Tier::Rotated,
        Tier::Noisy,
        Tier::Noisy,
        Tier::Halted,
        Tier::Aborted,
        Tier::Aborted,
    ];
    let mut i = 0usize;
    while i < probe.len() {
        let p = match probe.get(i) {
            Some(v) => v,
            None => return false,
        };
        let w = match want_tier.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if Tier::of(p) != w {
            return false;
        }
        if p.ok() != (p.code == EXIT_OK && p.note.success()) {
            return false;
        }
        i += 1;
    }
    // 轮换模式：只有带后缀才把原件名字移走；两种 in-place 都要临时件。
    if Rotate::None.rotates() || Rotate::None.moves_original() || Rotate::None.uses_tempfile() {
        return false;
    }
    if !Rotate::Bare.rotates() || Rotate::Bare.moves_original() || !Rotate::Bare.uses_tempfile() {
        return false;
    }
    if !Rotate::Suffix.rotates() || !Rotate::Suffix.moves_original() || !Rotate::Suffix.uses_tempfile()
    {
        return false;
    }
    if Rotate::Bare.name() == Rotate::Suffix.name() {
        return false;
    }
    // 工具面：两套 sed 的分叉必须成对出现，不许有一面"两个都对"。
    if Tool::Gnu.io_exit() != EXIT_IO || Tool::Busybox.io_exit() != EXIT_BB {
        return false;
    }
    if Tool::Gnu.usage_exit() != EXIT_USAGE || Tool::Busybox.usage_exit() != EXIT_BB {
        return false;
    }
    if Tool::Gnu.cleans_temp_on_fail() == Tool::Busybox.cleans_temp_on_fail() {
        return false;
    }
    if Tool::Gnu.rotates_after_attr_fail() == Tool::Busybox.rotates_after_attr_fail() {
        return false;
    }
    if Tool::Gnu.reports_close_error() == Tool::Busybox.reports_close_error() {
        return false;
    }
    if Tool::Busybox.supports_backup_suffix() || !Tool::Gnu.supports_backup_suffix() {
        return false;
    }
    if Tool::Busybox.supports_follow_symlinks() || !Tool::Gnu.supports_follow_symlinks() {
        return false;
    }
    if Tool::Busybox.attempts_chown() {
        return false;
    }
    if Tool::Gnu.attr_ceiling() != A_ALL || Tool::Busybox.attr_ceiling() != A_MODE {
        return false;
    }
    if attr_name(A_OWNER) == attr_name(A_MODE) || attr_name(A_NONE) != "none" {
        return false;
    }
    if attr_popcount(A_NONE) != 0 || attr_popcount(A_ALL) != A_BIT_COUNT {
        return false;
    }
    if attr_popcount(A_OWNER) != 1 || attr_popcount(A_MODE) != 1 {
        return false;
    }
    if !attr_subset(A_NONE, A_NONE) || attr_subset(A_ALL, A_MODE) {
        return false;
    }
    if ATTR_BITS.len() != A_BIT_COUNT {
        return false;
    }
    true
}

/// SI03：三块目录画像与本卡结构闸的载体。
///
/// `sed -i` 的空间账有两处不在"数据块"这一维上：备份改名与最终改名都要往目录
/// 里**加一个名字**。所以画像必须分得开三种现场——
/// `cfg.d` 满但目录树可扩（改名照成，红在写出段）、`legacy.d` 满且目录不可扩
/// （改名会真返 ENOSPC，这是 `rename` 闸存在的全部理由）、`ro.d` 不可写
/// （`mkstemp` 连件都建不出来，红得跟空间无关）。
fn profile_ok() -> bool {
    let ps = dir_profiles();
    if ps.len() != 3 {
        return false;
    }
    if DirProfile::blank().well_formed() {
        return false;
    }
    let mut i = 0usize;
    while i < ps.len() {
        let p = match ps.get(i) {
            Some(v) => v,
            None => return false,
        };
        if !p.well_formed() {
            return false;
        }
        if p.dir.is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < ps.len() {
            if let Some(q) = ps.get(j) {
                if q.dir == p.dir || q.seal() == p.seal() {
                    return false;
                }
            }
            j += 1;
        }
        // 空闲字节要么精确算得出、要么明确算不出，不许"看起来是 0"。
        match p.free_bytes() {
            Some(v) => {
                if (v == 0) != p.data_full() {
                    return false;
                }
            }
            None => return false,
        }
        // 数据满 + 目录可扩 才能加名字；不可扩就必须在改名闸报 ENOSPC。
        if p.can_add_entry() != (p.writable && (p.data_free > 0 || p.dir_can_grow)) {
            return false;
        }
        if p.can_mktemp() != (p.can_add_entry() && p.writable) {
            return false;
        }
        if p.close_can_fail() != p.delalloc {
            return false;
        }
        if p.seal() == 0 {
            return false;
        }
        i += 1;
    }
    let cfg = match ps.get(0) {
        Some(v) => v,
        None => return false,
    };
    let legacy = match ps.get(1) {
        Some(v) => v,
        None => return false,
    };
    let ro = match ps.get(2) {
        Some(v) => v,
        None => return false,
    };
    if cfg.dir != "cfg.d" || legacy.dir != "legacy.d" || ro.dir != "ro.d" {
        return false;
    }
    // 满盘但目录树能长：改名成功，红只能在写出段——本卡正判据的地基。
    if !cfg.data_full() || !cfg.dir_can_grow || !cfg.can_add_entry() {
        return false;
    }
    if !cfg.delalloc || !cfg.close_can_fail() {
        return false;
    }
    // 满盘且目录扩不出：`rename` 真会 ENOSPC，落位闸必须有存在理由。
    if !legacy.data_full() || legacy.dir_can_grow || legacy.can_add_entry() {
        return false;
    }
    if !legacy.can_mktemp() || legacy.delalloc {
        return false;
    }
    // 不可写：与空间无关的建件失败，不许被算成 ENOSPC 归因。
    if ro.writable || ro.data_full() || ro.free_bytes().is_none() {
        return false;
    }
    if ro.can_add_entry() || ro.can_mktemp() {
        return false;
    }
    match ro.free_bytes() {
        Some(v) => {
            if v != 4096 {
                return false;
            }
        }
        None => return false,
    }
    // 坏画像必须露出来：块大小非法、目录名空、空闲数溢出都不许进判据。
    let bad = [
        DirProfile {
            dir: "",
            bsize: 4096,
            data_free: 1,
            dir_can_grow: true,
            delalloc: false,
            writable: true,
        },
        DirProfile {
            dir: "odd.d",
            bsize: 700,
            data_free: 1,
            dir_can_grow: true,
            delalloc: false,
            writable: true,
        },
        DirProfile {
            dir: "ovf.d",
            bsize: 4096,
            data_free: SIZE_MAX + 1,
            dir_can_grow: true,
            delalloc: false,
            writable: true,
        },
    ];
    let mut i = 0usize;
    while i < bad.len() {
        if let Some(p) = bad.get(i) {
            if p.well_formed() {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// SI04：形状的合法域。
///
/// 本卡的形状有 24 个维度，合法与否不是"看着像"就算数：每个错误旗标都必须与
/// 字节数、errno、轮换模式、属性位、硬链接数**互相自洽**。尤其三条：
///
/// * `is_symlink` 必须配 `follow_symlinks`——不跟随时的替换语义归 F11464，
///   本卡不收那种形状，免得两卡对同一现场各判一次。
/// * `close_err`/`rename_err` 只认 ENOSPC；邻居 errno 造成的落定失败是另一件事。
/// * 属性段一次只塌一位——真实 `sed` 也是逐条报，两位一起塌落不进单一文案。
fn shape_ok() -> bool {
    if !Shape::blank().valid() {
        return false;
    }
    if Shape::blank().seal() == 0 {
        return false;
    }
    if SHAPES.len() != CORPUS {
        return false;
    }
    let mut i = 0usize;
    let mut live_seals: [u64; ACCEPT] = [0u64; ACCEPT];
    let mut n_live = 0usize;
    while i < CORPUS {
        let s = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        if !s.valid() {
            return false;
        }
        if s.tag.is_empty() || s.file.is_empty() || s.dir.is_empty() {
            return false;
        }
        if s.dir != "cfg.d" && s.dir != "legacy.d" && s.dir != "ro.d" {
            return false;
        }
        // 备份后缀只允许两种写法：带后缀非空，其余一律空。
        if s.rotate == Rotate::Suffix && s.suffix.is_empty() {
            return false;
        }
        if s.rotate != Rotate::Suffix && !s.suffix.is_empty() {
            return false;
        }
        // 活槽的形状必须互不相同（复刻槽本来就与基准槽同形）。
        if i < ACCEPT {
            if let Some(slot) = live_seals.get_mut(n_live) {
                *slot = s.seal();
            } else {
                return false;
            }
            n_live += 1;
        }
        i += 1;
    }
    if n_live != ACCEPT {
        return false;
    }
    let mut a = 0usize;
    while a < ACCEPT {
        let x = match live_seals.get(a) {
            Some(v) => *v,
            None => return false,
        };
        if x == 0 {
            return false;
        }
        let mut b = a + 1;
        while b < ACCEPT {
            if let Some(y) = live_seals.get(b) {
                if *y == x {
                    return false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    // 四个克隆器：只许动自己那一维，其余维度必须原样。
    let base = match shape_at(6) {
        Some(v) => v,
        None => return false,
    };
    if base.suffix != ".bak" {
        return false;
    }
    let w = base.clone_written(base.written.saturating_add(1));
    if w.seal() == base.seal() || w.written != base.written + 1 {
        return false;
    }
    let mut diff_dims = 0usize;
    if w.size != base.size {
        diff_dims += 1;
    }
    if w.want != base.want {
        diff_dims += 1;
    }
    if diff_dims != 0 {
        return false;
    }
    let e = base.clone_errno(ERR_EIO);
    if e.seal() == base.seal() || e.errno_seen != ERR_EIO || e.write_err != base.write_err {
        return false;
    }
    let t = base.clone_tool(Tool::Busybox);
    // busybox 不支持备份后缀：克隆出来的形状必须被判为非法。
    if t.valid() {
        return false;
    }
    let r = base.clone_rotate(Rotate::Bare, "");
    if !r.valid() || r.seal() == base.seal() || r.suffix.len() != 0 {
        return false;
    }
    // clone_written 保持原值时形状不变（指纹不许掺随机数）。
    if base.clone_written(base.written).seal() != base.seal() {
        return false;
    }
    // 逐条非法形状：任何一条被判合法，说明合法域漏了一个维度。
    let mut bad = [base; 8];
    if let Some(x) = bad.get_mut(0) {
        x.suffix = "";
    }
    if let Some(x) = bad.get_mut(1) {
        x.written = x.want + 1;
    }
    if let Some(x) = bad.get_mut(2) {
        x.hardlinks = 0;
    }
    if let Some(x) = bad.get_mut(3) {
        x.is_symlink = true;
    }
    if let Some(x) = bad.get_mut(4) {
        x.errno_seen = 21;
    }
    if let Some(x) = bad.get_mut(5) {
        x.attr_failed = A_ALL;
    }
    if let Some(x) = bad.get_mut(6) {
        x.size = SIZE_MAX + 1;
    }
    if let Some(x) = bad.get_mut(7) {
        x.want_attrs = A_NONE;
        x.attr_failed = A_NONE;
        x.attr_err = false;
        x.write_err = false;
        x.written = 0;
    }
    let mut i = 0usize;
    while i < 7 {
        if let Some(x) = bad.get(i) {
            if x.valid() {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    // 第 8 格是"合法化简"：属性段全清、写出段无错，形状应当仍然合法。
    if let Some(x) = bad.get(7) {
        if !x.valid() {
            return false;
        }
    } else {
        return false;
    }
    // 非普通件不许同时挂着写错；未打开必须带 errno。
    let mut m = match shape_at(0) {
        Some(v) => v,
        None => return false,
    };
    m.open_ok = false;
    m.written = 0;
    m.errno_seen = ERR_NONE;
    if m.valid() {
        return false;
    }
    m.errno_seen = ERR_ENOENT;
    if !m.valid() {
        return false;
    }
    m.not_regular = true;
    if m.valid() {
        return false;
    }
    // root 之下 owner 不可能失败（内核不拒 root 的 fchown）。
    let mut p = match shape_at(3) {
        Some(v) => v,
        None => return false,
    };
    p.root_caller = true;
    if p.valid() {
        return false;
    }
    // busybox 要搬 owner 就越界。
    let mut q = match shape_at(0) {
        Some(v) => v,
        None => return false,
    };
    q.tool = Tool::Busybox;
    if q.valid() {
        return false;
    }
    // 未落位时的应得字节：改名走过就是 0，否则是原尺寸。
    if !base.original_moved() || base.untouched_bytes() != 0 {
        return false;
    }
    let bare = match shape_at(5) {
        Some(v) => v,
        None => return false,
    };
    if bare.original_moved() || bare.untouched_bytes() != bare.size {
        return false;
    }
    if halt_value(&bare) != 3072 || halt_value(&base) != 0 {
        return false;
    }
    if base.backup_path().as_str() != "cfg.conf.bak" {
        return false;
    }
    if bare.backup_path().as_str() != "cfg.conf" {
        return false;
    }
    if !bare.temp_path().contains("cfg.d/sedXXXXXX") {
        return false;
    }
    if !bare.rotates() || !base.rotates() {
        return false;
    }
    true
}

/// SI05：九道闸的轮换纵线。
///
/// 本卡的心跳是"**先改名、后写**"。`rename` 不占数据块，所以满盘上备份闸先过，
/// 原件名字先没了，写闸才红。于是同一句"原件不动"在两种 `-i` 形态下是两个不同
/// 的字节数：无后缀报 `size`，带后缀报 `0`（内容全在 `.bak` 里）。这条谓词把
/// 九道闸逐道的正例、`define` 与 `first_flag` 的双陈述、以及"后闸不许改写前闸
/// 结论"钉在一起。
fn define_ok() -> bool {
    let cs = cases();
    // 1. 可复算：同一形状两次判定同指纹；形状变结论必变。
    let mut i = 0usize;
    while i < CORPUS {
        let s = match cs.get(i) {
            Some(v) => v.shape,
            None => return false,
        };
        let a = define(&s);
        let b = define(&s);
        if a.seal() != b.seal() || a.stage != b.stage || a.value != b.value {
            return false;
        }
        if !s.valid() {
            return false;
        }
        // 双陈述：判定落点 == 第一个置错的闸；无闸置错则落在退出闸。
        if a.stage.order() != first_flag(&s) {
            return false;
        }
        if a.stage == Stage::Exit && a.code != EXIT_OK {
            return false;
        }
        if a.stage.failure() && a.default && !a.attributed() {
            return false;
        }
        // 文案落点归属的闸不得晚于判定落点（`Note::stage` 是唯一反查表）。
        if a.note.stage().order() > a.stage.order() {
            return false;
        }
        if a.stage == Stage::Attr && decides_value(&a) {
            return false;
        }
        // 落位族的结果字节一律是编辑后尺寸；未落位一律是"应得字节"。
        if rotation_landed(&s, &a) && a.value != s.want {
            return false;
        }
        if !rotation_landed(&s, &a) && a.value != halt_value(&s) {
            return false;
        }
        i += 1;
    }
    // 2. 心脏格：无后缀 / 带后缀 在同一写出 ENOSPC 下只差一个数字。
    let bare = match cs.get(5) {
        Some(v) => v,
        None => return false,
    };
    let suff = match cs.get(6) {
        Some(v) => v,
        None => return false,
    };
    let ob = define(&bare.shape);
    let os = define(&suff.shape);
    if ob.stage != Stage::Write || os.stage != Stage::Write {
        return false;
    }
    if ob.code != os.code || ob.note != os.note || ob.default != os.default {
        return false;
    }
    if ob.value != 3072 || os.value != 0 {
        return false;
    }
    if orig_intact(&bare.shape, &ob) != true || orig_intact(&suff.shape, &os) != false {
        return false;
    }
    if backup_bytes(&bare.shape, &ob) != 0 || backup_bytes(&suff.shape, &os) != 3072 {
        return false;
    }
    if !content_balanced(&bare.shape, &ob) || !content_balanced(&suff.shape, &os) {
        return false;
    }
    // 备份闸上还没有临时件：残痕字节必须为 0（清理责任从建件闸起算）。
    if leftover(&bare.shape, &ob) != 0 {
        return false;
    }
    // 3. 九道闸逐道正例。
    // (a) 用法闸：选项本身被拒，字节一个没动。
    let mut s = match shape_at(4) {
        Some(v) => v,
        None => return false,
    };
    s.argv_bad = true;
    s.written = 0;
    let o = define(&s);
    if o.stage != Stage::Argv || o.value != s.size || o.default || !o.note.prelude_family() {
        return false;
    }
    if o.code != expect_code_for(s.tool, o.note) {
        return false;
    }
    // (b) 用法闸的另一支：`--follow-symlinks` 没配 `-i`。
    let mut s = match Shape::blank().valid() {
        true => match shape_at(0) {
            Some(v) => v,
            None => return false,
        },
        false => return false,
    };
    s.rotate = Rotate::None;
    s.follow_symlinks = true;
    s.want_attrs = A_NONE;
    s.attr_failed = A_NONE;
    s.suffix = "";
    s.written = 0;
    if !s.valid() {
        return false;
    }
    let o = define(&s);
    if o.stage != Stage::Argv || o.note != Note::FollowNoI || o.code != EXIT_USAGE {
        return false;
    }
    if o.value != s.size || o.default || o.ok() {
        return false;
    }
    // (c) 打开闸：源件读不到。
    let mut s = match shape_at(5) {
        Some(v) => v,
        None => return false,
    };
    s.open_ok = false;
    s.write_err = false;
    s.written = 0;
    s.errno_seen = ERR_ENOENT;
    if !s.valid() {
        return false;
    }
    let o = define(&s);
    if o.stage != Stage::Open
        || o.note != Note::CantRead
        || o.code != EXIT_IO
        || o.value != 3072
        || o.default
    {
        return false;
    }
    // (d) 打开闸的另一支：源不是普通件，无从轮换。
    let mut s = s;
    s.open_ok = true;
    s.not_regular = true;
    s.errno_seen = ERR_NONE;
    if !s.valid() {
        return false;
    }
    let o = define(&s);
    if o.stage != Stage::Open || o.note != Note::NotRegular || o.value != 3072 {
        return false;
    }
    // (e) 备份闸：改名被权限拒——名字还在原地。
    let mut s = match shape_at(6) {
        Some(v) => v,
        None => return false,
    };
    s.backup_err = true;
    s.write_err = false;
    s.written = 0;
    s.errno_seen = ERR_EACCES;
    if !s.valid() {
        return false;
    }
    let o = define(&s);
    if o.stage != Stage::Backup || o.note != Note::BackupFail || o.value != 3072 || o.default {
        return false;
    }
    if o.default || !orig_intact(&s, &o) {
        return false;
    }
    if backup_bytes(&s, &o) != 0 || leftover(&s, &o) != 0 {
        return false;
    }
    // (f) 备份闸：改名被 ENOSPC 拒——本卡唯一"红在备份闸"的归因格。
    let mut s = s;
    s.errno_seen = ERR_ENOSPC;
    let o = define(&s);
    if o.stage != Stage::Backup || o.note != Note::BackupEnospc || !o.default {
        return false;
    }
    if o.value != 3072 || backup_bytes(&s, &o) != 0 || !content_balanced(&s, &o) {
        return false;
    }
    // (g) 建件闸：目录不可写，红得跟空间无关。
    let mut s = match shape_at(7) {
        Some(v) => v,
        None => return false,
    };
    s.mktemp_ok = false;
    s.errno_seen = ERR_EACCES;
    if !s.valid() {
        return false;
    }
    let o = define(&s);
    if o.stage != Stage::Mktemp || o.note != Note::TempHomeless || o.default || o.value != 1024 {
        return false;
    }
    if temp_present(&s, &o) {
        return false;
    }
    // (h) 建件闸：ENOSPC 连件都开不出来（`legacy.d` 的画像）。
    let mut s = s;
    s.errno_seen = ERR_ENOSPC;
    let o = define(&s);
    if o.stage != Stage::Mktemp || o.note != Note::MktempFail || !o.default || o.code != EXIT_IO {
        return false;
    }
    if leftover(&s, &o) != 0 || !content_balanced(&s, &o) {
        return false;
    }
    // (i) 写出闸：邻居 errno，红但不是本卡的归因。
    let mut s = match shape_at(5) {
        Some(v) => v,
        None => return false,
    };
    s.errno_seen = ERR_EIO;
    if !s.valid() {
        return false;
    }
    let o = define(&s);
    if o.stage != Stage::Write || o.note != Note::ForeignErr || o.default || o.attributed() {
        return false;
    }
    if msg_foreign(&s, &o).is_empty() || o.code != EXIT_IO {
        return false;
    }
    if content_sum(&s, &o) != 3072 {
        return false;
    }
    // (j) 写出闸的异常形态：少了字节却没报错——静默轮换，判死。
    let mut s = s;
    s.errno_seen = ERR_NONE;
    s.write_err = false;
    if !s.valid() {
        return false;
    }
    let o = define(&s);
    if o.stage != Stage::Write || o.note != Note::SilentRotate || o.default || o.ok() {
        return false;
    }
    if o.note.defect_family() || !msg_silent_is_broken(&s, &o) {
        return false;
    }
    // (k) 落定闸：延迟分配在 close 才爆。
    let mut s = match shape_at(5) {
        Some(v) => v,
        None => return false,
    };
    s.write_err = false;
    s.close_err = true;
    s.written = s.want;
    s.errno_seen = ERR_ENOSPC;
    if !s.valid() {
        return false;
    }
    let o = define(&s);
    if o.stage != Stage::Close || o.note != Note::CloseEnospc || !o.default || o.value != 3072 {
        return false;
    }
    if o.landed() || !leftover_is_zero(&s, &o) {
        return false;
    }
    // (l) 落定闸在 busybox 上不拦：错误被吞，表观落位、延迟块无人记账。
    let mut sw = Shape::blank();
    sw.tag = "bb-close-swallow";
    sw.tool = Tool::Busybox;
    sw.size = 3072;
    sw.want = 4096;
    sw.written = 4096;
    sw.close_err = true;
    sw.errno_seen = ERR_ENOSPC;
    if !sw.valid() {
        return false;
    }
    let o = define(&sw);
    if o.stage != Stage::Exit || o.note != Note::CloseSwallowed || o.code != EXIT_OK {
        return false;
    }
    if !o.ok() || !o.note.noisy_success() || deferred_lost(&sw, &o) != 4096 {
        return false;
    }
    if content_balanced(&sw, &o) {
        return false;
    }
    if !content_sum_is(&sw, &o, 4096) {
        return false;
    }
    // (m) 属性闸：GNU 报错但照换——码非 0 而内容已落位。
    let mut s = match shape_at(3) {
        Some(v) => v,
        None => return false,
    };
    s.attr_failed = A_MODE;
    s.errno_seen = ERR_EACCES;
    if !s.valid() {
        return false;
    }
    let o = define(&s);
    if o.stage != Stage::Attr || o.note != Note::AttrMode || o.code != EXIT_IO || o.value != 4096
    {
        return false;
    }
    if o.ok() || !rotation_landed(&s, &o) || o.default || orig_intact(&s, &o) {
        return false;
    }
    if !content_balanced(&s, &o) || backup_bytes(&s, &o) != 3072 {
        return false;
    }
    // (n) 属性闸：busybox 直接放弃，写完的临时件留在原地继续吃空间。
    let mut bb = Shape::blank();
    bb.tag = "bb-attr-mode";
    bb.tool = Tool::Busybox;
    bb.want_attrs = A_MODE;
    bb.attr_failed = A_MODE;
    bb.attr_err = true;
    bb.size = 3072;
    bb.want = 4096;
    bb.written = 4096;
    bb.errno_seen = ERR_EACCES;
    if !bb.valid() {
        return false;
    }
    let o = define(&bb);
    if o.stage != Stage::Attr || o.note != Note::AttrMode || o.code != EXIT_BB || o.value != 3072
    {
        return false;
    }
    if !rotation_landed_is_false(&bb, &o) {
        return false;
    }
    if leftover(&bb, &o) != 4096 || !temp_present(&bb, &o) {
        return false;
    }
    if msg_temp_residue(&bb, &o).is_empty() || !content_balanced(&bb, &o) {
        return false;
    }
    // (o) 落位闸：目录扩不出来，`rename` 真返 ENOSPC。
    let mut rn = Shape::blank();
    rn.tag = "gnu-rename-enospc";
    rn.dir = "legacy.d";
    rn.size = 3072;
    rn.want = 4096;
    rn.written = 4096;
    rn.rename_err = true;
    rn.errno_seen = ERR_ENOSPC;
    if !rn.valid() {
        return false;
    }
    let o = define(&rn);
    if o.stage != Stage::Rename || o.note != Note::RenameEnospc || !o.default || o.value != 3072 {
        return false;
    }
    if !o.attributed() || o.landed() || !orig_intact(&rn, &o) {
        return false;
    }
    if !message(&rn, &o).contains("can't rename") {
        return false;
    }
    // (p) 退出闸：硬链接被斩——落位成功但语义账有话。
    let mut s = match shape_at(2) {
        Some(v) => v,
        None => return false,
    };
    s.written = s.want;
    if !s.valid() {
        return false;
    }
    let o = define(&s);
    if o.stage != Stage::Exit || o.note != Note::LinkCut || o.code != EXIT_OK || o.value != 4096 {
        return false;
    }
    if !o.ok() || Tier::of(&o) != Tier::Noisy || !o.landed() {
        return false;
    }
    if s.hardlinks < 2 {
        return false;
    }
    // (q) 退出闸：跟随符号链接的轮换把链接本身还原了。
    let mut sy = Shape::blank();
    sy.tag = "gnu-symlink-restored";
    sy.is_symlink = true;
    sy.follow_symlinks = true;
    sy.size = 3072;
    sy.want = 4096;
    sy.written = 4096;
    sy.want_attrs = A_NONE;
    if !sy.valid() {
        return false;
    }
    let o = define(&sy);
    if o.stage != Stage::Exit || o.note != Note::SymlinkRestored || !o.ok() {
        return false;
    }
    if Tier::of(&o) != Tier::Noisy || o.value != 4096 {
        return false;
    }
    // (r) 退出闸的正例：全量落位、无声。
    let mut s = match shape_at(0) {
        Some(v) => v,
        None => return false,
    };
    s.written = s.want;
    let o = define(&s);
    if o.stage != Stage::Exit || o.note != Note::Complete || Tier::of(&o) != Tier::Rotated {
        return false;
    }
    if !o.ok() || !o.landed() || message(&s, &o).len() != 0 {
        return false;
    }
    if orig_bytes(&o) != 4096 || !content_balanced(&s, &o) {
        return false;
    }
    // 4. 后闸不许改写前闸的结论：同一形状的更靠后的错误旗标一律不起作用。
    let mut s = match shape_at(5) {
        Some(v) => v,
        None => return false,
    };
    s.write_err = true;
    s.rename_err = false;
    s.close_err = false;
    s.attr_err = false;
    s.attr_failed = A_NONE;
    s.errno_seen = ERR_ENOSPC;
    s.written = 2048;
    let early = define(&s);
    if !early.stage.failure() || early.stage != Stage::Write {
        return false;
    }
    // 把落位闸的错也挂上：写出闸先红，结论仍是写出闸——落位段没被执行过。
    let mut later = s;
    later.rename_err = true;
    if later.valid() {
        // 自洽域禁止两闸同错；禁止本身就是这条性质的守门人。
        return false;
    }
    if define(&s).seal() != early.seal() {
        return false;
    }
    true
}

/// 静默轮换是否落在归因破断的解释通道上。
fn msg_silent_is_broken(s: &Shape, o: &Outcome) -> bool {
    message(s, o).contains("silent rotation on ") && o.note.defect_family()
}

/// GNU 的失败路径应当不留残痕。
fn leftover_is_zero(s: &Shape, o: &Outcome) -> bool {
    leftover(s, o) == 0 && !temp_present(s, o)
}

/// busybox 属性闸放弃轮换时，原件名下必须还是改名前那份内容。
fn rotation_landed_is_false(s: &Shape, o: &Outcome) -> bool {
    !rotation_landed(s, o) && orig_intact(s, o) && o.value == s.size
}

/// 内容守恒和的具体数字（把"和等于几"写死，防止两侧同时漂移）。
fn content_sum_is(s: &Shape, o: &Outcome, want: u32) -> bool {
    content_sum(s, o) == want
}

/// SI06：归因三元组（文案族 × 退出码 × 归因位）。
///
/// 本卡的"是不是 ENOSPC 打断了轮换"不是一个布尔字段说了算：它必须同时兑现三条
/// ——归因位为真、文案属轮换中止族、退出码非 0。三条里任何一条单独成立都不算
/// 命中，因为现场形态有三种骗法：busybox 吞掉落定错误（码 0）、邻居 errno
/// （文案同族但成因不同）、属性段报错（码非 0 却没打断内容）。
fn abort_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    let mut attributed_slots = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        // 三元组：归因位真 ⇔ 文案族 + 码非 0 + 失败 同时成立。
        if a.default != a.attributed() {
            return false;
        }
        if a.attributed() {
            attributed_slots += 1;
            if !a.note.abort_family() || a.code == EXIT_OK || !a.failed() {
                return false;
            }
            if !a.stage.abort_plane() {
                return false;
            }
            if c.shape.errno_seen != ERR_ENOSPC {
                return false;
            }
            if !message(&c.shape, &a).contains(strerror(ERR_ENOSPC)) {
                return false;
            }
        } else if a.note.abort_family() && a.code != EXIT_OK {
            // 文案族齐了却没归因，只允许"邻居 errno"这一条路。
            if c.shape.errno_seen == ERR_ENOSPC {
                return false;
            }
        }
        // 归因位为假时不许谎报命中。
        if !a.default && a.attributed() {
            return false;
        }
        // 内容守恒：14 槽逐格都要平（busybox 吞掉落定错误那一格除外，
        // 因为它的物理块确实丢了，本卡不许用表观和掩盖）。
        let swallow = a.note == Note::CloseSwallowed;
        if content_balanced(&c.shape, &a) == swallow {
            return false;
        }
        if !swallow && deferred_lost(&c.shape, &a) != 0 {
            return false;
        }
        if swallow && c.shape.tool.reports_close_error() {
            return false;
        }
        // 残痕只可能出现在建件闸之后，且只在不清理的工具上。
        let l = leftover(&c.shape, &a);
        if l > 0 && !a.stage.residue_plane() {
            return false;
        }
        if l > 0 && c.shape.tool.cleans_temp_on_fail() {
            return false;
        }
        if a.ok() && l != 0 {
            return false;
        }
        i += 1;
    }
    if attributed_slots != 3 + REJECT {
        return false;
    }
    // 三字节数账：value 是"原件名下"，backup 是"备份件里"，两者不许互顶。
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let moved = c.shape.original_moved();
        if a.value != orig_bytes(&a) {
            return false;
        }
        if moved && a.value != 0 && rotation_landed(&c.shape, &a) == false && a.default == false {
            return false;
        }
        if !moved && a.failed() && a.value != c.shape.size && !a.note.tail_family() {
            return false;
        }
        if backup_bytes(&c.shape, &a) > c.shape.size {
            return false;
        }
        if c.shape.rotate.moves_original()
            && !c.shape.backup_err
            && backup_bytes(&c.shape, &a) != c.shape.size
            && a.stage.order() >= Stage::Mktemp.order()
        {
            return false;
        }
        i += 1;
    }
    // 破断解释行：归因位为假却缺字节时，必须能在文案里读出三个数。
    let mut s = match shape_at(5) {
        Some(v) => v,
        None => return false,
    };
    s.write_err = false;
    s.written = 2048;
    s.errno_seen = ERR_NONE;
    let o = define(&s);
    if !msg_silent_is_broken(&s, &o) {
        return false;
    }
    let line = msg_broken(&s, &o);
    if !line.contains("note=silent-rotate") || !line.contains("orig=3072") {
        return false;
    }
    if !line.contains("residue=0") || !line.contains("exit=4") {
        return false;
    }
    // 邻居 errno 的解释行只在非归因的失败上出现。
    let mut f = s;
    f.write_err = true;
    f.errno_seen = ERR_EDQUOT;
    let o = define(&f);
    if o.default || msg_foreign(&f, &o).is_empty() {
        return false;
    }
    if !msg_foreign(&s, &define(&s)).is_empty() {
        return false;
    }
    // ENOSPC 现场本身不出邻居解释行（否则同一现场会被两张纸各判一次）。
    let enospc = match shape_at(5) {
        Some(v) => v,
        None => return false,
    };
    let eo = define(&enospc);
    if !msg_foreign(&enospc, &eo).is_empty() {
        return false;
    }
    if !eo.attributed() || eo.note.abort_family() == false {
        return false;
    }
    true
}

/// SI07：六维对判的固定次序（状态 → 阶段 → 码 → 值 → 文案 → 归因）。
///
/// 次序是本卡唯一能兑现"先判成败互报"的地方：一槽把写出 ENOSPC 报成全量落位，
/// 六个维度**同时**全错，若先比阶段就会把"谎称成功"降级成"报错了闸"——审计里
/// 这是性质完全不同的一件事。这一格把优先级一维一维压出来。
fn judge_ok() -> bool {
    let cs = cases();
    // 1. 十四槽的实际缺陷必须与期望表逐格相等。
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want = match expected(i) {
            Some(v) => v,
            None => return false,
        };
        if judge(&c) != want || c.judge() != want || c.ok() != (want == Fault::Ok) {
            return false;
        }
        if c.tag() != c.shape.tag || c.actual().seal() != define(&c.shape).seal() {
            return false;
        }
        i += 1;
    }
    if expected(CORPUS).is_some() {
        return false;
    }
    let mut i = 0usize;
    while i < ACCEPT {
        if expected(i) != Some(Fault::Ok) {
            return false;
        }
        i += 1;
    }
    // 2. 单维翻案：一次只改申报的一维，落出的缺陷必须是那一维。
    let base = match cs.get(5) {
        Some(v) => *v,
        None => return false,
    };
    if base.tag() != BASE_TAG {
        return false;
    }
    let dims = [
        Fault::Estate,
        Fault::Stage,
        Fault::Code,
        Fault::Value,
        Fault::Note,
        Fault::Default,
    ];
    let mut k = 0usize;
    while k < dims.len() {
        let d = match dims.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let mut c = base;
        match d {
            Fault::Estate => {
                c.claim.stage = Stage::Exit;
                c.claim.code = EXIT_OK;
                c.claim.value = base.shape.want;
                c.claim.note = Note::Complete;
                c.claim.default = false;
            }
            Fault::Stage => c.claim.stage = Stage::Close,
            Fault::Code => c.claim.code = EXIT_USAGE,
            Fault::Value => c.claim.value = 0,
            Fault::Note => c.claim.note = Note::CloseEnospc,
            Fault::Default => c.claim.default = false,
            _ => return false,
        }
        if judge(&c) != d {
            return false;
        }
        k += 1;
    }
    // 3. 优先级：多维同错时按 Estate → Stage → Code → Value → Note → Default 落。
    let mut c = base;
    c.claim.stage = Stage::Rename;
    c.claim.code = EXIT_OK;
    c.claim.note = Note::Complete;
    c.claim.value = 999;
    c.claim.default = false;
    if judge(&c) != Fault::Estate {
        return false;
    }
    let mut c = base;
    c.claim.stage = Stage::Rename;
    c.claim.code = EXIT_USAGE;
    c.claim.value = 999;
    if judge(&c) != Fault::Stage {
        return false;
    }
    let mut c = base;
    c.claim.code = EXIT_USAGE;
    c.claim.value = 999;
    c.claim.note = Note::AttrMode;
    if judge(&c) != Fault::Code {
        return false;
    }
    let mut c = base;
    c.claim.value = 0;
    c.claim.note = Note::RenameEnospc;
    if judge(&c) != Fault::Value {
        return false;
    }
    let mut c = base;
    c.claim.note = Note::CloseEnospc;
    if judge(&c) != Fault::Note {
        return false;
    }
    // 归因位错时码不许跟着错，否则测不到最后一维。
    let mut c = base;
    c.claim.default = false;
    if judge(&c) != Fault::Default || base.claim.default != true {
        return false;
    }
    // 4. 申报与实际的成败判据必须同构（都是"码 0 且文案属成功族"）。
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.claim.success() != (c.claim.code == EXIT_OK && c.claim.note.success()) {
            return false;
        }
        if c.actual().ok() != (c.actual().code == EXIT_OK && c.actual().note.success()) {
            return false;
        }
        if c.shape.tag.is_empty() {
            return false;
        }
        i += 1;
    }
    // 5. 照抄现场必然一致；申报为空必然读出缺陷。
    let mut all = base;
    all.claim = match c_actual_claim(&base) {
        Some(v) => v,
        None => return false,
    };
    if judge(&all) != Fault::Ok {
        return false;
    }
    let mut empty = base;
    empty.claim = Claim::blank();
    if !empty.claim.success() || judge(&empty) == Fault::Ok {
        return false;
    }
    true
}

/// 用实际结果反写一份申报（"照抄现场必然一致"的复算）。
fn c_actual_claim(c: &Case) -> Option<Claim> {
    let a = c.actual();
    Some(Claim {
        stage: a.stage,
        code: a.code,
        value: a.value,
        note: a.note,
        default: a.default,
    })
}

/// SI08：四档矩阵（覆盖面与正确面各自独立计数）。
///
/// 档按**实际结果**投影，申报对错单独计——这正是本卡的立卡理由：六个红档全部
/// 落在 `aborted` 一档里，如果 `total` 跟着 `good` 走，就会显示成"中止档没有
/// 槽"，而这恰好掩盖了"红的全是复刻槽"这件事。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    if m.slots() != CORPUS || m.goods() != ACCEPT {
        return false;
    }
    if !m.covered() {
        return false;
    }
    if m.rows.len() != TIER_COUNT {
        return false;
    }
    let want_total = [2usize, 1usize, 2usize, 9usize];
    let want_good = [2usize, 1usize, 2usize, 3usize];
    let mut i = 0usize;
    while i < TIER_COUNT {
        let t = match TIERS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let r = match m.row(t) {
            Some(v) => v,
            None => return false,
        };
        let wt = match want_total.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let wg = match want_good.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if r.total != wt || r.good != wg || r.red() != wt - wg {
            return false;
        }
        if r.perfect() != (r.red() == 0) {
            return false;
        }
        if !r.reds_are_replicas() {
            return false;
        }
        if r.total == 0 || r.good > r.total {
            return false;
        }
        if !r.render(t).contains(t.name()) || !r.render(t).contains("red=") {
            return false;
        }
        i += 1;
    }
    if m.perfect_rows() != 3 {
        return false;
    }
    if Matrix::blank().covered() || Matrix::blank().slots() != 0 {
        return false;
    }
    if m.row(Tier::Rotated).is_none() || m.row(Tier::Aborted).is_none() {
        return false;
    }
    // 中止档的红档数必须恰等于复刻槽数（红的全是复刻槽，不是覆盖面）。
    let ab = match m.row(Tier::Aborted) {
        Some(v) => v,
        None => return false,
    };
    if ab.red() != REJECT || ab.total != ACCEPT_TIER_ABORTED {
        return false;
    }
    // 渲染：四行、每行一个档名、行数与档数一致。
    let text = m.render();
    let mut lines = 0usize;
    for ch in text.chars() {
        if ch == '\n' {
            lines += 1;
        }
    }
    if lines != TIER_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < TIER_COUNT {
        if let Some(t) = TIERS.get(i) {
            if !text.contains(t.name()) {
                return false;
            }
        }
        i += 1;
    }
    // 报表：与矩阵逐档一致，且全绿渲染被逐字钉住。
    let rep = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if rep.total as usize != CORPUS
        || rep.accept as usize != ACCEPT
        || rep.reject as usize != REJECT
    {
        return false;
    }
    let mut i = 0usize;
    let counts = [rep.rotated, rep.noisy, rep.halted, rep.aborted];
    while i < TIER_COUNT {
        let got = match counts.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let wt = match want_total.get(i) {
            Some(v) => *v as u32,
            None => return false,
        };
        let wg = match want_good.get(i) {
            Some(v) => *v as u32,
            None => return false,
        };
        // 报表按"申报正确"计数，矩阵按"该档有槽"计数，两者在这一格对上。
        if got != wg || got > wt {
            return false;
        }
        i += 1;
    }
    if rep.zero_cells() != rep.rotated + rep.noisy || rep.nonzero_cells() != rep.halted + rep.aborted
    {
        return false;
    }
    if rep.zero_cells() + rep.nonzero_cells() != rep.accept {
        return false;
    }
    if !rep.green() || rep.gate().is_err() || rep.rate() != 1000 {
        return false;
    }
    if rep.render().as_str() != RENDER_OK {
        return false;
    }
    if !RENDER_OK.contains("sed-i-rotate corpus=14") {
        return false;
    }
    // 注入一槽缺陷后必须离开全绿（matched 是算出来的，不是硬编码）。
    let red = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let red_rate = red.rate();
    let want_rate = match VALS.get(7) {
        Some(v) => *v,
        None => return false,
    };
    if red.green() || red.gate().is_ok() || red.rate() >= 1000 {
        return false;
    }
    if red.matched != 13 || red_rate != want_rate {
        return false;
    }
    if red.accept != rep.accept - 1 || red.reject != rep.reject + 1 {
        return false;
    }
    if red.rotated != rep.rotated - 1 {
        return false;
    }
    // 越界槽位必须回 `SI_ESLOT`，不许悄悄当成"没注入"。
    match run_with(Some((CORPUS, Fault::Ok))) {
        Ok(_) => return false,
        Err(c) => {
            if c != SI_ESLOT {
                return false;
            }
        }
    }
    match run_with(Some((CORPUS + 4096, Fault::Stage))) {
        Ok(_) => return false,
        Err(c) => {
            if c != SI_ESLOT {
                return false;
            }
        }
    }
    if rep.gate() != Ok(()) {
        return false;
    }
    true
}

/// 中止档的槽数（3 个活槽 + 6 个复刻槽）。
const ACCEPT_TIER_ABORTED: usize = 9;

/// SI09：语料与三份陈述。
/// 同一份现场在本卡里写了三遍：`SHAPES`（形状）、`CLAIMS`（申报）、`WANT_*`
/// （期望表）。三份必须两两对得上——这是"申报表被复写一遍"的交叉校验，任一处
/// 笔误（把落位闸写成 `Rename`、把备份字节写成 `4096`）都会在第二或第三份里露出来。
fn corpus_ok() -> bool {
    let cs = cases();
    if cs.len() != CORPUS || SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS {
        return false;
    }
    // 三份的第一对：形状 ↔ 标签表。
    let mut own_tags: [&str; CORPUS] = [""; CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        let s = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let w = match WANT_TAGS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.tag != w || let_some_tag(s, w) {
            return false;
        }
        if let Some(slot) = own_tags.get_mut(i) {
            *slot = s.tag;
        }
        i += 1;
    }
    if !tags_equal(own_tags.as_slice(), WANT_TAGS) {
        return false;
    }
    let mut a = 0usize;
    while a < CORPUS {
        let mut b = a + 1;
        while b < CORPUS {
            if let (Some(x), Some(y)) = (own_tags.get(a), own_tags.get(b)) {
                if x == y {
                    return false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    // 三份的第二对：纵线实判 ↔ 期望表（六个维度逐格）。
    let mut i = 0usize;
    while i < CORPUS {
        let s = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let o = define(&s);
        if Some(o.stage) != WANT_STAGE.get(i).copied() {
            return false;
        }
        if Some(o.code) != WANT_CODE.get(i).copied() {
            return false;
        }
        if Some(o.value) != WANT_VALUE.get(i).copied() {
            return false;
        }
        if Some(o.note) != WANT_NOTE.get(i).copied() {
            return false;
        }
        if Some(o.default) != WANT_DEFAULT.get(i).copied() {
            return false;
        }
        if Some(Tier::of(&o).index() as u8) != WANT_NDX.get(i).copied() {
            return false;
        }
        if Some(backup_bytes(&s, &o)) != WANT_BACKUP.get(i).copied() {
            return false;
        }
        if Some(leftover(&s, &o)) != WANT_RESIDUE.get(i).copied() {
            return false;
        }
        if o.code > EXIT_INTERNAL + 1 {
            return false;
        }
        if !errno_legal(s.errno_seen) {
            return false;
        }
        // 归因位与期望表同序：真 ⇒ 中止档。
        if o.default != (Some(Tier::Aborted.index() as u8) == WANT_NDX.get(i).copied()) {
            return false;
        }
        i += 1;
    }
    // 三份的第三对：活槽申报 ↔ 实判（活槽必须逐维照抄现场）。
    let mut i = 0usize;
    while i < ACCEPT {
        let s = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let k = match CLAIMS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let o = define(&s);
        if k.stage != o.stage || k.code != o.code || k.value != o.value {
            return false;
        }
        if k.note != o.note || k.default != o.default {
            return false;
        }
        i += 1;
    }
    // 期望退出码与实判一致（红档槽也一样：错的是申报，不是现场）。
    let mut i = 0usize;
    while i < CORPUS {
        let s = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let o = define(&s);
        if let Some(w) = WANT_CODE.get(i) {
            if o.code != *w {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 标签为空的可读别名（上一段已判，这里只把"空标签"这件事写成一格）。
fn let_some_tag(s: &Shape, w: &str) -> bool {
    s.tag.is_empty() || w.is_empty()
}

/// SI10：八个活槽的覆盖面。
///
/// 活槽不是"随便凑八格"：四档都要有、两套工具都要有、`-i` 的两种形态都要有、
/// 尺寸账要么变要么明确不变。缺一条就说明覆盖面塌了一角，而这正是"绿得没有意义"
/// 的开始。
fn slots_ok() -> bool {
    if ACCEPT_SLOTS.len() != ACCEPT || REJECT_SLOTS.len() != REJECT {
        return false;
    }
    // 两套下标不重不漏，合起来正好是 0..14。
    let mut seen = [false; CORPUS];
    let mut i = 0usize;
    while i < ACCEPT {
        if let Some(v) = ACCEPT_SLOTS.get(i) {
            if *v >= CORPUS {
                return false;
            }
            if let Some(flag) = seen.get_mut(*v) {
                if *flag {
                    return false;
                }
                *flag = true;
            }
        }
        i += 1;
    }
    i = 0;
    while i < REJECT {
        if let Some(v) = REJECT_SLOTS.get(i) {
            if *v >= CORPUS {
                return false;
            }
            if let Some(flag) = seen.get_mut(*v) {
                if *flag {
                    return false;
                }
                *flag = true;
            }
        }
        i += 1;
    }
    let mut n_seen = 0usize;
    for x in seen.iter() {
        if *x {
            n_seen += 1;
        }
    }
    if n_seen != CORPUS {
        return false;
    }
    if ACCEPT_SLOTS.len() + REJECT_SLOTS.len() != CORPUS {
        return false;
    }
    let mut i = 0usize;
    while i < ACCEPT {
        if Some(i) != ACCEPT_SLOTS.get(i).copied() {
            return false;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < REJECT {
        let want = match ACCEPT.checked_add(i) {
            Some(v) => v,
            None => return false,
        };
        if Some(want) != REJECT_SLOTS.get(i).copied() {
            return false;
        }
        i += 1;
    }
    // 活槽逐格：申报无缺陷、形状合法、现场与档位对得上。
    let cs = cases();
    let mut tier_hits = [0usize; TIER_COUNT];
    let mut tool_gnu = false;
    let mut tool_bb = false;
    let mut mode_bare = false;
    let mut mode_suffix = false;
    let mut stage_hits = [false; 9];
    let mut dir_cfg = false;
    let mut dir_legacy = false;
    let mut plane_orig = false;
    let mut byte_account = 0usize;
    let mut i = 0usize;
    while i < ACCEPT {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if judge(&c) != Fault::Ok || expected(i) != Some(Fault::Ok) {
            return false;
        }
        if !c.shape.valid() || c.shape.tag.is_empty() {
            return false;
        }
        let t = Tier::of(&c.actual()).index();
        if let Some(n) = tier_hits.get_mut(t) {
            *n += 1;
        }
        match c.shape.tool {
            Tool::Gnu => tool_gnu = true,
            Tool::Busybox => tool_bb = true,
        }
        match c.shape.rotate {
            Rotate::Bare => mode_bare = true,
            Rotate::Suffix => mode_suffix = true,
            Rotate::None => return false,
        }
        let ord = c.actual().stage.order();
        if let Some(flag) = stage_hits.get_mut(ord as usize) {
            *flag = true;
        }
        if c.shape.dir == "cfg.d" {
            dir_cfg = true;
        }
        if c.shape.dir == "legacy.d" {
            dir_legacy = true;
        }
        if c.actual().stage.orig_plane() {
            plane_orig = true;
        }
        // 尺寸账：编辑改变尺寸，落位格报 want、中止格报 size/0 才互相区分不开。
        if c.shape.want > c.shape.size {
            byte_account += 1;
        }
        i += 1;
    }
    // 四档齐、两工具齐、两形态齐、两块目录都用到、原件语义闸有正例。
    for n in tier_hits.iter() {
        if *n == 0 {
            return false;
        }
    }
    if !tool_gnu || !tool_bb || !mode_bare || !mode_suffix {
        return false;
    }
    if !dir_cfg || !dir_legacy || !plane_orig {
        return false;
    }
    if byte_account < 6 {
        return false;
    }
    // 活槽至少走过五道不同的闸（ argv/attr/write/mktemp/exit ）。
    let mut stages_hit = 0usize;
    for flag in stage_hits.iter() {
        if *flag {
            stages_hit += 1;
        }
    }
    if stages_hit < 5 {
        return false;
    }
    // 落位/有话/停/中止四档各自的代表槽（把档位再复述一遍）。
    let rot = match tier_hits.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let noi = match tier_hits.get(1) {
        Some(v) => *v,
        None => return false,
    };
    let hal = match tier_hits.get(2) {
        Some(v) => *v,
        None => return false,
    };
    let abo = match tier_hits.get(3) {
        Some(v) => *v,
        None => return false,
    };
    if rot < 2 || noi < 1 || hal < 2 || abo < 3 {
        return false;
    }
    if rot + noi + hal + abo != ACCEPT {
        return false;
    }
    // 活槽里必须有一格"码非 0 但内容已落位"与一格"码 0 但语义账有话"。
    let mut landed_but_red = false;
    let mut zero_but_noisy = false;
    let mut i = 0usize;
    while i < ACCEPT {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let o = c.actual();
        if rotation_landed(&c.shape, &o) && o.code != EXIT_OK {
            landed_but_red = true;
        }
        if o.ok() && o.note.noisy_success() {
            zero_but_noisy = true;
        }
        i += 1;
    }
    if !landed_but_red || !zero_but_noisy {
        return false;
    }
    // 红档槽：形状一律复刻基准格，档位一律留在中止档。
    let base_seal = match shape_at(5) {
        Some(v) => v.seal(),
        None => return false,
    };
    let mut i = 0usize;
    while i < REJECT {
        let slot = match REJECT_SLOTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let s = match shape_at(slot) {
            Some(v) => v,
            None => return false,
        };
        if s.seal() != base_seal || !s.valid() {
            return false;
        }
        let o = define(&s);
        if Tier::of(&o) != Tier::Aborted || !o.attributed() {
            return false;
        }
        i += 1;
    }
    true
}

/// SI11：六个红档槽各自只坏一维。
///
/// 六槽的形状与基准格**逐字节同形**（指纹相等），差别全在申报表上，而且一槽只
/// 改一维；改的那一维必须正好是契约规定的次序 `Estate → Stage → Code → Value
/// → Note → Default`。这一格防的是最常见的两种作弊：红档槽靠改形状变红（那样
/// 判的就不是申报而是现场），以及一槽同时改两维（那样次序就测不出来了）。
fn reds_ok() -> bool {
    let cs = cases();
    let base = match cs.get(5) {
        Some(v) => *v,
        None => return false,
    };
    if base.tag() != BASE_TAG {
        return false;
    }
    let dims = ["estate", "stage", "code", "value", "note", "default"];
    let mut i = 0usize;
    while i < REJECT {
        let slot = match REJECT_SLOTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let c = match cs.get(slot) {
            Some(v) => *v,
            None => return false,
        };
        let d = match REJECT_FAULTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        // 标签：`<维度>-swap`，且互不相同。
        let name = match dims.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut want_tag = String::from(name);
        want_tag.push_str(SWAP_SUFFIX);
        if c.tag() != want_tag.as_str() {
            return false;
        }
        // 形状同形（现场一模一样）。
        if c.shape.seal() != base.shape.seal() || c.shape.tag == base.shape.tag {
            return false;
        }
        // 申报只差一维，且差的正是本槽挂的那一维。
        let mut diff = 0usize;
        if c.claim.stage != base.claim.stage {
            diff += 1;
        }
        if c.claim.code != base.claim.code {
            diff += 1;
        }
        if c.claim.value != base.claim.value {
            diff += 1;
        }
        if c.claim.note != base.claim.note {
            diff += 1;
        }
        if c.claim.default != base.claim.default {
            diff += 1;
        }
        if c.claim.success() != base.claim.success() {
            diff += 1;
        }
        if diff != 1 {
            return false;
        }
        if judge(&c) != d || expected(slot) != Some(d) {
            return false;
        }
        if d == Fault::Estate && c.claim.success() == base.claim.success() {
            return false;
        }
        if d != Fault::Estate && c.claim.success() != base.claim.success() {
            return false;
        }
        i += 1;
    }
    // 六个缺陷码逐格对上，且互不相同（一维一码，红档读得出是谁报的）。
    let mut i = 0usize;
    while i < REJECT {
        let slot = match REJECT_SLOTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let c = match cs.get(slot) {
            Some(v) => *v,
            None => return false,
        };
        let code = match REJECT_CODES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let f = match expected(slot) {
            Some(v) => v,
            None => return false,
        };
        if f.code() != code || judge(&c).code() != code {
            return false;
        }
        let mut j = i + 1;
        while j < REJECT {
            if let Some(other) = REJECT_CODES.get(j) {
                if *other == code {
                    return false;
                }
            }
            j += 1;
        }
        // 红档槽不改变覆盖面：仍留在中止档，且现场仍可归因。
        if Tier::of(&c.actual()) != Tier::Aborted {
            return false;
        }
        i += 1;
    }
    // 报表读数：8 成 6 败、全绿 1000。
    let rep = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if rep.accept != ACCEPT as u32 || rep.reject != REJECT as u32 {
        return false;
    }
    if rep.matched != rep.total || rep.rate() != 1000 {
        return false;
    }
    true
}

/// SI12：报错文案逐字。
///
/// 本卡判的是"运维读那行 stderr 能不能反推出停在哪道闸"，所以文案不是装饰：
/// GNU 的 `couldn't write 1 output file to ...`（单数）、busybox 的
/// `unrecognized option: 'Z'`（冒号 + 单引号）、`couldn't open temporary file
/// cfg.d/sedXXXXXX`（模板名留在原地）都必须逐字符对上。成功族一律空串——落位
/// 时把诊断通道占着，审计脚本就会把"无话"读成"错"。
fn message_ok() -> bool {
    let cs = cases();
    let want = [
        "",
        "",
        "",
        "sed: can't chown cfg.conf: Operation not permitted",
        "sed: unrecognized option: 'Z'",
        "sed: couldn't write 1 output file to No space left on device",
        "sed: couldn't write 1 output file to No space left on device",
        "sed: couldn't open temporary file legacy.d/sedXXXXXX: No space left on device",
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
        // 成功族的诊断通道必须空着。
        if a.note.success() && !w.is_empty() {
            return false;
        }
        if !a.note.success() && w.is_empty() {
            return false;
        }
        i += 1;
    }
    // 中止族文案一律落在 ENOSPC 的 glibc 原文上，且带前缀。
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let line = message(&c.shape, &a);
        if a.note.abort_family() {
            if !line.ends_with(strerror(ERR_ENOSPC)) {
                return false;
            }
            if !line.starts_with("sed: ") {
                return false;
            }
        } else if a.note.success() && line.len() != 0 {
            return false;
        }
        if !line.is_empty() && !line.contains("sed") {
            return false;
        }
        i += 1;
    }
    // 单元件：GNU 的 `%d output file%s` 复数展开。
    let one = msg_gnu_write_fail(1, ERR_ENOSPC);
    let two = msg_gnu_write_fail(2, ERR_ENOSPC);
    if one.as_str() != "sed: couldn't write 1 output file to No space left on device" {
        return false;
    }
    if two.as_str() != "sed: couldn't write 2 output files to No space left on device" {
        return false;
    }
    // 单元件：临时件模板名留在目录里，这是残痕解释的实体。
    if !msg_gnu_mktemp("cfg.d", ERR_ENOSPC).contains("cfg.d/sedXXXXXX") {
        return false;
    }
    if !msg_gnu_close_fail("cfg.d", ERR_ENOSPC).contains("couldn't close temporary file") {
        return false;
    }
    if !msg_gnu_rename_fail("cfg.d/sedXXXXXX", "cfg.conf", ERR_ENOSPC)
        .contains("can't rename cfg.d/sedXXXXXX to cfg.conf")
    {
        return false;
    }
    // 单元件：备份改名的两个路径都要出现在同一行里。
    if !msg_gnu_backup_fail("cfg.conf", "cfg.conf.bak", ERR_EACCES)
        .contains("sed: can't rename cfg.conf to cfg.conf.bak: Permission denied")
    {
        return false;
    }
    // 单元件：两套工具的用法族标点不同（GNU 空格 + 单引号，busybox 冒号）。
    if msg_gnu_unrecognized("--follow-symlink").as_str() != "sed: unrecognized option '--follow-symlink'"
    {
        return false;
    }
    if msg_bb_unrecognized("Z").as_str() != "sed: unrecognized option: 'Z'" {
        return false;
    }
    if msg_gnu_invalid_short("Z").as_str() != "sed: invalid option -- 'Z'" {
        return false;
    }
    if msg_gnu_follow_without_i().as_str() != "sed: can't follow symlink without -I/--in-place" {
        return false;
    }
    if msg_gnu_not_regular("/dev/stdin").as_str() != "sed: couldn't edit /dev/stdin: not a regular file"
    {
        return false;
    }
    // 单元件：busybox 的开件/建件/写错三行措辞各自独立。
    if msg_bb_cannot_open("cfg.conf", ERR_ENOENT).as_str()
        != "sed: can't open file cfg.conf: No such file or directory"
    {
        return false;
    }
    if msg_bb_temp_fail("cfg.conf", ERR_ENOSPC).as_str()
        != "sed: can't create temp file for cfg.conf: No space left on device"
    {
        return false;
    }
    if msg_bb_write_error("cfg.d/sedXXXXXX", ERR_ENOSPC).as_str()
        != "sed: write error to cfg.d/sedXXXXXX: No space left on device"
    {
        return false;
    }
    if msg_bb_chmod("cfg.conf", ERR_EACCES).as_str() != "sed: can't chmod cfg.conf: Permission denied"
    {
        return false;
    }
    if msg_gnu_chmod("cfg.conf", ERR_EPERM).as_str() != "sed: can't chmod cfg.conf: Operation not permitted"
    {
        return false;
    }
    // 静默轮换行必须把两个字节数都写出来。
    let silent = msg_silent_rotate("cfg.conf", 2048, 4096);
    if !silent.contains("wrote 2048") || !silent.contains("4096") {
        return false;
    }
    // 残痕行必须带目录与字节数，且只在有残痕时出现。
    let residue = msg_residue_left("cfg.d", 4096);
    if !residue.contains("cfg.d") || !residue.contains("4096") {
        return false;
    }
    // strerror 逐字：六个 errno 各不相同，未知码回空串。
    let codes = [
        ERR_ENOSPC,
        ERR_ENOENT,
        ERR_EACCES,
        ERR_EPERM,
        ERR_EDQUOT,
        ERR_EIO,
    ];
    if codes.len() != ERRNO_SET_SIZE - 1 {
        return false;
    }
    let mut i = 0usize;
    while i < codes.len() {
        let c = match codes.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if strerror(c).is_empty() || !errno_legal(c) {
            return false;
        }
        let mut j = i + 1;
        while j < codes.len() {
            if let Some(d) = codes.get(j) {
                if strerror(c) == strerror(*d) {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    if strerror(ERR_NONE) != "" || strerror(21) != "" || errno_legal(21) {
        return false;
    }
    if strerror(ERR_ENOSPC) != "No space left on device" {
        return false;
    }
    true
}

/// SI13 的前半：逐槽释义行的六个键与三个字节数。
///
/// 释义行是审计脚本真正读的字符串，所以本卡把"哪几个数必须同时出现"钉死：
/// `orig/backup/residue/lost` 四个字节数 + `stage/exit/note/abort` 四个落点。
/// 少任何一个，运维就只能靠猜——而猜错的代价是把"名字没了内容还在"读成
/// "内容没了"。
fn detail_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let line = detail(&c);
        if !line.ends_with('\n') || line.len() < 40 {
            return false;
        }
        if !line.starts_with(c.shape.file) {
            return false;
        }
        let keys = [
            " tool=",
            " mode=",
            " stage=",
            " exit=",
            " orig=",
            " backup=",
            " residue=",
            " lost=",
            " abort=",
            " note=",
        ];
        for k in keys.iter() {
            if !line.contains(*k) {
                return false;
            }
        }
        // 数字必须与实判一致（逐字符钉，不许写"约等于"）。
        let pairs = [
            (" exit=", dec_text(a.code)),
            (" orig=", dec_text(a.value)),
            (
                " backup=",
                dec_text(backup_bytes(&c.shape, &a)),
            ),
            (" residue=", dec_text(leftover(&c.shape, &a))),
            (" lost=", dec_text(deferred_lost(&c.shape, &a))),
        ];
        let mut k = 0usize;
        while k < pairs.len() {
            let (head, num) = match pairs.get(k) {
                Some(v) => (v.0, &v.1),
                None => return false,
            };
            let mut probe = String::from(head);
            probe.push_str(num.as_str());
            if !line.contains(probe.as_str()) {
                return false;
            }
            k += 1;
        }
        let mut stage_key = String::from(" stage=");
        stage_key.push_str(a.stage.name());
        if !line.contains(stage_key.as_str()) {
            return false;
        }
        let mut note_key = String::from(" note=");
        note_key.push_str(a.note.name());
        if !line.contains(note_key.as_str()) {
            return false;
        }
        let mut abort_key = String::from(" abort=");
        abort_key.push_str(bool_text(a.default));
        if !line.contains(abort_key.as_str()) {
            return false;
        }
        // 诊断行只在有话可说时才进释义；有诊断时两侧必须同文。
        let msg = message(&c.shape, &a);
        if msg.is_empty() {
            if line.contains(" msg=\"") {
                return false;
            }
        } else {
            let mut quoted = String::from(" msg=\"");
            quoted.push_str(msg.as_str());
            quoted.push('"');
            if !line.contains(quoted.as_str()) {
                return false;
            }
        }
        if !line.contains(c.shape.tool.text()) || !line.contains(c.shape.rotate.name()) {
            return false;
        }
        i += 1;
    }
    // 八个活槽的释义行两两不同（同形槽除外，它们的差别只在申报）。
    let mut lines: [String; ACCEPT] = [
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
    ];
    let mut i = 0usize;
    while i < ACCEPT {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if let Some(slot) = lines.get_mut(i) {
            *slot = detail(&c);
        }
        i += 1;
    }
    let mut a = 0usize;
    while a < ACCEPT {
        let mut b = a + 1;
        while b < ACCEPT {
            if let (Some(x), Some(y)) = (lines.get(a), lines.get(b)) {
                if x == y {
                    return false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    // 六个复刻槽的释义行与基准格相同：错的从来不是现场，而是申报。
    let base_line = match cs.get(5) {
        Some(v) => detail(v),
        None => return false,
    };
    let mut i = 0usize;
    while i < REJECT {
        let slot = match REJECT_SLOTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let c = match cs.get(slot) {
            Some(v) => *v,
            None => return false,
        };
        if detail(&c).as_str() != base_line.as_str() {
            return false;
        }
        if c.claim.success() && judge(&c) != Fault::Estate {
            return false;
        }
        i += 1;
    }
    true
}

/// SI13 的中段：阶段闸门的单调性。
///
/// 观测必须是"走过才报"。回退意味着申报者拿着文案倒推闸序——这类谎在真实故障
/// 里最难查，因为文案与字节数都对得上，只有闸序暴露它没走过备份闸。闸门本身
/// 还要守住第二件事：关账之后任何登记都是伪造，一律 `SI_ESTATE`。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    if g.steps() != 0 || g.last() != -1 || g.closed() {
        return false;
    }
    // 全程走一遍九道闸：段数与秩都要对上。
    let mut i = 0usize;
    while i < STAGES.len() {
        let s = match STAGES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if g.step(s).is_err() {
            return false;
        }
        if g.steps() as usize != i + 1 {
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
    let total = match g.close() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if total as usize != STAGES.len() || !g.closed() {
        return false;
    }
    // 关账之后：一切登记与再关账都拒。
    if g.step(Stage::Exit).is_ok() {
        return false;
    }
    match g.close() {
        Ok(_) => return false,
        Err(c) => {
            if c != SI_ESTATE {
                return false;
            }
        }
    }
    if g.steps() as usize != STAGES.len() {
        return false;
    }
    // 回退：已经判死在落位闸，又声称停在写出闸。
    let mut h = Gate::new();
    for s in [Stage::Argv, Stage::Open, Stage::Backup, Stage::Write].iter() {
        if h.step(*s).is_err() {
            return false;
        }
    }
    match h.step(Stage::Backup) {
        Ok(_) => return false,
        Err(c) => {
            if c != SI_ESTAGE {
                return false;
            }
        }
    }
    match h.step(Stage::Argv) {
        Ok(_) => return false,
        Err(c) => {
            if c != SI_ESTAGE {
                return false;
            }
        }
    }
    // 回退被拒之后闸门不损坏：继续向前仍可走。
    if h.step(Stage::Close).is_err() {
        return false;
    }
    if h.steps() as usize != 5 {
        return false;
    }
    // 同段重复不算回退（同一道闸观测两次是允许的）。
    let mut k = Gate::new();
    if k.step(Stage::Write).is_err() || k.step(Stage::Write).is_err() {
        return false;
    }
    if k.steps() != 2 || k.last() != Stage::Write.order() as i32 {
        return false;
    }
    // 空闸门的段数与"未走"哨兵。
    let mut z = Gate::new();
    if z.close().is_err() || z.steps() != 0 || z.last() != -1 {
        return false;
    }
    // 跳段是允许的（现场可能根本没经过属性闸），只禁倒退。
    let mut j = Gate::new();
    if j.step(Stage::Argv).is_err() || j.step(Stage::Exit).is_err() {
        return false;
    }
    if j.close().is_err() {
        return false;
    }
    // 每条语料的实判都构成一条合法闸门行程。
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let mut walk = Gate::new();
        let mut ord = 0u8;
        while ord <= a.stage.order() {
            let s = match Stage::at(ord) {
                Some(v) => v,
                None => return false,
            };
            if walk.step(s).is_err() {
                return false;
            }
            ord = match ord.checked_add(1) {
                Some(v) => v,
                None => return false,
            };
        }
        if walk.close().is_err() {
            return false;
        }
        if walk.last() != a.stage.order() as i32 {
            return false;
        }
        i += 1;
    }
    true
}

/// 造一个槽位与内容都给定的账本（谓词不许改内部字段，只能照实登记）。
fn fill_ledger(rows: &[Sample]) -> Ledger {
    let mut l = Ledger::new();
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(s) = rows.get(i) {
            let _ = l.register(*s);
        }
        i += 1;
    }
    l
}

/// SI13 的后段：账本逐格对账。
///
/// 账本记的是"这十四个数字各自对不对"，不是"结论好不好看"。三条硬规矩：
/// 槽位必须严格等于当前长度（跳号与越界都拒且计数）、成格值必须逐格等于
/// `VALS`（第 8 格是红档率 929）、败格值一律归零且错误码必须等于 `REJECT_CODES`。
fn ledger_ok() -> bool {
    let l = Ledger::new();
    if !l.is_empty() || l.len() != 0 || l.rejects() != 0 || l.is_frozen() {
        return false;
    }
    if l.sample(0).is_some() || l.ok_count() != 0 || l.err_count() != 0 {
        return false;
    }
    if l.seal() == 0 {
        return false;
    }
    // 建账本：14 格、8 成 6 败、逐格对账通过。
    let b = build_ledger();
    if b.len() != SAMPLES || b.ok_count() != ACCEPT || b.err_count() != REJECT {
        return false;
    }
    if b.audit().is_err() || b.rejects() != 0 || b.is_frozen() {
        return false;
    }
    if b.seal() == 0 || b.replay().is_err() {
        return false;
    }
    if b.sample(SAMPLES).is_some() {
        return false;
    }
    // 每格的槽位与长度对得上，且前 8 格是统计值、后 6 格是缺陷码。
    let mut i = 0usize;
    while i < SAMPLES {
        let s = match b.sample(i) {
            Some(v) => v,
            None => return false,
        };
        if s.slot as usize != i {
            return false;
        }
        if i < ACCEPT {
            let w = match VALS.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if !s.ok || s.code != 0 || s.value != w {
                return false;
            }
        } else {
            let w = match REJECT_CODES.get(i - ACCEPT) {
                Some(v) => *v,
                None => return false,
            };
            if s.ok || s.value != 0 || s.code != w {
                return false;
            }
        }
        i += 1;
    }
    // 第 8 格必须是红档率（这一格是"账本里有没有红"的唯一数字凭据）。
    if let Some(cell) = VALS.get(7) {
        if *cell != 929 {
            return false;
        }
    } else {
        return false;
    }
    // 跳号登记：槽位大于当前长度一律拒，且不入账、计数加一。
    let mut g = Ledger::new();
    if g.register(Sample { slot: 1, op: OP_CORPUS, ok: true, code: 0, value: 14 }).is_ok() {
        return false;
    }
    if g.len() != 0 || g.rejects() != 1 || !g.is_empty() {
        return false;
    }
    // 越界登记。
    if g.register(Sample { slot: SAMPLES as u8, op: OP_CORPUS, ok: true, code: 0, value: 14 })
        .is_ok()
    {
        return false;
    }
    if g.len() != 0 || g.rejects() != 2 {
        return false;
    }
    // 顺序登记到满，再多一格就被 SAMPLES 拦住。
    let mut seq = Ledger::new();
    let mut i = 0usize;
    while i < SAMPLES {
        let ok_cell = i < ACCEPT;
        let mut code = 0u32;
        let mut value = 0u32;
        if ok_cell {
            if let Some(v) = VALS.get(i) {
                value = *v;
            }
        } else {
            let back = match i.checked_sub(ACCEPT) {
                Some(v) => v,
                None => return false,
            };
            if let Some(v) = REJECT_CODES.get(back) {
                code = *v;
            }
        }
        let cell = Sample {
            slot: i as u8,
            op: OP_WRITE,
            ok: ok_cell,
            code,
            value,
        };
        if seq.register(cell).is_err() {
            return false;
        }
        i += 1;
    }
    if seq.len() != SAMPLES || seq.audit().is_err() {
        return false;
    }
    if seq.register(Sample { slot: SAMPLES as u8, op: OP_WRITE, ok: true, code: 0, value: 1 })
        .is_ok()
    {
        return false;
    }
    // 重复槽位（回号）必须被拒。
    let mut dup = Ledger::new();
    if dup.register(Sample { slot: 0, op: OP_CORPUS, ok: true, code: 0, value: 14 }).is_err() {
        return false;
    }
    if dup.register(Sample { slot: 0, op: OP_CORPUS, ok: true, code: 0, value: 14 }).is_ok() {
        return false;
    }
    if dup.len() != 1 || dup.rejects() != 1 {
        return false;
    }
    // 篡改：成格值漂一个字节 → SI_EVALUE。
    let tampered = [
        Sample { slot: 0, op: OP_CORPUS, ok: true, code: 0, value: 15 },
        Sample { slot: 1, op: OP_CORPUS, ok: true, code: 0, value: 8 },
        Sample { slot: 2, op: OP_CORPUS, ok: true, code: 0, value: 6 },
        Sample { slot: 3, op: OP_BACKUP, ok: true, code: 0, value: 2 },
        Sample { slot: 4, op: OP_OPEN, ok: true, code: 0, value: 1 },
        Sample { slot: 5, op: OP_MKTEMP, ok: true, code: 0, value: 2 },
        Sample { slot: 6, op: OP_WRITE, ok: true, code: 0, value: 3 },
        Sample { slot: 7, op: OP_REPLAY, ok: true, code: 0, value: 929 },
        Sample { slot: 8, op: OP_CLOSE, ok: false, code: SI_ESTATE, value: 0 },
        Sample { slot: 9, op: OP_CLOSE, ok: false, code: SI_ESTAGE, value: 0 },
        Sample { slot: 10, op: OP_CLOSE, ok: false, code: SI_ECODE, value: 0 },
        Sample { slot: 11, op: OP_CLOSE, ok: false, code: SI_EVALUE, value: 0 },
        Sample { slot: 12, op: OP_CLOSE, ok: false, code: SI_ENOTE, value: 0 },
        Sample { slot: 13, op: OP_CLOSE, ok: false, code: SI_EDEFAULT, value: 0 },
    ];
    if tampered.len() != SAMPLES {
        return false;
    }
    match fill_ledger(&tampered).audit() {
        Ok(()) => return false,
        Err(c) => {
            if c != SI_EVALUE {
                return false;
            }
        }
    }
    // 篡改：败格留了值 → SI_EVALUE。
    let mut leak = tampered;
    if let Some(x) = leak.get_mut(9) {
        x.value = 3072;
    }
    match fill_ledger(&leak).audit() {
        Ok(()) => return false,
        Err(c) => {
            if c != SI_EVALUE {
                return false;
            }
        }
    }
    // 篡改：败格换了缺陷码 → SI_EVALUE。
    let mut wrong_code = tampered;
    if let Some(x) = wrong_code.get_mut(11) {
        x.code = SI_ESTATE;
    }
    match fill_ledger(&wrong_code).audit() {
        Ok(()) => return false,
        Err(c) => {
            if c != SI_EVALUE {
                return false;
            }
        }
    }
    // 篡改：成格却带错误码 → SI_EVALUE。
    let mut lying_ok = tampered;
    if let Some(x) = lying_ok.get_mut(2) {
        x.code = SI_ECOUNT;
    }
    match fill_ledger(&lying_ok).audit() {
        Ok(()) => return false,
        Err(c) => {
            if c != SI_EVALUE {
                return false;
            }
        }
    }
    // 篡改：少一格 → 账本不平（SI_EBAL），不是"少几格就算几分"。
    let mut short = tampered;
    if let Some(x) = short.get_mut(13) {
        x.slot = 12;
    }
    match fill_ledger(&short).audit() {
        Ok(()) => return false,
        Err(c) => {
            if c != SI_ESLOT && c != SI_EBAL {
                return false;
            }
        }
    }
    // 篡改：红档率那一格被改回 1000 → 必露。
    let mut fake_green = tampered;
    if let Some(x) = fake_green.get_mut(7) {
        x.value = 1000;
    }
    match fill_ledger(&fake_green).audit() {
        Ok(()) => return false,
        Err(c) => {
            if c != SI_EVALUE {
                return false;
            }
        }
    }
    // 冻结：指纹非零、冻结后拒登记、二次冻结被拒。
    let mut f = build_ledger();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if root == 0 || !f.is_frozen() || f.root() != root {
        return false;
    }
    // 冻结当场可重放：重建账本的指纹与冻结指纹逐位相同。
    if f.replay() != Ok(root) {
        return false;
    }
    if f.register(Sample { slot: 14, op: OP_CORPUS, ok: true, code: 0, value: 14 }).is_ok() {
        return false;
    }
    if f.rejects() != 1 || f.len() != SAMPLES {
        return false;
    }
    match f.freeze() {
        Ok(_) => return false,
        Err(c) => {
            if c != SI_EBAL {
                return false;
            }
        }
    }
    // 冻结后仍发生的拒登记会留下痕迹：重放必须读不出同一个指纹。
    match f.replay() {
        Ok(_) => return false,
        Err(c) => {
            if c != SI_ESEAL {
                return false;
            }
        }
    }
    // 冻结前与冻结后的指纹同值（freeze 只登记，不改账）。
    let g = build_ledger();
    if g.seal() != root {
        return false;
    }
    // 账本与矩阵/报表同读数：账本里的八个统计值就是报表的八个字段。
    let rep = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let from_report = [
        rep.total,
        rep.accept,
        rep.reject,
        rep.rotated,
        rep.noisy,
        rep.halted,
        rep.aborted,
    ];
    let mut i = 0usize;
    while i < from_report.len() {
        let got = match from_report.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let cell = match build_ledger().sample(i) {
            Some(v) => v,
            None => return false,
        };
        if cell.value != got {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 判据入口：十三格
// ---------------------------------------------------------------------------

/// 本卡的十三格判据（`SI01 … SI13`）。
///
/// 十三格是一条纵线，不是十四个独立测试：名称/枚举/画像/形状先把"能不能读"立住，
/// 闸门纵线与归因三元组再立"读对了没有"，语料/槽位/红档立"覆盖面是不是真的"，
/// 文案与释义立"运维能不能从 stderr 反推现场"，最后用阶段闸门与账本把"这十四个
/// 数字各自对不对"钉成可重放的指纹。
pub fn run_unx11686_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11686_sed_i_enospc");

    s.add("SI01-名称与段基址", name_ok(), "static");

    s.add("SI02-枚举与九闸次序", enums_ok(), "static");

    s.add("SI03-目录画像与改名面", profile_ok(), "static");

    s.add("SI04-形状合法域", shape_ok(), "static");

    s.add("SI05-先改名后写出的纵线", define_ok(), "static");

    s.add("SI06-归因三元组与三字节数", abort_ok(), "static");

    s.add("SI07-六维对判次序", judge_ok(), "static");

    s.add("SI08-四档矩阵与渲染", matrix_ok(), "static");

    s.add("SI09-语料三份陈述", corpus_ok(), "static");

    s.add("SI10-八活槽覆盖", slots_ok(), "static");

    s.add("SI11-六红槽单维复刻", reds_ok(), "static");

    s.add("SI12-报错文案逐字", message_ok(), "static");

    s.add(
        "SI13-冻结重放四注入",
        detail_ok()
            && gate_ok()
            && ledger_ok()
            && {
                let mut l = build_ledger();
                let a = l.audit() == Ok(());
                let b = match l.freeze() {
                    Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                    Err(_) => false,
                };
                let inj = inject_four();
                let four = inj.iter().all(|x| *x);
                a && b && four
            },
        "static",
    );
    s
}

// ---------------------------------------------------------------------------
// 测试面（std 下跑；生产码里不许出现的断言全部关在这里）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 取一槽实判（越界即 panic，测试里允许）。
    fn act(i: usize) -> Outcome {
        match case_at(i) {
            Some(c) => define(&c.shape),
            None => panic!("槽位越界"),
        }
    }

    /// 取一槽形状。
    fn shp(i: usize) -> Shape {
        match case_at(i) {
            Some(c) => c.shape,
            None => panic!("槽位越界"),
        }
    }

    #[test]
    fn t01_seg_base_and_codes() {
        assert_eq!(SEG_BASE, 0x6d320);
        assert_eq!(SI_ESTAGE, SEG_BASE + 1);
        assert_eq!(SI_EBAL, SEG_BASE + 0x0d);
        assert!(name_ok());
        assert_eq!(Fault::Ok.code(), 0);
        assert_eq!(Fault::Estate.code(), SI_ESTATE);
        assert_eq!(fault_short(Fault::Default), "default");
    }

    #[test]
    fn t02_scale_constants() {
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(ACCEPT + REJECT, CORPUS);
    }

    #[test]
    fn t03_stage_order_is_a_vertical() {
        assert_eq!(Stage::Backup.order(), 2);
        assert_eq!(Stage::Write.order(), 4);
        assert!(Stage::Backup.order() < Stage::Write.order());
        assert_eq!(Stage::at(4), Some(Stage::Write));
        assert_eq!(Stage::at(9), None);
        assert_eq!(STAGES.len(), 9);
        assert!(!Stage::Exit.failure());
        assert!(Stage::Write.failure());
    }

    #[test]
    fn t04_backup_gate_carries_no_residue() {
        // 备份闸上盘里还没有 sedXXXXXX：残痕责任从建件闸起算。
        assert!(!Stage::Backup.residue_plane());
        assert!(Stage::Mktemp.residue_plane());
        assert!(Stage::Write.residue_plane());
        assert!(!Stage::Attr.orig_plane());
        assert!(!Stage::Attr.abort_plane());
        assert!(Stage::Rename.abort_plane() && Stage::Rename.orig_plane());
    }

    #[test]
    fn t05_note_families_are_disjoint() {
        for n in NOTES.iter() {
            let fam = n.success() as u32
                + n.abort_family() as u32
                + n.tail_family() as u32
                + n.prelude_family() as u32
                + n.defect_family() as u32;
            assert_eq!(fam, 1, "落点 {:?} 同时属于两族", n);
        }
        assert_eq!(NOTES.len(), 20);
        assert!(Note::WriteEnospc.abort_family());
        assert!(Note::CloseSwallowed.noisy_success());
        assert!(Note::SilentRotate.defect_family());
    }

    #[test]
    fn t06_tier_projection_prefers_attribution() {
        let aborted = Outcome {
            stage: Stage::Exit,
            code: EXIT_OK,
            value: 0,
            note: Note::Complete,
            default: true,
        };
        assert_eq!(Tier::of(&aborted), Tier::Aborted);
        let noisy = Outcome {
            stage: Stage::Exit,
            code: EXIT_OK,
            value: 4096,
            note: Note::LinkCut,
            default: false,
        };
        assert_eq!(Tier::of(&noisy), Tier::Noisy);
        assert_eq!(Tier::Rotated.index(), 0);
        assert_eq!(Tier::Aborted.index(), 3);
        assert!(!Tier::Halted.success());
    }

    #[test]
    fn t07_tool_divergences_are_pairs() {
        assert_eq!(Tool::Gnu.io_exit(), 4);
        assert_eq!(Tool::Busybox.io_exit(), 1);
        assert_eq!(Tool::Gnu.usage_exit(), 2);
        assert_ne!(Tool::Gnu.cleans_temp_on_fail(), Tool::Busybox.cleans_temp_on_fail());
        assert_ne!(
            Tool::Gnu.rotates_after_attr_fail(),
            Tool::Busybox.rotates_after_attr_fail()
        );
        assert!(!Tool::Busybox.supports_backup_suffix());
        assert!(!Tool::Busybox.attempts_chown());
        assert_eq!(Tool::Busybox.attr_ceiling(), A_MODE);
    }

    #[test]
    fn t08_rotate_modes_split_the_value_semantics() {
        assert!(!Rotate::Bare.moves_original());
        assert!(Rotate::Suffix.moves_original());
        assert!(Rotate::Bare.uses_tempfile());
        assert!(!Rotate::None.uses_tempfile());
        assert!(Rotate::Suffix.rotates());
        assert_eq!(Rotate::Bare.name(), "inplace");
        assert_eq!(Rotate::Suffix.name(), "inplace-suffix");
    }

    #[test]
    fn t09_dir_profiles_are_three_different_worlds() {
        let ps = dir_profiles();
        assert!(ps[0].data_full() && ps[0].dir_can_grow && ps[0].can_add_entry());
        assert!(ps[1].data_full() && !ps[1].dir_can_grow && !ps[1].can_add_entry());
        assert!(!ps[2].writable && !ps[2].can_mktemp());
        assert_eq!(ps[2].free_bytes(), Some(4096));
        assert!(ps[0].close_can_fail() && !ps[1].close_can_fail());
        assert_ne!(ps[0].seal(), ps[1].seal());
    }

    #[test]
    fn t10_bad_dir_profiles_are_rejected() {
        let blank = DirProfile::blank();
        assert!(!blank.well_formed());
        let odd = DirProfile {
            dir: "odd.d",
            bsize: 700,
            data_free: 1,
            dir_can_grow: true,
            delalloc: false,
            writable: true,
        };
        assert!(!odd.well_formed());
    }

    #[test]
    fn t11_all_shapes_are_valid_and_distinct() {
        assert!(shape_ok());
        assert!(Shape::blank().valid());
        let mut seals = [0u64; ACCEPT];
        for (i, s) in SHAPES.iter().enumerate().take(ACCEPT) {
            assert!(!s.tag.is_empty());
            seals[i] = s.seal();
        }
        for a in 0..ACCEPT {
            for b in a + 1..ACCEPT {
                assert_ne!(seals[a], seals[b]);
            }
        }
    }

    #[test]
    fn t12_invalid_shapes_are_refused() {
        let mut s = shp(0);
        s.size = SIZE_MAX + 1;
        assert!(!s.valid());
        let mut s = shp(0);
        s.written = s.want + 1;
        assert!(!s.valid());
        let mut s = shp(0);
        s.hardlinks = 0;
        assert!(!s.valid());
        // 未跟随的符号链接归 F11464，本卡不收。
        let mut s = shp(0);
        s.is_symlink = true;
        assert!(!s.valid());
        // busybox 没有 -i=SUFFIX。
        let mut s = shp(6);
        s.tool = Tool::Busybox;
        assert!(!s.valid());
        // 属性段一次只塌一位。
        let mut s = shp(3);
        s.attr_failed = A_ALL;
        assert!(!s.valid());
        // root 之下 fchown 不可能失败。
        let mut s = shp(3);
        s.root_caller = true;
        assert!(!s.valid());
    }

    #[test]
    fn t13_clone_helpers_move_one_dimension_only() {
        let base = shp(5);
        let w = base.clone_written(2049);
        assert_eq!(w.written, 2049);
        assert_eq!(w.size, base.size);
        assert_ne!(w.seal(), base.seal());
        assert_eq!(base.clone_written(base.written).seal(), base.seal());
        let e = base.clone_errno(ERR_EIO);
        assert_eq!(e.errno_seen, ERR_EIO);
        assert_eq!(e.write_err, base.write_err);
        let r = base.clone_rotate(Rotate::Bare, "");
        assert!(r.valid());
        assert!(r.suffix.is_empty());
    }

    #[test]
    fn t14_write_gate_bare_keeps_original_bytes() {
        let o = act(5);
        assert_eq!(o.stage, Stage::Write);
        assert_eq!(o.code, EXIT_IO);
        assert_eq!(o.value, 3072);
        assert_eq!(o.note, Note::WriteEnospc);
        assert!(o.default);
        let s = shp(5);
        assert!(orig_intact(&s, &o));
        assert_eq!(backup_bytes(&s, &o), 0);
        assert!(content_balanced(&s, &o));
    }

    #[test]
    fn t15_write_gate_suffix_reports_zero() {
        // 本卡的心脏：满盘上 rename 先成，原件名字已经不在了。
        let o = act(6);
        let s = shp(6);
        assert_eq!(o.stage, Stage::Write);
        assert_eq!(o.value, 0);
        assert_eq!(backup_bytes(&s, &o), 3072);
        assert!(!orig_intact(&s, &o));
        assert!(s.original_moved());
        assert_eq!(s.untouched_bytes(), 0);
        assert!(content_balanced(&s, &o));
        assert_eq!(
            message(&s, &o).as_str(),
            "sed: couldn't write 1 output file to No space left on device"
        );
    }

    #[test]
    fn t16_attr_gate_gnu_rotates_anyway() {
        let o = act(3);
        let s = shp(3);
        assert_eq!(o.stage, Stage::Attr);
        assert_eq!(o.code, EXIT_IO);
        assert_eq!(o.value, 4096);
        assert!(!o.default);
        assert!(rotation_landed(&s, &o));
        assert!(!o.ok());
        assert!(content_balanced(&s, &o));
        assert_eq!(message(&s, &o).as_str(), "sed: can't chown cfg.conf: Operation not permitted");
    }

    #[test]
    fn t17_attr_gate_busybox_abandons_and_leaves_residue() {
        let mut s = Shape::blank();
        s.tool = Tool::Busybox;
        s.want_attrs = A_MODE;
        s.attr_failed = A_MODE;
        s.attr_err = true;
        s.size = 3072;
        s.want = 4096;
        s.written = 4096;
        s.errno_seen = ERR_EACCES;
        assert!(s.valid());
        let o = define(&s);
        assert_eq!(o.code, EXIT_BB);
        assert_eq!(o.value, 3072);
        assert_eq!(leftover(&s, &o), 4096);
        assert!(temp_present(&s, &o));
        assert!(!msg_temp_residue(&s, &o).is_empty());
        assert!(content_balanced(&s, &o));
    }

    #[test]
    fn t18_close_gate_only_breaks_on_tools_that_look() {
        let mut s = shp(5);
        s.write_err = false;
        s.close_err = true;
        s.written = s.want;
        s.errno_seen = ERR_ENOSPC;
        assert!(s.valid());
        let gnu = define(&s);
        assert_eq!(gnu.stage, Stage::Close);
        assert!(gnu.default);
        assert_eq!(gnu.value, 3072);
        // busybox 不看 close 返回值：同一现场表观落位。
        let mut b = Shape::blank();
        b.tool = Tool::Busybox;
        b.size = 3072;
        b.want = 4096;
        b.written = 4096;
        b.close_err = true;
        b.errno_seen = ERR_ENOSPC;
        let bb = define(&b);
        assert_eq!(bb.stage, Stage::Exit);
        assert_eq!(bb.note, Note::CloseSwallowed);
        assert_eq!(bb.code, EXIT_OK);
        assert_eq!(deferred_lost(&b, &bb), 4096);
        assert!(!content_balanced(&b, &bb));
        assert_eq!(Tier::of(&bb), Tier::Noisy);
    }

    #[test]
    fn t19_silent_rotation_is_a_defect() {
        let mut s = shp(5);
        s.write_err = false;
        s.written = 2048;
        s.errno_seen = ERR_NONE;
        assert!(s.valid());
        let o = define(&s);
        assert_eq!(o.stage, Stage::Write);
        assert_eq!(o.note, Note::SilentRotate);
        assert!(!o.default);
        assert!(!o.ok());
        let line = message(&s, &o);
        assert!(line.contains("wrote 2048 of 4096 bytes"));
        assert!(msg_broken(&s, &o).contains("note=silent-rotate"));
    }

    #[test]
    fn t20_foreign_errno_is_not_attributed() {
        let mut s = shp(5);
        s.errno_seen = ERR_EIO;
        let o = define(&s);
        assert_eq!(o.note, Note::ForeignErr);
        assert!(!o.default);
        assert!(!o.attributed());
        assert!(msg_foreign(&s, &o).contains("Input/output error"));
        // ENOSPC 现场不出邻居解释行：同一现场不许被两张纸各判一次。
        assert!(msg_foreign(&shp(5), &act(5)).is_empty());
    }

    #[test]
    fn t21_rename_gate_needs_a_growable_dir() {
        let mut s = Shape::blank();
        s.dir = "legacy.d";
        s.size = 3072;
        s.want = 4096;
        s.written = 4096;
        s.rename_err = true;
        s.errno_seen = ERR_ENOSPC;
        assert!(s.valid());
        let o = define(&s);
        assert_eq!(o.stage, Stage::Rename);
        assert!(o.attributed());
        assert_eq!(o.value, 3072);
        assert!(message(&s, &o).contains("can't rename cfg.d/sedXXXXXX to cfg.conf"));
    }

    #[test]
    fn t22_backup_gate_two_verdicts() {
        let mut s = shp(6);
        s.backup_err = true;
        s.write_err = false;
        s.written = 0;
        s.errno_seen = ERR_EACCES;
        assert!(s.valid());
        let denied = define(&s);
        assert_eq!(denied.note, Note::BackupFail);
        assert!(!denied.default);
        assert_eq!(denied.value, 3072);
        assert!(orig_intact(&s, &denied));
        let mut s2 = s;
        s2.errno_seen = ERR_ENOSPC;
        let full = define(&s2);
        assert_eq!(full.note, Note::BackupEnospc);
        assert!(full.default);
        assert_eq!(leftover(&s2, &full), 0);
    }

    #[test]
    fn t23_hardlinks_are_cut_only_on_success() {
        let o = act(2);
        assert_eq!(o.note, Note::LinkCut);
        assert!(o.ok());
        assert_eq!(Tier::of(&o), Tier::Noisy);
        assert!(shp(2).hardlinks > 1);
        // 中止的轮换不斩硬链接：旧 inode 还在原件名下。
        let h = act(5);
        assert!(!h.landed());
        assert_eq!(h.value, shp(5).size);
    }

    #[test]
    fn t24_define_matches_first_flag_on_every_slot() {
        for i in 0..CORPUS {
            let s = shp(i);
            let o = act(i);
            assert_eq!(o.stage.order(), first_flag(&s), "槽 {} 闸序不符", i);
            assert_eq!(o.stage, define(&s).stage);
        }
        assert_eq!(first_flag(&shp(0)), Stage::Exit.order());
    }

    #[test]
    fn t25_judge_priority_estate_first() {
        let base = match case_at(5) {
            Some(c) => c,
            None => panic!("缺槽"),
        };
        let mut c = base;
        c.claim.stage = Stage::Rename;
        c.claim.code = EXIT_OK;
        c.claim.note = Note::Complete;
        c.claim.value = 999;
        assert_eq!(judge(&c), Fault::Estate);
        let mut c = base;
        c.claim.stage = Stage::Rename;
        c.claim.code = EXIT_USAGE;
        assert_eq!(judge(&c), Fault::Stage);
        let mut c = base;
        c.claim.code = EXIT_USAGE;
        c.claim.value = 0;
        assert_eq!(judge(&c), Fault::Code);
        let mut c = base;
        c.claim.value = 1;
        c.claim.note = Note::RenameEnospc;
        assert_eq!(judge(&c), Fault::Value);
        let mut c = base;
        c.claim.note = Note::CloseEnospc;
        assert_eq!(judge(&c), Fault::Note);
        let mut c = base;
        c.claim.default = false;
        assert_eq!(judge(&c), Fault::Default);
    }

    #[test]
    fn t26_replica_slots_each_break_one_dimension() {
        assert!(reds_ok());
        for i in 0..REJECT {
            let slot = ACCEPT + i;
            let c = match case_at(slot) {
                Some(v) => v,
                None => panic!("缺红档槽"),
            };
            assert_eq!(judge(&c), REJECT_FAULTS[i]);
            assert_eq!(judge(&c).code(), REJECT_CODES[i]);
            assert!(c.tag().ends_with(SWAP_SUFFIX));
            assert_eq!(c.shape.seal(), shp(5).seal());
            assert_eq!(Tier::of(&c.actual()), Tier::Aborted);
        }
    }

    #[test]
    fn t27_claims_restate_the_shapes() {
        assert!(corpus_ok());
        for i in 0..ACCEPT {
            let c = match case_at(i) {
                Some(v) => v,
                None => panic!("缺槽"),
            };
            assert_eq!(judge(&c), Fault::Ok);
            assert_eq!(c.claim.stage, c.actual().stage);
            assert_eq!(c.claim.value, WANT_VALUE[i]);
            assert_eq!(c.claim.note, WANT_NOTE[i]);
            assert_eq!(c.shape.tag, WANT_TAGS[i]);
            assert_eq!(backup_bytes(&c.shape, &c.actual()), WANT_BACKUP[i]);
            assert_eq!(leftover(&c.shape, &c.actual()), WANT_RESIDUE[i]);
        }
    }

    #[test]
    fn t28_matrix_counts_are_independent() {
        let m = Matrix::of();
        assert_eq!(m.slots(), CORPUS);
        assert_eq!(m.goods(), ACCEPT);
        assert!(m.covered());
        assert_eq!(m.perfect_rows(), 3);
        let ab = match m.row(Tier::Aborted) {
            Some(v) => v,
            None => panic!("缺档"),
        };
        assert_eq!(ab.total, 9);
        assert_eq!(ab.red(), REJECT);
        assert!(ab.reds_are_replicas());
        assert_eq!(m.render().matches('\n').count(), TIER_COUNT);
    }

    #[test]
    fn t29_report_render_is_pinned() {
        let rep = match run() {
            Ok(v) => v,
            Err(_) => panic!("报表跑不动"),
        };
        assert!(rep.green());
        assert_eq!(rep.rate(), 1000);
        assert_eq!(rep.render().as_str(), RENDER_OK);
        assert_eq!(rep.zero_cells() + rep.nonzero_cells(), rep.accept);
    }

    #[test]
    fn t30_injection_moves_the_rate_off_a_thousand() {
        let red = match run_with(Some((0, Fault::Code))) {
            Ok(v) => v,
            Err(_) => panic!("注入失败"),
        };
        assert_eq!(red.matched, 13);
        assert_eq!(red.rate(), 929);
        assert!(!red.green());
        assert_eq!(red.gate(), Err(SI_ECOUNT));
        assert!(matches!(
            run_with(Some((CORPUS, Fault::Ok))),
            Err(c) if c == SI_ESLOT
        ));
    }

    #[test]
    fn t31_messages_are_verbatim() {
        assert_eq!(
            msg_gnu_write_fail(1, ERR_ENOSPC).as_str(),
            "sed: couldn't write 1 output file to No space left on device"
        );
        assert_eq!(
            msg_gnu_write_fail(2, ERR_ENOSPC).as_str(),
            "sed: couldn't write 2 output files to No space left on device"
        );
        assert_eq!(
            msg_gnu_mktemp("cfg.d", ERR_ENOSPC).as_str(),
            "sed: couldn't open temporary file cfg.d/sedXXXXXX: No space left on device"
        );
        assert_eq!(
            msg_bb_unrecognized("Z").as_str(),
            "sed: unrecognized option: 'Z'"
        );
        assert_eq!(
            msg_gnu_backup_fail("cfg.conf", "cfg.conf.bak", ERR_EACCES).as_str(),
            "sed: can't rename cfg.conf to cfg.conf.bak: Permission denied"
        );
        assert_eq!(strerror(ERR_EDQUOT), "Quota exceeded");
        assert_eq!(strerror(21), "");
        assert!(!errno_legal(21));
    }

    #[test]
    fn t32_detail_lines_carry_every_number() {
        assert!(detail_ok());
        let c = match case_at(6) {
            Some(v) => v,
            None => panic!("缺槽"),
        };
        let line = detail(&c);
        assert!(line.starts_with("cfg.conf "));
        assert!(line.contains(" orig=0 "));
        assert!(line.contains(" backup=3072 "));
        assert!(line.contains(" mode=inplace-suffix "));
        assert!(line.contains(" abort=true"));
        assert!(line.ends_with('\n'));
    }

    #[test]
    fn t33_gate_walk_is_monotonic() {
        let mut g = Gate::new();
        for s in STAGES.iter() {
            assert_eq!(g.step(*s), Ok(()));
        }
        assert_eq!(g.close(), Ok(STAGES.len() as u32));
        assert_eq!(g.step(Stage::Exit), Err(SI_ESTATE));
        assert_eq!(g.close(), Err(SI_ESTATE));
        let mut h = Gate::new();
        assert_eq!(h.step(Stage::Write), Ok(()));
        assert_eq!(h.step(Stage::Backup), Err(SI_ESTAGE));
        assert_eq!(h.step(Stage::Write), Ok(()));
        assert_eq!(h.steps(), 2);
    }

    #[test]
    fn t34_ledger_slots_are_strict() {
        let mut l = Ledger::new();
        assert!(l.is_empty());
        assert_eq!(
            l.register(Sample { slot: 1, op: OP_CORPUS, ok: true, code: 0, value: 14 }),
            Err(SI_ESLOT)
        );
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.len(), 0);
        assert_eq!(
            l.register(Sample { slot: 0, op: OP_CORPUS, ok: true, code: 0, value: 14 }),
            Ok(())
        );
        assert_eq!(l.len(), 1);
        assert_eq!(l.sample(0).map(|x| x.value), Some(14));
        assert!(l.sample(1).is_none());
    }

    #[test]
    fn t35_ledger_audit_catches_every_tamper() {
        let b = build_ledger();
        assert_eq!(b.audit(), Ok(()));
        assert_eq!(b.len(), SAMPLES);
        assert_eq!(b.ok_count(), ACCEPT);
        assert_eq!(b.err_count(), REJECT);
        let mut bad = [Sample::blank(); SAMPLES];
        let mut i = 0usize;
        while i < SAMPLES {
            match b.sample(i) {
                Some(s) => {
                    if let Some(slot) = bad.get_mut(i) {
                        *slot = s;
                    }
                }
                None => panic!("账本缺格"),
            }
            i += 1;
        }
        if let Some(x) = bad.get_mut(0) {
            x.value = 15;
        }
        let mut l = Ledger::new();
        for s in bad.iter() {
            assert_eq!(l.register(*s), Ok(()));
        }
        assert_eq!(l.audit(), Err(SI_EVALUE));
    }

    #[test]
    fn t36_freeze_and_replay_round_trip() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => panic!("冻结失败"),
        };
        assert_ne!(root, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(SI_EBAL));
    }

    #[test]
    fn t37_eighth_val_cell_is_the_red_rate() {
        assert_eq!(VALS.len(), ACCEPT);
        for i in 0..7 {
            assert_ne!(VALS[i], 929);
        }
        assert_eq!(VALS[7], 929);
        let rate = match run_with(Some((0, Fault::Code))) {
            Ok(v) => v.rate(),
            Err(_) => panic!("注入失败"),
        };
        assert_eq!(rate, 929);
    }

    #[test]
    fn t38_all_thirteen_checks_are_green() {
        let s = run_unx11686_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "红灯：{}", s.red_items_text());
    }

    #[test]
    fn t39_inject_four_paths_are_independent() {
        let four = inject_four();
        assert_eq!(four.len(), 4);
        assert!(four.iter().all(|x| *x), "四路注入有假：{:?}", four);
    }

    #[test]
    fn t40_predicates_are_all_true() {
        assert!(name_ok());
        assert!(enums_ok());
        assert!(profile_ok());
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
}
