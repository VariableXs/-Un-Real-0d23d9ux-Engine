//! UNX-F11683 dd ENOSPC 中止与部分写入判据
//!
//! `dd` 和 `cp`/`tee` 最大的不同是它**按块行事**：它不写"一个文件"，它写
//! "若干整块 + 至多一个残块"。于是设备满在 `dd` 这里不是一个布尔事件，而是
//! 一组必须互相自洽的数字：
//!
//! ```text
//! dd: error writing 'disk.img': No space left on device
//! 2+0 records in
//! 1+1 records out
//! 6144 bytes (6.1 kB, 6.0 KiB) copied
//! ```
//!
//! 四行里只有第三行的第二个数字（`+1`）说明"死在半块上"。把这段读成"写了 1 块
//! 就挂了"会把 2048 个已经落盘的字节算丢；读成"写完了"更会把残件当完整件送去
//! 挂载。本卡判的就是**半块账**：读侧 records、写侧 records（含 partial-block
//! 标记）、落盘字节、丢失字节，四者必须与块几何一起自洽，且中止文案要落在正确的
//! 工具家族上。
//!
//! `dd` 的块算术还有两条 `cp`/`tee` 完全没有的维度：
//!
//! * **`bs=` 与 `ibs=/obs=`**：`bs=` 同时定读块与写块；`ibs=512 obs=4096` 时
//!   读侧数 8 格、写侧数 1 格，两侧的 `N+M` 天然不相等，此时"records out 的
//!   partial"不再等于"写挂了"——正常收尾也会出 `1+1 records out`。判据必须
//!   能把**成功的残块**与**中止的残块**分开，否则每一刀都是误杀。
//! * **`skip=`/`seek=`/`count=`**：`skip=` 用 lseek 跨过输入前段，被跨过的字节
//!   一次都不进读账；`seek=` 在目的件里造洞，洞不吃块，所以**满盘也能 seek 成功**，
//!   随后第一笔写立刻整块失败（`0+0 records out`、字节 0、**没有 partial 标记**）；
//!   `count=` 只截读侧，截掉的字节既不在读账也不在写账——把它写成"丢失"是本卡
//!   显式判死的一格。
//! * **`status=none`**：它只压掉汇总两行，不压错误文案，也不改退出码。于是
//!   "没有 records 行的成功"与"没有 records 行的中止"在日志里长得一模一样，
//!   而后者退 1。本卡把"无汇总"单立一道闸：它永远不给锚位（无从对账），
//!   并且**不许**被当成绿——这是本卡最典型的假绿来源。
//!
//! 工具家族必须分表判：GNU coreutils 8.x 打 `dd: writing 'X': No space left on
//! device`，9.x 起打 `dd: error writing 'X': No space left on device`，busybox
//! 打 `dd: write error to 'X': No space left on device` 且**不打字节行**。三式
//! 混认，"哪个块坏了、落了多少字节"就无法从日志复原；busybox 缺字节行时，本卡
//! 用块几何把字节数**复原**出来，而不是跟着日志报"未知"。区分三式的仍然是
//! partial-block 那一格：`M+1 records out`。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 九道闸，秩从 **1** 起算；`Gate` 只接受"前进到下一秩"，倒退、跳档、封印后
//! 再动各留痕迹。前一闸不产出事实，后一闸就没有输入。
//!
//! 1. `args`（秩 1）——命令行本身。`bs=0`、未知 operand、`conv=` 越界值都在
//!    这里退 2，一个字节都没碰过。**`conv=noerror`/`conv=sync` 也在这里出局**：
//!    它们改的是读账与跳过账，归 UNX-F11697；本卡在这一闸就把这样的行判成
//!    "不裁定"，而不是偷偷按普通中止算。
//! 2. `open`（秩 2）——目的件的打开/创建。GNU 在打不开目标时打
//!    `dd: failed to open 'X': No such file or directory` 并**不产出任何汇总行**；
//!    这一闸之后才谈得上"件里有多少字节"，所以留档字节只能在 open 之前判为 0。
//! 3. `seek`（秩 3）——`seek=N` 的 lseek。洞不吃块，满盘也能跳过；只有偏移
//!    算不动（乘法溢出、越界）才在这里死。它在写之前：可用空间就是被这一闸
//!    的洞语义改写的。
//! 4. `read`（秩 4）——读侧循环。`skip=` 跨过的字节不进读账，`count=` 在这里
//!    截断；读不满一块就是 `N+1 records in` 的那个 `+1`，它是**输入本来就短**，
//!    不是错误。读本身红（EIO）才终止在这一闸。
//! 5. `write`（秩 5）——写侧循环，本卡的主战场。落盘字节 = min(请求, 余量)；
//!    `landed % obs > 0` 才是 partial-block；`landed == 0` 是整块失败、**没有**
//!    partial 标记。延迟分配在这里"当场绿"，真相迟到 `conv=fsync` 才爆，
//!    本卡把这一格归入同一闸、用注解区分（ errno 面与 F11681 同锚）。
//! 6. `records`（秩 6）——对账闸。把日志里申报的 `N+M records in`、
//!    `N+M records out`、字节数与几何算出来的三对数逐一比对。它必须在 write
//!    之后：先有物理落点，才谈得上"申报的数字对不对"；放在前面会把真实半块
//!    失败也算成谎报。
//! 7. `summary`（秩 7）——汇总存在性闸。`status=none` 让第 6 闸无从对账，
//!    于是判定终点前移到这一闸，且**锚位强制归零**。它排在 records 之后：
//!    "有没有汇总"这个事实只有在打算对账时才有意义。
//! 8. `abort`（秩 8）——中止结算闸。写侧真的失败、汇总在场且数字自洽、
//!    errno 与 F11681 的总锚同值（28）、退出码 1 四件一起成立，才在这里落锚
//!    并把文案家族钉死。任何一件缺了就落不进这一闸。
//! 9. `exit`（秩 9）——正常收尾。全绿、汇总在场、码 0。它是唯一允许的
//!    "封印秩"（`Gate::close` 只接受已经站到秩 9 的账），因为只有走到这里
//!    整条纵线才算过完。
//!
//! ## 关键差异（本卡深度所在）
//!
//! * **与 F11681（ENOSPC 总锚，段基址 `0x6d130`，位距 `0x20`）的边界**：总锚判
//!   的是**那一次** `write()`/`fsync()` 的 (返回, errno) 配对；它不知道 `dd` 已经
//!   写了几个整块、残块多少字节、`records out` 有没有 `+1`、退出码是几。本卡
//!   把锚的现场折算成"块账四元组 + 文案家族 + 退出码"，并**只在 errno_seen==28
//!   且写返回 -1 的锚亮着时**才允许落 `abort` 闸——邻居 errno（EIO=5）冒充
//!   ENOSPC 的 `dd` 行在本卡一律 Refused。两卡不互相 import，本卡用自声明的
//!   `ANCHOR_F11681_SEG`/`GAP` 常量署名，由 `anchors_agree()` 复算对齐。
//! * **与 F11682（cp 中止，段基址 `0x6d140`，位距 `0x10`，直接前置）的边界**：
//!   cp 只有一个目的地、没有块边界，它的主判据是"部分件字节 + 中止文案落点 +
//!   退出码"三元组；本卡**不重判**这三元组的对错，只把它当作外部事实引进
//!   （`errno_seen`、`flavor`），本卡的独有一维是**块算术**：records in/out 的
//!   `N+M`、partial-block 标记、`bs` 与 `ibs/obs` 的分离、`skip/seek/count`
//!   对计数各自的影响。同一次满盘，cp 卡报"留了 4096 字节"，本卡报
//!   "1+1 records out、6000 字节落盘、2192 字节丢失"——两套数字各管一件事，
//!   互相顶替就是错位。
//! * **与 F11697（`conv=noerror`/`sync` 的跳过账）的边界**：`noerror` 把纵线
//!   从"中止"改成"续传并计 skipped"，读账/写账的语义整个换掉。本卡**点名**这条
//!   边界并在 `args` 闸拒绝裁定这类行——语料里那格 Estate 缺陷槽就是一条被申报
//!   成本卡活行的 `conv=noerror` 记录，判序第一维（成败互报）必须当场抓住它，
//!   而不是让它安静地走进 `abort` 闸。
//! * **与 F11684（tee 半程失败，段基址 `0x6d160`，后继）的边界**：tee 的"半程"
//!   是两支路字节分叉，dd 的"半块"是单支路里的块内分叉；两词一物两面，段位相差
//!   `0x10`，由后继卡负责链上结算。
//! * **本卡的排他职责**：① 半块账（`records` 闸：申报数与几何数逐格对）；
//!   ② partial-block 标记作为家族判别的支点；③ `bs`/`ibs`/`obs` 与
//!   `skip`/`seek`/`count` 的计数算术；④ `status=none` 的假绿拒锚；
//!   ⑤ GNU 8.x / GNU 9.x / busybox 三式文案分表且 busybox 缺字节行时的字节复原。
//! * **谁判死谁放行**：现场满、`-1/28` 成对、汇总在场、records 与几何逐格一致、
//!   码 1、文案家族合法 → 放行并落锚；汇总缺席、数字对不上、errno 串档、
//!   码 0、`conv` 越界 → 一律判死。
//!
//! 判据六维：结束阶段（gate）、退出码（code）、落盘字节（value）、文案家族与
//! 形态（note）、中止锚位（default：是否"dd 面 ENOSPC 中止 + records 自洽"）、
//! 成败申报（estate）。判序固定：状态 → 阶段 → 退出码 → 字节 → 注解 → 锚位。
//!
//! 语料 14 槽：8 槽申报正确（完整 3、无汇总 1、中止 3、用法 1），6 槽里 5 槽
//! 复刻同一"写满在中块"现场、各挂一维错报（stage/code/value/note/default），
//! 第 6 槽（Estate）特意用一条 `conv=noerror` 行申报成活行——本卡的成败互报
//! 维度要抓的就是这个越界。账本 14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码，
//! 冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11683）。
pub const SEG_BASE: u32 = 0x6d150;

/// 交叉署名：F11681「ENOSPC 总锚」的段基址。本卡**不 import** 那个模块的
/// 任何符号，只把它的段位与位距原样声明一遍，由 `anchors_agree()` 复算对齐。
pub const ANCHOR_F11681_SEG: u32 = 0x6d130;
/// F11681 与本卡的位距（0x6d150 - 0x6d130）。
pub const ANCHOR_F11681_GAP: u32 = 0x20;
/// 交叉署名：F11682「cp ENOSPC 中止」的段基址（本卡的直接前置）。
pub const ANCHOR_F11682_SEG: u32 = 0x6d140;
/// F11682 与本卡的位距（0x6d150 - 0x6d140）。
pub const ANCHOR_F11682_GAP: u32 = 0x10;

/// 与 F11681 在 errno 面的署名：ENOSPC 在 Linux 就是 28，两张卡必须同数。
pub const ANCHOR_ENOSPC: i32 = 28;
/// 与 F11681/F11682 在文案面的署名：三条卡都只认这一串 strerror 原文。
pub const ANCHOR_ERRNO_TEXT: &str = "No space left on device";
/// 与 F11682 在退出码面的署名：工具面文件错统一退 1，本卡不许自造。
pub const ANCHOR_EXIT_FILE: u32 = 1;

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

/// 闸门道数（秩 1..9）。
pub const STAGE_COUNT: u32 = 9;

/// 最后一道闸的秩（封印只接受站在这里的账）。
pub const STAGE_LAST_RANK: u32 = 9;

/// 账本里统计格的个数（红档格与缺陷格之外）。
pub const STAT_CELLS: usize = 7;

/// 红档率标定值：14 格里恰好中 13 格时的千分率 `(13*1000 + 14/2) / 14`。
pub const RED_RATE: u32 = 929;

/// 全绿千分率。
pub const RATE_FULL: u32 = 1000;

// ---------------------------------------------------------------------------
// errno / 退出码（POSIX、Linux、coreutils、busybox 真值）
// ---------------------------------------------------------------------------

/// 设备无空间（strerror: `No space left on device`）。
pub const ERR_ENOSPC: i32 = 28;
/// 提交期设备错（strerror: `Input/output error`）。
pub const ERR_EIO: i32 = 5;
/// 目标不可写（strerror: `Permission denied`）。
pub const ERR_EACCES: i32 = 13;
/// 路径上没这个件（strerror: `No such file or directory`）。
pub const ERR_ENOENT: i32 = 2;
/// 配额超限（strerror: `Quota exceeded`）——邻居，不许冒充 ENOSPC。
pub const ERR_EDQUOT: i32 = 122;
/// 成功时 errno 应停放的零值。
pub const ERR_NONE: i32 = 0;

/// 合法 errno 观测集合的大小（0/28/5/13/2/122 六个）。
pub const ERRNO_SET_SIZE: usize = 6;

/// 全部落定，正常收尾。
pub const EXIT_OK: u32 = 0;
/// 文件类错误：写失败、读失败、打不开。
pub const EXIT_FILE: u32 = 1;
/// 用法错误：命令行本身不合法，一个字节都没碰。
pub const EXIT_USAGE: u32 = 2;
/// 本卡不裁定的占位码。它**不是** dd 的任何真实退出码（0/1/2/128+sig），
/// 故意取一个撞不上的值，越界行就不可能被误读成正常行。
pub const EXIT_UNJUDGED: u32 = 99;

/// 退出码档位数量（0/1/2 三档真实码 + 1 档不裁定）。
pub const EXIT_RANKS: usize = 4;

/// dd 单次块大小的合法上界（1 GiB；超过它本卡认为形状非法，不参与判据）。
pub const BLOCK_MAX: u32 = 0x4000_0000;

/// 读写字节数的合法上界。
pub const BYTES_MAX: u32 = 0x0800_0000;

// ---------------------------------------------------------------------------
// conv= 位（本卡只认三种，另两种点名交给 F11697）
// ---------------------------------------------------------------------------

/// `conv=notrunc`：不截断目的件（改变可用空间的算法）。
pub const CONV_NOTRUNC: u8 = 0x01;
/// `conv=fsync`：收尾前显式 fsync——延迟分配的真相在这一格才爆。
pub const CONV_FSYNC: u8 = 0x02;
/// `conv=excl`：O_EXCL 打开目的件。
pub const CONV_EXCL: u8 = 0x04;
/// `conv=noerror`：出错续传并计 skipped——**归 UNX-F11697**，本卡拒绝裁定。
pub const CONV_NOERROR: u8 = 0x08;
/// `conv=sync`：补齐残块再写——同样改变读账语义，**归 UNX-F11697**。
pub const CONV_SYNC: u8 = 0x10;

/// 本卡认得的 conv 位全集。
pub const CONV_KNOWN: u8 = CONV_NOTRUNC | CONV_FSYNC | CONV_EXCL | CONV_NOERROR | CONV_SYNC;

/// 越界（不裁定）的 conv 位集合。
pub const CONV_OUTSCOPE: u8 = CONV_NOERROR | CONV_SYNC;

/// 位个数（语料与谓词按位遍历用）。
pub const CONV_BIT_COUNT: usize = 5;

/// conv 位表（顺序与 `CONV_*` 值一致）。
pub const CONV_BITS: [u8; CONV_BIT_COUNT] =
    [CONV_NOTRUNC, CONV_FSYNC, CONV_EXCL, CONV_NOERROR, CONV_SYNC];

// ---------------------------------------------------------------------------
// 错误码（13 个，SEG_BASE + 0x01..0x0d）
// ---------------------------------------------------------------------------

/// 01 阶段申报不符：中止落在哪道闸说错了（写闸死却说成汇总闸）。
pub const DD_ESTAGE: u32 = SEG_BASE + 0x01;
/// 02 退出码申报不符：红了却报 0，或用法错报成文件错。
pub const DD_ECODE: u32 = SEG_BASE + 0x02;
/// 03 落盘字节申报不符：把请求字节当落盘字节，半块账就地错位。
pub const DD_EVALUE: u32 = SEG_BASE + 0x03;
/// 04 注解申报不符：GNU 8.x / 9.x / busybox 三式文案混认。
pub const DD_ENOTE: u32 = SEG_BASE + 0x04;
/// 05 锚位申报不符：中止锚该亮不亮、不该亮却亮。
pub const DD_EDEFAULT: u32 = SEG_BASE + 0x05;
/// 06 半块账不符：records 申报数与块几何算出的数对不上。
pub const DD_EPARTIAL: u32 = SEG_BASE + 0x06;
/// 07 状态违例：成败互报、闸门封印后再动、账本冻结后再登记。
pub const DD_ESTATE: u32 = SEG_BASE + 0x07;
/// 08 计数不符：报表不满、矩阵不平、跳过数混进本卡账。
pub const DD_ECOUNT: u32 = SEG_BASE + 0x08;
/// 09 槽位非法：注入槽越界，或账本登记时 slot != len。
pub const DD_ESLOT: u32 = SEG_BASE + 0x09;
/// 10 指纹不符：冻结后重放对不上，或篡改留痕。
pub const DD_ESEAL: u32 = SEG_BASE + 0x0a;
/// 11 无观测：槽号超成语料、期望表缺格。
pub const DD_ENOBS: u32 = SEG_BASE + 0x0b;
/// 12 矩阵不平：档位覆盖出现空洞或行列对不上。
pub const DD_EMATRIX: u32 = SEG_BASE + 0x0c;
/// 13 账本不平：格数、成败比或对账表任一失衡。
pub const DD_EBAL: u32 = SEG_BASE + 0x0d;

/// 13 个错误码的有序全集（检查器与账本共用）。
pub const ALL_CODES: [u32; 13] = [
    DD_ESTAGE,
    DD_ECODE,
    DD_EVALUE,
    DD_ENOTE,
    DD_EDEFAULT,
    DD_EPARTIAL,
    DD_ESTATE,
    DD_ECOUNT,
    DD_ESLOT,
    DD_ESEAL,
    DD_ENOBS,
    DD_EMATRIX,
    DD_EBAL,
];

// ---------------------------------------------------------------------------
// 操作码（账本格的操作归类）
// ---------------------------------------------------------------------------

/// 命令行/conv 闸。
pub const OP_ARGS: u8 = 0x01;
/// 打开闸。
pub const OP_OPEN: u8 = 0x02;
/// 读侧循环（含 skip/count）。
pub const OP_READ: u8 = 0x03;
/// 写侧循环（含 seek/半块）。
pub const OP_WRITE: u8 = 0x04;
/// 对账闸（records/bytes）。
pub const OP_RECORDS: u8 = 0x05;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x06;
/// 红档率标定。
pub const OP_RATE: u8 = 0x07;
/// 缺陷码格。
pub const OP_DEFECT: u8 = 0x08;
/// 账本重放。
pub const OP_REPLAY: u8 = 0x09;

/// 操作码个数。
pub const OP_COUNT: usize = 9;

// ---------------------------------------------------------------------------
// FNV-1a 指纹
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
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

/// 折入一个字（大端序逐字节，保证高位差异也换指纹）。
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

/// 折入一个有符号数（errno 用）。
fn fold_i32(h: u64, v: i32) -> u64 {
    fold_word(h, v as i64 as u64)
}

// ---------------------------------------------------------------------------
// 数字/布尔文本
// ---------------------------------------------------------------------------

/// 十进制文本（u32，不递归、不格式化）。
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

/// u32 文本（对外）。
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

/// 段位文本（诊断用，定宽 5 位十六进制）。
pub fn seg_text(v: u32) -> String {
    hex_text(v, 5)
}

/// `N+M records in|out` 里的 `N+M` 文本。
pub fn pair_text(full: u32, part: u32) -> String {
    let mut s = dec_text(full);
    s.push('+');
    s.push_str(dec_text(part).as_str());
    s
}

// ---------------------------------------------------------------------------
// strerror 与文案家族（GNU 8.x / GNU 9.x / busybox 分表，逐字）
// ---------------------------------------------------------------------------

/// strerror(errno) 的 glibc 原文（本卡涉及的六个值）。
pub fn strerror(code: i32) -> &'static str {
    match code {
        ERR_ENOSPC => "No space left on device",
        ERR_EIO => "Input/output error",
        ERR_EACCES => "Permission denied",
        ERR_ENOENT => "No such file or directory",
        ERR_EDQUOT => "Quota exceeded",
        ERR_NONE => "",
        _ => "Unknown error",
    }
}

/// errno 的符号名（本卡只在审计行里用，不参与判据）。
pub fn errno_name(code: i32) -> &'static str {
    match code {
        ERR_ENOSPC => "ENOSPC",
        ERR_EIO => "EIO",
        ERR_EACCES => "EACCES",
        ERR_ENOENT => "ENOENT",
        ERR_EDQUOT => "EDQUOT",
        ERR_NONE => "0",
        _ => "?",
    }
}

/// GNU coreutils 8.x 的写失败原文：`dd: writing 'X': <strerror>`。
pub fn msg_gnu8_writing(path: &str, code: i32) -> String {
    let mut s = String::from("dd: writing '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU coreutils 9.x 的写失败原文：`dd: error writing 'X': <strerror>`。
pub fn msg_gnu9_error_writing(path: &str, code: i32) -> String {
    let mut s = String::from("dd: error writing '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// busybox dd 的写失败原文：`dd: write error to 'X': <strerror>`。
pub fn msg_bb_write_error(path: &str, code: i32) -> String {
    let mut s = String::from("dd: write error to '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU 的读失败原文（8.x 无 error 前缀，9.x 有）。
pub fn msg_gnu_reading(path: &str, code: i32, modern: bool) -> String {
    let mut s = String::from("dd: ");
    if modern {
        s.push_str("error reading '");
    } else {
        s.push_str("reading '");
    }
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU/busybox 共同的打开失败原文：`dd: failed to open 'X': <strerror>`。
pub fn msg_failed_to_open(path: &str, code: i32) -> String {
    let mut s = String::from("dd: failed to open '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// `conv=fsync` 提交期才爆的原文：`dd: fsync failed for 'X': <strerror>`。
pub fn msg_fsync_failed(path: &str, code: i32) -> String {
    let mut s = String::from("dd: fsync failed for '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// `seek=` 的偏移算不动时 GNU 的原文：`dd: 'X': cannot seek: Invalid argument`。
///
/// 这里的 `Invalid argument` 是 lseek 的 EINVAL 文案，字面写死：本卡的 errno
/// 合法集（`ERRNO_SET_SIZE`）只收"空间族与邻居族"六个值，EINVAL 只在 seek 闸
/// 出现，不进 errno 判据，免得把一个纯文本常量撑成一维。
pub fn msg_cannot_seek(path: &str) -> String {
    let mut s = String::from("dd: '");
    s.push_str(path);
    s.push_str("': cannot seek: Invalid argument");
    s
}

/// 用法错原文（操作数不recognised）。
pub fn msg_unrecognized_operand(operand: &str) -> String {
    let mut s = String::from("dd: unrecognized operand '");
    s.push_str(operand);
    s.push('\'');
    s
}

/// 选项不合法的原文（getopt 句式）。
pub fn msg_invalid_option(letter: char) -> String {
    let mut s = String::from("dd: invalid option -- '");
    s.push(letter);
    s.push('\'');
    s
}

/// 汇总第一行：`N+M records in`。
pub fn msg_records_in(full: u32, part: u32) -> String {
    let mut s = pair_text(full, part);
    s.push_str(" records in");
    s
}

/// 汇总第二行：`N+M records out`。
pub fn msg_records_out(full: u32, part: u32) -> String {
    let mut s = pair_text(full, part);
    s.push_str(" records out");
    s
}

/// 字节行的 SI/IEC 双括号部分（coreutils 现代句式）。
///
/// 表只覆盖语料用到的值；未收录的字节数返回 `n/a`，本卡的判据量是**字节数
/// 本身**，人类可读尾巴不进判据（见 `human_tail_judged`）。
pub fn human_pair(bytes: u32) -> &'static str {
    match bytes {
        0 => "0 B",
        512 => "512 B",
        1024 => "1.0 kB, 1.0 KiB",
        1536 => "1.6 kB, 1.5 KiB",
        2048 => "2.0 kB, 2.0 KiB",
        3072 => "3.1 kB, 3.0 KiB",
        4096 => "4.1 kB, 4.0 KiB",
        6144 => "6.1 kB, 6.0 KiB",
        8192 => "8.2 kB, 8.0 KiB",
        _ => "n/a",
    }
}

/// 字节总计行：`N bytes (H) copied`。计时与速率尾巴**不进判据**，本函数不产出。
pub fn msg_bytes_copied(bytes: u32) -> String {
    let mut s = dec_text(bytes);
    s.push_str(" bytes (");
    s.push_str(human_pair(bytes));
    s.push_str(") copied");
    s
}

/// partial-block 标记行的审计文本（本卡自研，用来把 `+1` 说成一个可判的短语）。
pub fn msg_partial_marker(out_full: u32, out_part: u32, tail: u32) -> String {
    let mut s = String::from("partial block: records out ");
    s.push_str(pair_text(out_full, out_part).as_str());
    s.push_str(", tail bytes ");
    s.push_str(dec_text(tail).as_str());
    s
}

/// 半块账对不上的审计文本：申报数与几何数逐格比较后的结论。
pub fn msg_records_lie(want: &str, got: &str) -> String {
    let mut s = String::from("dd records lie: log says ");
    s.push_str(got);
    s.push_str(" but geometry gives ");
    s.push_str(want);
    s
}

/// errno 与写失败不成对的审计文本（与 F11681 的总锚同族）。
///
/// `ret` 用 `i32`：dd 的 write 返回 `-1` 才是失败，把 `-1` 打印成 `4294967295`
/// 会让判据文案失去取证价值（F11681 的 write==-1 语言要求这里是负数）。
pub fn msg_pair_broken(ret: i32, errno: i32) -> String {
    let mut s = String::from("dd write/errno pair broken (ret=");
    s.push_str(i32_text(ret).as_str());
    s.push_str(", errno=");
    s.push_str(i32_text(errno).as_str());
    s.push(')');
    s
}

/// 越界 conv 的审计文本：点名归属卡，不静默。
pub fn msg_out_of_scope(operand: &str) -> String {
    let mut s = String::from("dd conv out of scope: ");
    s.push_str(operand);
    s.push_str(" belongs to UNX-F11697");
    s
}

/// `status=none` 的审计文本：汇总缺席时本卡拒绝给锚。
pub fn msg_summary_suppressed() -> String {
    String::from("dd status=none: records summary suppressed, anchor withheld")
}

/// 字节行缺席（busybox）时的复原声明文本。
pub fn msg_bytes_recovered(path: &str, bytes: u32) -> String {
    let mut s = String::from("dd byte line absent for '");
    s.push_str(path);
    s.push_str("': recovered ");
    s.push_str(dec_text(bytes).as_str());
    s.push_str(" bytes from block geometry");
    s
}

/// 人类可读尾巴是否进入判据——**假**。判据只认字节数与 records 对。
pub fn human_tail_judged() -> bool {
    false
}

// ---------------------------------------------------------------------------
// 枚举：工具家族
// ---------------------------------------------------------------------------

/// dd 的实现家族。三式的文案与汇总形状都不同，必须分表判。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    /// GNU coreutils 8.x：`dd: writing 'X': ...`，有字节行。
    Gnu8,
    /// GNU coreutils 9.x：`dd: error writing 'X': ...`，有字节行。
    Gnu9,
    /// busybox dd：`dd: write error to 'X': ...`，**无字节行**。
    Busybox,
}

impl Flavor {
    /// 全集（遍历与唯一性检查用）。
    pub const ALL: [Flavor; 3] = [Flavor::Gnu8, Flavor::Gnu9, Flavor::Busybox];

    /// 短名。
    pub fn name(&self) -> &'static str {
        match self {
            Flavor::Gnu8 => "gnu8",
            Flavor::Gnu9 => "gnu9",
            Flavor::Busybox => "busybox",
        }
    }

    /// 程序名前缀（三式都是 `dd: `，本卡把它钉在一处以免各文案函数漂移）。
    pub fn prefix(&self) -> &'static str {
        "dd: "
    }

    /// 是否 GNU 家族（8.x 与 9.x 同族异措辞）。
    pub fn gnu(&self) -> bool {
        match self {
            Flavor::Gnu8 | Flavor::Gnu9 => true,
            Flavor::Busybox => false,
        }
    }

    /// 是否 GNU 9.x 起的 `error writing` 措辞。
    pub fn modern(&self) -> bool {
        match self {
            Flavor::Gnu9 => true,
            Flavor::Gnu8 | Flavor::Busybox => false,
        }
    }

    /// 是否打印 `N bytes (...) copied` 字节行。busybox 不打——本卡必须自己复原。
    pub fn prints_byte_line(&self) -> bool {
        match self {
            Flavor::Gnu8 | Flavor::Gnu9 => true,
            Flavor::Busybox => false,
        }
    }

    /// 该家族的写失败原文。
    pub fn write_error_text(&self, path: &str, code: i32) -> String {
        match self {
            Flavor::Gnu8 => msg_gnu8_writing(path, code),
            Flavor::Gnu9 => msg_gnu9_error_writing(path, code),
            Flavor::Busybox => msg_bb_write_error(path, code),
        }
    }

    /// 下标（矩阵与遍历用）。
    pub fn index(&self) -> usize {
        match self {
            Flavor::Gnu8 => 0,
            Flavor::Gnu9 => 1,
            Flavor::Busybox => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// 枚举：闸门阶段（秩从 1 起，只能前进到下一秩）
// ---------------------------------------------------------------------------

/// dd 一行的判定终点（九道闸）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 秩 1：命令行与 conv 闸。
    Args,
    /// 秩 2：目的件打开/创建闸。
    Open,
    /// 秩 3：`seek=` 的 lseek 闸。
    Seek,
    /// 秩 4：读侧循环（含 `skip=`/`count=`）。
    Read,
    /// 秩 5：写侧循环（半块在这里发生）。
    Write,
    /// 秩 6：records 对账闸。
    Records,
    /// 秩 7：汇总存在性闸（`status=none`）。
    Summary,
    /// 秩 8：中止结算闸（落锚处）。
    Abort,
    /// 秩 9：正常收尾。
    Exit,
}

impl Stage {
    /// 全集（按秩升序）。
    pub fn all() -> [Stage; STAGE_COUNT as usize] {
        [
            Stage::Args,
            Stage::Open,
            Stage::Seek,
            Stage::Read,
            Stage::Write,
            Stage::Records,
            Stage::Summary,
            Stage::Abort,
            Stage::Exit,
        ]
    }

    /// 秩：从 **1** 起算，0 表示闸门未起跑。
    pub fn rank(&self) -> u32 {
        match self {
            Stage::Args => 1,
            Stage::Open => 2,
            Stage::Seek => 3,
            Stage::Read => 4,
            Stage::Write => 5,
            Stage::Records => 6,
            Stage::Summary => 7,
            Stage::Abort => 8,
            Stage::Exit => 9,
        }
    }

    /// 由秩反查（秩 0 与秩 >9 都查不出来）。
    pub fn from_rank(r: u32) -> Option<Stage> {
        match r {
            1 => Some(Stage::Args),
            2 => Some(Stage::Open),
            3 => Some(Stage::Seek),
            4 => Some(Stage::Read),
            5 => Some(Stage::Write),
            6 => Some(Stage::Records),
            7 => Some(Stage::Summary),
            8 => Some(Stage::Abort),
            9 => Some(Stage::Exit),
            _ => None,
        }
    }

    /// 短名。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Args => "args",
            Stage::Open => "open",
            Stage::Seek => "seek",
            Stage::Read => "read",
            Stage::Write => "write",
            Stage::Records => "records",
            Stage::Summary => "summary",
            Stage::Abort => "abort",
            Stage::Exit => "exit",
        }
    }

    /// 是否失败终点（Exit 之外都不算全绿收尾）。
    pub fn failure(&self) -> bool {
        match self {
            Stage::Exit => false,
            _ => true,
        }
    }

    /// 是否在循环之前（这三闸没有任何块账可报，留档字节必须为 0）。
    pub fn pre_loop(&self) -> bool {
        match self {
            Stage::Args | Stage::Open | Stage::Seek => true,
            _ => false,
        }
    }

    /// 块账是否可能非零（秩 ≥4 才有读侧循环的产物）。
    pub fn has_ledger(&self) -> bool {
        self.rank() >= Stage::Read.rank()
    }

    /// 是否空间族闸门（write/records/summary/abort 四段的红才是 ENOSPC 候选）。
    pub fn space_family(&self) -> bool {
        match self {
            Stage::Write | Stage::Records | Stage::Summary | Stage::Abort => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 枚举：注解（文案家族 × 事件形态）
// ---------------------------------------------------------------------------

/// 一行的形态注解。家族与事件折在同一维里，是**本卡的设计决定**：
/// 判据要能抓住"GNU 现场报成 busybox 句式"这种混认，所以家族必须是可判维，
/// 而不是藏在文案函数里。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 全绿收尾，records 齐全，最后一块是整块。
    Done,
    /// 全绿收尾，最后一块是残块（`ibs != obs` 或输入本来就短）。
    DonePartial,
    /// 全绿收尾，`count=` 截断了读侧。
    DoneTrunc,
    /// 汇总被 `status=none` 压掉（成败都可能）。
    Silent,
    /// 写失败，GNU 8.x 句式（`writing 'X'`）。
    AbortGnu8,
    /// 写失败，GNU 9.x 句式（`error writing 'X'`）。
    AbortGnu9,
    /// 写失败，busybox 句式（`write error to 'X'`，无字节行）。
    AbortBb,
    /// 写当场全绿，`conv=fsync` 提交时才爆。
    FsyncGnu,
    /// 读侧红（EIO 等），不是空间问题。
    ReadErr,
    /// 目的件没打开成（`failed to open`）。
    OpenErr,
    /// `seek=` 的偏移算不动。
    SeekErr,
    /// 用法错，退 2，一字节未碰。
    Usage,
    /// `conv=noerror`/`sync`：本卡不裁定，点名交给 F11697。
    OutScope,
    /// 日志申报的 records/字节与块几何对不上。
    RecordsLie,
    /// 写失败但 errno 不是 28：与 F11681 的总锚不成对。
    PairBroken,
}

impl Note {
    /// 全集。
    pub fn all() -> [Note; 15] {
        [
            Note::Done,
            Note::DonePartial,
            Note::DoneTrunc,
            Note::Silent,
            Note::AbortGnu8,
            Note::AbortGnu9,
            Note::AbortBb,
            Note::FsyncGnu,
            Note::ReadErr,
            Note::OpenErr,
            Note::SeekErr,
            Note::Usage,
            Note::OutScope,
            Note::RecordsLie,
            Note::PairBroken,
        ]
    }

    /// 短名（进释义行）。
    pub fn name(&self) -> &'static str {
        match self {
            Note::Done => "done",
            Note::DonePartial => "done-partial",
            Note::DoneTrunc => "done-trunc",
            Note::Silent => "summary-none",
            Note::AbortGnu8 => "abort-gnu8",
            Note::AbortGnu9 => "abort-gnu9",
            Note::AbortBb => "abort-busybox",
            Note::FsyncGnu => "fsync-gnu",
            Note::ReadErr => "read-error",
            Note::OpenErr => "open-error",
            Note::SeekErr => "seek-error",
            Note::Usage => "usage",
            Note::OutScope => "out-of-scope",
            Note::RecordsLie => "records-lie",
            Note::PairBroken => "pair-broken",
        }
    }

    /// 是否成功态注解。`Silent` 说"汇总缺席"，它本身不是失败——失败与否
    /// 由退出码决定，这正是 `status=none` 难判的原因。
    pub fn success(&self) -> bool {
        match self {
            Note::Done | Note::DonePartial | Note::DoneTrunc | Note::Silent => true,
            _ => false,
        }
    }

    /// 是否 dd 面的 ENOSPC 中止家族。
    pub fn abort_family(&self) -> bool {
        match self {
            Note::AbortGnu8 | Note::AbortGnu9 | Note::AbortBb | Note::FsyncGnu => true,
            _ => false,
        }
    }

    /// 是否邻居（红但不是 ENOSPC 中止：读错/打开错/偏移错/errno 串档）。
    pub fn foreign(&self) -> bool {
        match self {
            Note::ReadErr | Note::OpenErr | Note::SeekErr | Note::PairBroken => true,
            _ => false,
        }
    }

    /// 是否用法错。
    pub fn usage(&self) -> bool {
        match self {
            Note::Usage => true,
            _ => false,
        }
    }

    /// 是否越界（不裁定）。
    pub fn out_of_scope(&self) -> bool {
        match self {
            Note::OutScope => true,
            _ => false,
        }
    }

    /// 是否半块账谎报。
    pub fn lie(&self) -> bool {
        match self {
            Note::RecordsLie => true,
            _ => false,
        }
    }

    /// 是否 GNU 家族的文案。
    pub fn gnu_text(&self) -> bool {
        match self {
            Note::AbortGnu8 | Note::AbortGnu9 | Note::FsyncGnu => true,
            _ => false,
        }
    }

    /// 该注解是否需要一条 partial-block 标记才算自洽。
    ///
    /// 只有"死在块中间"的两种形态需要（残块中止 / fsync 前写全绿不需要标记），
    /// 整块失败**没有** partial 标记——把它也算成半程是本卡的 Value 维缺陷。
    pub fn wants_partial_marker(&self) -> bool {
        match self {
            Note::AbortGnu8 | Note::AbortGnu9 | Note::AbortBb => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 枚举：档位
// ---------------------------------------------------------------------------

/// 结果档位。四档是本卡的覆盖骨架：**完整**（绿且可对账）、**无汇总**
/// （绿但无从对账）、**中止**（红且落锚）、**拒绝**（本卡不收的一切）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 成功且汇总在场。
    Complete,
    /// 成功但 `status=none` 压掉了汇总。
    Silent,
    /// ENOSPC 中止且半块账自洽（锚位亮）。
    Aborted,
    /// 本卡拒绝受理：用法错、越界 conv、邻居 errno、半块账谎报、无汇总中止。
    Refused,
}

impl Tier {
    /// 由结果投影。
    ///
    /// 分档只看**实际结果**，不看申报；申报正确与否另计（见 `Matrix`）。
    /// 锚位是唯一入口：亮锚才算 Aborted，红而不亮锚一律 Refused——把"红了但
    /// 对不上账"的行放进 Aborted，就等于允许日志少一行也能过本卡。
    pub fn of(o: &Outcome) -> Tier {
        if o.ok() {
            if o.stage == Stage::Summary {
                return Tier::Silent;
            }
            return Tier::Complete;
        }
        if o.default {
            return Tier::Aborted;
        }
        Tier::Refused
    }

    /// 次序。
    pub fn index(&self) -> usize {
        match self {
            Tier::Complete => 0,
            Tier::Silent => 1,
            Tier::Aborted => 2,
            Tier::Refused => 3,
        }
    }

    /// 反查。
    pub fn from_index(i: usize) -> Option<Tier> {
        match i {
            0 => Some(Tier::Complete),
            1 => Some(Tier::Silent),
            2 => Some(Tier::Aborted),
            3 => Some(Tier::Refused),
            _ => None,
        }
    }

    /// 短名。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Complete => "complete",
            Tier::Silent => "silent",
            Tier::Aborted => "aborted",
            Tier::Refused => "refused",
        }
    }

    /// 是否成功档。
    pub fn success(&self) -> bool {
        match self {
            Tier::Complete | Tier::Silent => true,
            Tier::Aborted | Tier::Refused => false,
        }
    }

    /// 是否本卡受理的中止档。
    pub fn anchored(&self) -> bool {
        match self {
            Tier::Aborted => true,
            _ => false,
        }
    }

    /// 汇总在场是否是这一档的前提。
    pub fn needs_summary(&self) -> bool {
        match self {
            Tier::Complete | Tier::Aborted => true,
            Tier::Silent | Tier::Refused => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 枚举：缺陷
// ---------------------------------------------------------------------------

/// 申报缺陷（判据的输出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 六维全一致。
    Ok,
    /// 阶段错：中止落在哪道闸说反了。
    Stage,
    /// 退出码错：红了报 0，或用法错报成文件错。
    Code,
    /// 落盘字节错：把请求字节当落盘字节。
    Value,
    /// 注解错：三式文案混认。
    Note,
    /// 锚位错：该落锚不落，不该落却落。
    Default,
    /// 半块账错：records 与几何对不上（结构性缺陷，不进六维判序）。
    Partial,
    /// 成败互报（优先于其余五维）。
    Estate,
    /// 计数错：报表不满、注入后仍报全绿。
    Count,
    /// 槽位非法。
    Slot,
    /// 指纹不符。
    Seal,
    /// 无观测。
    Nobs,
    /// 矩阵不平。
    Matrix,
    /// 账本不平。
    Bal,
}

impl Fault {
    /// 全集（13 个非零码 + Ok）。
    pub fn all() -> [Fault; 14] {
        [
            Fault::Ok,
            Fault::Stage,
            Fault::Code,
            Fault::Value,
            Fault::Note,
            Fault::Default,
            Fault::Partial,
            Fault::Estate,
            Fault::Count,
            Fault::Slot,
            Fault::Seal,
            Fault::Nobs,
            Fault::Matrix,
            Fault::Bal,
        ]
    }

    /// 错误码。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Stage => DD_ESTAGE,
            Fault::Code => DD_ECODE,
            Fault::Value => DD_EVALUE,
            Fault::Note => DD_ENOTE,
            Fault::Default => DD_EDEFAULT,
            Fault::Partial => DD_EPARTIAL,
            Fault::Estate => DD_ESTATE,
            Fault::Count => DD_ECOUNT,
            Fault::Slot => DD_ESLOT,
            Fault::Seal => DD_ESEAL,
            Fault::Nobs => DD_ENOBS,
            Fault::Matrix => DD_EMATRIX,
            Fault::Bal => DD_EBAL,
        }
    }

    /// 是否非零。
    pub fn alive(&self) -> bool {
        match self {
            Fault::Ok => false,
            _ => true,
        }
    }

    /// 是否六维判序会产出的缺陷（Partial/Count/… 是结构维，判序不产出）。
    pub fn judged(&self) -> bool {
        match self {
            Fault::Estate
            | Fault::Stage
            | Fault::Code
            | Fault::Value
            | Fault::Note
            | Fault::Default => true,
            _ => false,
        }
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
        Fault::Partial => "partial",
        Fault::Estate => "estate",
        Fault::Count => "count",
        Fault::Slot => "slot",
        Fault::Seal => "seal",
        Fault::Nobs => "nobs",
        Fault::Matrix => "matrix",
        Fault::Bal => "bal",
    }
}

/// 判序的六维顺序（语料的六个死槽一一对应）。
pub const JUDGE_ORDER: [Fault; REJECT] =
    [Fault::Estate, Fault::Stage, Fault::Code, Fault::Value, Fault::Note, Fault::Default];

// ---------------------------------------------------------------------------
// 设备几何（块账的载体）
// ---------------------------------------------------------------------------

/// 一块被测目的文件系统的几何。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevGeom {
    /// 挂载点/设备名。
    pub mount: &'static str,
    /// 文件系统块大小（字节）。
    pub bsize: u32,
    /// 空闲块数。
    pub blocks_free: u32,
    /// 是否延迟分配（写当场绿、提交才爆）。
    pub delalloc: bool,
    /// 目的件已有的字节数（`conv=notrunc` 时这些字节不占新额度）。
    pub dst_existing: u32,
}

impl DevGeom {
    /// 空白几何。
    pub const fn blank() -> DevGeom {
        DevGeom {
            mount: "",
            bsize: 0,
            blocks_free: 0,
            delalloc: false,
            dst_existing: 0,
        }
    }

    /// 空闲字节数（乘法溢出视为坏几何）。
    pub fn bytes_free(&self) -> Option<u32> {
        self.bsize.checked_mul(self.blocks_free)
    }

    /// 是否结构良好：块大小在 {512,1024,4096}，空闲字节可算且不超上界。
    pub fn well_formed(&self) -> bool {
        if self.mount.is_empty() {
            return false;
        }
        if self.bsize != 512 && self.bsize != 1024 && self.bsize != 4096 {
            return false;
        }
        match self.bytes_free() {
            Some(v) => v <= BYTES_MAX,
            None => false,
        }
    }

    /// 是否满到零块。
    pub fn is_full(&self) -> bool {
        self.blocks_free == 0
    }

    /// 打开目的件后可写的字节数：截断（缺省）把已有字节全部还回空间，
    /// `dst_existing` 只在 `conv=notrunc` 时才继续占着。
    pub fn room_after_truncate(&self) -> Option<u32> {
        self.bytes_free()
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.mount);
        h = fold_word(h, self.bsize as u64);
        h = fold_word(h, self.blocks_free as u64);
        h = fold_word(h, self.delalloc as u64);
        fold_word(h, self.dst_existing as u64)
    }
}

/// 被测设备表：即时型满、即时型剩 6144 字节（半块来源）、延迟分配型满。
pub fn geoms() -> [DevGeom; 3] {
    [
        DevGeom {
            mount: "/mnt/full",
            bsize: 4096,
            blocks_free: 0,
            delalloc: false,
            dst_existing: 0,
        },
        DevGeom {
            mount: "/mnt/almost",
            bsize: 512,
            blocks_free: 12,
            delalloc: false,
            dst_existing: 0,
        },
        DevGeom {
            mount: "/mnt/delalloc",
            bsize: 1024,
            blocks_free: 0,
            delalloc: true,
            dst_existing: 2048,
        },
    ]
}

/// 第 i 块设备的可用字节（越界返回 None，不做静默回绕）。
pub fn geom_room(i: usize) -> Option<u32> {
    match geoms().get(i) {
        Some(g) => g.room_after_truncate(),
        None => None,
    }
}

// ---------------------------------------------------------------------------
// 形状（本卡输入的全部维度）
// ---------------------------------------------------------------------------

/// 一次 dd 观测的形状：命令行几何 + 设备余量 + 日志申报的三对数。
///
/// `logged_*` 是**日志里读到的**申报，`calc` 是**几何算出的**真相；本卡的
/// `records` 闸就是把这两份逐格对上。缺任何一份都不算对上（busybox 没有
/// 字节行，故该格按家族规则豁免，见 `records_agree`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// 目的件操作数名。
    pub path: &'static str,
    /// 触发的 operand（用法错与越界 conv 的取证串；正常行为空串）。
    pub operand: &'static str,
    /// 实现家族。
    pub flavor: Flavor,
    /// `bs=`（同时定读写块；0 表示未给）。
    pub bs: u32,
    /// `ibs=`（0 表示未给，回落到 bs）。
    pub ibs: u32,
    /// `obs=`（0 表示未给，回落到 bs）。
    pub obs: u32,
    /// 输入件实有字节数。
    pub src_bytes: u32,
    /// 打开目的件之后目的文件系统可写字节数。
    pub free_bytes: u32,
    /// `count=`（输入块数，0 表示不限）。
    pub count: u32,
    /// `skip=`（跳过输入块数）。
    pub skip: u32,
    /// `seek=`（目的件里跳过的输出块数）。
    pub seek: u32,
    /// `conv=` 位集合。
    pub conv: u8,
    /// `status=none`：汇总被压掉。
    pub status_none: bool,
    /// 现场观测到的 errno（与 F11681 总锚对照用；0 表示没红）。
    pub errno_seen: i32,
    /// 命令行本身不合法（未知 operand / 选项）。
    pub bad_option: bool,
    /// 目的件没打开成。
    pub open_fail: bool,
    /// 读侧红（EIO 一类）。
    pub read_fail: bool,
    /// `conv=fsync` 提交时才爆。
    pub commit_fail: bool,
    /// 日志申报的 records in 整块数。
    pub logged_in_full: u32,
    /// 日志申报的 records in 残块数（0/1）。
    pub logged_in_part: u32,
    /// 日志申报的 records out 整块数。
    pub logged_out_full: u32,
    /// 日志申报的 records out 残块数（0/1，partial-block 标记）。
    pub logged_out_part: u32,
    /// 日志申报的字节总数。
    pub logged_bytes: u32,
    /// 日志是否给了字节行（busybox 恒为 false）。
    pub logged_bytes_known: bool,
}

impl Shape {
    /// 默认形状：bs=512、读写各一、干净现场。
    pub const fn blank() -> Shape {
        Shape {
            tag: "",
            path: "disk.img",
            operand: "",
            flavor: Flavor::Gnu9,
            bs: 512,
            ibs: 0,
            obs: 0,
            src_bytes: 0,
            free_bytes: 0,
            count: 0,
            skip: 0,
            seek: 0,
            conv: 0,
            status_none: false,
            errno_seen: ERR_NONE,
            bad_option: false,
            open_fail: false,
            read_fail: false,
            commit_fail: false,
            logged_in_full: 0,
            logged_in_part: 0,
            logged_out_full: 0,
            logged_out_part: 0,
            logged_bytes: 0,
            logged_bytes_known: true,
        }
    }

    /// 生效的输入块大小。
    pub const fn eff_ibs(&self) -> u32 {
        if self.bs != 0 {
            self.bs
        } else {
            self.ibs
        }
    }

    /// 生效的输出块大小。
    pub const fn eff_obs(&self) -> u32 {
        if self.bs != 0 {
            self.bs
        } else {
            self.obs
        }
    }

    /// 是否加了某个 conv 位。
    pub const fn conv_has(&self, bit: u8) -> bool {
        self.conv & bit != 0
    }

    /// 越界（不裁定）的 conv 位。
    pub const fn out_of_scope_bits(&self) -> u8 {
        self.conv & CONV_OUTSCOPE
    }

    /// 未知的 conv 位（属于用法错，不是越界）。
    pub const fn unknown_conv_bits(&self) -> u8 {
        self.conv & !CONV_KNOWN
    }

    /// 用法是否已经坏到不必往下走。
    pub fn usage_bad(&self) -> bool {
        if self.bad_option {
            return true;
        }
        if self.unknown_conv_bits() != 0 {
            return true;
        }
        let ibs = self.eff_ibs();
        let obs = self.eff_obs();
        if ibs == 0 || obs == 0 {
            return true;
        }
        ibs > BLOCK_MAX || obs > BLOCK_MAX
    }

    /// 家族是否可判。
    pub fn flavor_legible(&self) -> bool {
        match self.flavor {
            Flavor::Gnu8 | Flavor::Gnu9 | Flavor::Busybox => true,
        }
    }

    /// 观测 errno 是否在合法集合内。
    pub fn errno_legal(&self) -> bool {
        self.errno_seen == ERR_NONE
            || self.errno_seen == ERR_ENOSPC
            || self.errno_seen == ERR_EIO
            || self.errno_seen == ERR_EACCES
            || self.errno_seen == ERR_ENOENT
            || self.errno_seen == ERR_EDQUOT
    }

    /// 申报的 records 数字本身是否成形状（残块只可能是 0/1）。
    ///
    /// 家族规则在这里生效：GNU 只要打了 records 行就必须带字节行，busybox
    /// 永远打不出字节行。完全没打汇总（用法错、`status=none`、打开失败）
    /// 不在此列——那时"缺字节行"是事实，不是谎报。
    pub fn logged_legible(&self) -> bool {
        if self.logged_in_part > 1 || self.logged_out_part > 1 {
            return false;
        }
        if self.logged_bytes > BYTES_MAX {
            return false;
        }
        let summary_logged = self.logged_in_full != 0
            || self.logged_in_part != 0
            || self.logged_out_full != 0
            || self.logged_out_part != 0;
        if !self.flavor.prints_byte_line() && self.logged_bytes_known {
            // busybox 打不出字节行：日志里有就是家族混认。
            return false;
        }
        if self.flavor.prints_byte_line() && !self.logged_bytes_known && summary_logged {
            // GNU 该有字节行却没有：同样无从对账。
            return false;
        }
        true
    }

    /// 数值维是否在本卡上界内。
    ///
    /// 块数上界取 16 Mi 块：`ibs ≤ 1 GiB` 时 `ibs × skip` 仍算得动，再大就当
    /// 成越界输入拒收（dd 会用 EINVAL 拒绝，而不是给出一个绕回的偏移）。
    pub fn magnitudes_legal(&self) -> bool {
        if self.src_bytes > BYTES_MAX || self.free_bytes > BYTES_MAX {
            return false;
        }
        if self.skip > 0x00ff_ffff || self.seek > 0x00ff_ffff {
            return false;
        }
        self.count <= 0x00ff_ffff
    }

    /// 形状是否可判。
    pub fn valid(&self) -> bool {
        if !self.errno_legal() {
            return false;
        }
        if !self.flavor_legible() {
            return false;
        }
        if !self.logged_legible() {
            return false;
        }
        if !self.magnitudes_legal() {
            return false;
        }
        if self.usage_bad() && !self.bad_option && self.unknown_conv_bits() == 0 {
            // bs/ibs/obs 归零或超界属于"形状非法"，不是 dd 的用法错样本。
            return false;
        }
        true
    }

    /// 换字节余量的副本。
    pub fn clone_free(&self, f: u32) -> Shape {
        Shape { free_bytes: f, ..*self }
    }

    /// 换输入尺寸的副本。
    pub fn clone_src(&self, n: u32) -> Shape {
        Shape { src_bytes: n, ..*self }
    }

    /// 换 count 的副本。
    pub fn clone_count(&self, c: u32) -> Shape {
        Shape { count: c, ..*self }
    }

    /// 换 conv 位的副本（叠加，不清原有位）。
    pub fn with_conv(&self, bit: u8) -> Shape {
        Shape { conv: self.conv | bit, ..*self }
    }

    /// 换家族而不换其余维的副本（三式对照实验用）。
    ///
    /// 家族换了，字节行的有无也跟着换：busybox 打不出字节行，GNU 必须打。
    /// 这一条是 `logged_legible` 的家族维，不是装饰。
    pub fn with_flavor(&self, f: Flavor) -> Shape {
        Shape {
            flavor: f,
            logged_bytes_known: f.prints_byte_line(),
            ..*self
        }
    }

    /// 指纹：对全部判定字段敏感。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_str(h, self.path);
        h = fold_str(h, self.operand);
        h = fold_str(h, self.flavor.name());
        h = fold_word(h, self.bs as u64);
        h = fold_word(h, self.ibs as u64);
        h = fold_word(h, self.obs as u64);
        h = fold_word(h, self.src_bytes as u64);
        h = fold_word(h, self.free_bytes as u64);
        h = fold_word(h, self.count as u64);
        h = fold_word(h, self.skip as u64);
        h = fold_word(h, self.seek as u64);
        h = fold_word(h, self.conv as u64);
        h = fold_word(h, self.status_none as u64);
        h = fold_i32(h, self.errno_seen);
        h = fold_word(h, self.bad_option as u64);
        h = fold_word(h, self.open_fail as u64);
        h = fold_word(h, self.read_fail as u64);
        h = fold_word(h, self.commit_fail as u64);
        h = fold_word(h, self.logged_in_full as u64);
        h = fold_word(h, self.logged_in_part as u64);
        h = fold_word(h, self.logged_out_full as u64);
        h = fold_word(h, self.logged_out_part as u64);
        h = fold_word(h, self.logged_bytes as u64);
        fold_word(h, self.logged_bytes_known as u64)
    }

    /// 走一遍九闸后的结果指纹（复刻纪律用：死槽的现场必须与母槽一模一样）。
    pub fn actual_seal(&self) -> u64 {
        define(self).seal()
    }

    /// 块账指纹（records 闸的两份陈述之一）。
    pub fn blocks_seal(&self) -> u64 {
        calc(self).seal()
    }
}

// ---------------------------------------------------------------------------
// 块账（几何算出来的真相）
// ---------------------------------------------------------------------------

/// 一次 dd 运行的块账。这是本卡**独有**的一维：cp/tee 没有块边界。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blocks {
    /// 生效输入块大小。
    pub ibs: u32,
    /// 生效输出块大小。
    pub obs: u32,
    /// 读到的整块数。
    pub in_full: u32,
    /// 读到的残块数（0/1）。
    pub in_part: u32,
    /// 读到的字节总数。
    pub in_bytes: u32,
    /// `skip=` 跨过的字节（一次都没进读账）。
    pub skipped_bytes: u32,
    /// `count=` 截掉的字节（既不在读账也不在写账）。
    pub cut_bytes: u32,
    /// `seek=` 造的洞（不吃块，所以满盘也能成）。
    pub hole_bytes: u32,
    /// 写成的整块数。
    pub out_full: u32,
    /// 写成的残块数（0/1，即 partial-block 标记）。
    pub out_part: u32,
    /// 落盘字节。
    pub landed: u32,
    /// 丢失字节（请求减去落盘）。
    pub lost: u32,
    /// 残块尾字节。
    pub tail: u32,
    /// 偏移算术溢出（进 seek 闸）。
    pub overflow: bool,
    /// 写侧是否真的失败。
    pub write_failed: bool,
}

impl Blocks {
    /// 全零账（用法错/越界行的账本就长这样）。
    pub const fn zero() -> Blocks {
        Blocks {
            ibs: 0,
            obs: 0,
            in_full: 0,
            in_part: 0,
            in_bytes: 0,
            skipped_bytes: 0,
            cut_bytes: 0,
            hole_bytes: 0,
            out_full: 0,
            out_part: 0,
            landed: 0,
            lost: 0,
            tail: 0,
            overflow: false,
            write_failed: false,
        }
    }

    /// 是否有 partial-block 标记。
    pub fn partial(&self) -> bool {
        self.out_part == 1
    }

    /// 读侧残块的字节数（`records in` 的 `+1` 背后那一口气读了多少字节）。
    pub fn tail_read(&self) -> u32 {
        self.in_bytes.saturating_sub(self.in_full.saturating_mul(self.ibs))
    }

    /// 落盘字节是否等于请求字节（写侧全绿）。
    pub fn all_landed(&self) -> bool {
        self.lost == 0
    }

    /// `N+M records in` 的申报文本。
    pub fn in_text(&self) -> String {
        msg_records_in(self.in_full, self.in_part)
    }

    /// `N+M records out` 的申报文本。
    pub fn out_text(&self) -> String {
        msg_records_out(self.out_full, self.out_part)
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.ibs as u64);
        h = fold_word(h, self.obs as u64);
        h = fold_word(h, self.in_full as u64);
        h = fold_word(h, self.in_part as u64);
        h = fold_word(h, self.in_bytes as u64);
        h = fold_word(h, self.skipped_bytes as u64);
        h = fold_word(h, self.cut_bytes as u64);
        h = fold_word(h, self.hole_bytes as u64);
        h = fold_word(h, self.out_full as u64);
        h = fold_word(h, self.out_part as u64);
        h = fold_word(h, self.landed as u64);
        h = fold_word(h, self.lost as u64);
        h = fold_word(h, self.tail as u64);
        h = fold_word(h, self.overflow as u64);
        fold_word(h, self.write_failed as u64)
    }
}

/// 块算术：把命令行几何与设备余量折算成 records 三对数。
///
/// 规则全部按 GNU dd 的**记账口径**写：
/// * `skip=N` 用 lseek 跨过 N 个输入块，被跨过的字节不进读账；
/// * `count=N` 至多读 N 个输入块，读满即停，剩下的算 `cut`（不是 lost）；
/// * 输入短于 `ibs` 的那一次读产出 `records in` 的 `+1`；
/// * 输出按 `obs` 切块，落盘字节 = min(请求, 余量)，`landed % obs > 0` 才给
///   `records out` 的 `+1`；`landed == 0` 是**整块失败**，没有 partial 标记；
/// * `seek=N` 造的洞不占余量，所以满盘也能 seek 成功、第一笔写立刻失败。
pub fn calc(s: &Shape) -> Blocks {
    let ibs = s.eff_ibs();
    let obs = s.eff_obs();
    if ibs == 0 || obs == 0 {
        let mut z = Blocks::zero();
        z.overflow = true;
        return z;
    }
    let mut b = Blocks::zero();
    b.ibs = ibs;
    b.obs = obs;

    let skip_bytes = match ibs.checked_mul(s.skip) {
        Some(v) => v,
        None => {
            b.overflow = true;
            return b;
        }
    };
    let seek_bytes = match obs.checked_mul(s.seek) {
        Some(v) => v,
        None => {
            b.overflow = true;
            return b;
        }
    };
    b.skipped_bytes = if skip_bytes > s.src_bytes { s.src_bytes } else { skip_bytes };
    b.hole_bytes = seek_bytes;

    let readable = s.src_bytes.saturating_sub(skip_bytes);
    let limit = if s.count > 0 { s.count } else { u32::MAX };
    let full_possible = readable / ibs;
    let in_full = if full_possible < limit { full_possible } else { limit };
    let consumed = match in_full.checked_mul(ibs) {
        Some(v) => v,
        None => {
            b.overflow = true;
            return b;
        }
    };
    let after = readable.saturating_sub(consumed);
    // `count=` 把读侧提前收口：这时剩下的字节是**截掉**（cut），不是残块读。
    // 只有把整块都读完还剩尾巴，那一次读才算 `N+1 records in` 的 `+1`。
    let truncated = in_full < full_possible;
    let in_part = if after > 0 && !truncated { 1 } else { 0 };
    b.in_full = in_full;
    b.in_part = in_part;
    b.in_bytes = consumed.saturating_add(if in_part == 1 { after } else { 0 });
    b.cut_bytes = if in_part == 1 { 0 } else { after };

    let want = b.in_bytes;
    let landed = if want <= s.free_bytes { want } else { s.free_bytes };
    b.landed = landed;
    b.lost = want.saturating_sub(landed);
    b.out_full = landed / obs;
    b.tail = landed % obs;
    b.out_part = if b.tail > 0 { 1 } else { 0 };
    b.write_failed = b.lost > 0;
    b
}

/// 日志申报与几何是否逐格一致（records 闸的判据）。
pub fn records_agree(s: &Shape, b: &Blocks) -> bool {
    if s.logged_in_part > 1 || s.logged_out_part > 1 {
        return false;
    }
    if s.logged_in_full != b.in_full || s.logged_in_part != b.in_part {
        return false;
    }
    if s.logged_out_full != b.out_full || s.logged_out_part != b.out_part {
        return false;
    }
    if s.flavor.prints_byte_line() {
        if !s.logged_bytes_known {
            return false;
        }
        return s.logged_bytes == b.landed;
    }
    // busybox 不打字节行：申报里出现字节就是家族混认，判死。
    !s.logged_bytes_known
}

/// 家族 → 中止注解。
pub fn family_note(f: Flavor) -> Note {
    match f {
        Flavor::Gnu8 => Note::AbortGnu8,
        Flavor::Gnu9 => Note::AbortGnu9,
        Flavor::Busybox => Note::AbortBb,
    }
}

// ---------------------------------------------------------------------------
// 结果
// ---------------------------------------------------------------------------

/// 一次 dd 判定的结果（六维）。
///
/// `code` 这里是 **dd 的退出码**（0/1/2），不是 errno——errno 面归 F11681 的
/// 总锚，本卡只把它当外部事实（`Shape::errno_seen`）读进来做一次配对。
/// 把两卡的一维混用是这族卡最容易犯的错位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 判定终点落在哪道闸。
    pub stage: Stage,
    /// dd 退出码。
    pub code: i32,
    /// 落盘字节（`status=none` 也要给得出：几何算得出来）。
    pub value: u32,
    /// 文案家族与形态。
    pub note: Note,
    /// 锚位：是否"dd 面 ENOSPC 中止 + 半块账自洽"。
    pub default: bool,
}

impl Outcome {
    /// 空白结果（未观测）。
    pub const fn blank() -> Outcome {
        Outcome {
            stage: Stage::Args,
            code: EXIT_UNJUDGED as i32,
            value: 0,
            note: Note::OutScope,
            default: false,
        }
    }

    /// 构造。
    pub const fn make(
        stage: Stage,
        code: i32,
        value: u32,
        note: Note,
        default: bool,
    ) -> Outcome {
        Outcome { stage, code, value, note, default }
    }

    /// 是否成功收尾（码 0 且注解属成功态）。
    ///
    /// `Silent` 在注解上算"成功态"（它只说汇总缺席），所以成败最终由退出码
    /// 决定——这正是 `status=none` 难判的根源，也是本卡把汇总存在性单立一道
    /// 闸的理由：`Silent` 行永远 `default=false`，不许被当成本卡的绿。
    pub fn ok(&self) -> bool {
        self.code == EXIT_OK as i32 && self.note.success()
    }

    /// 是否失败。
    pub fn failed(&self) -> bool {
        !self.ok()
    }

    /// 是否落到本卡的中止锚：码 1 + 终点在 `abort` 闸 + 文案属中止家族 + 锚位亮。
    ///
    /// 四件缺一不可。少"终点在 abort"就会把 `records` 闸的谎报行也算成锚；
    /// 少"家族"就会把 busybox 行当 GNU 行送去比对 GNU 文案。
    pub fn anchored(&self) -> bool {
        self.failed()
            && self.code == EXIT_FILE as i32
            && self.stage == Stage::Abort
            && self.note.abort_family()
            && self.default
    }

    /// 是否本卡拒绝受理（不裁定、用法错、邻居 errno、半块账对不上）。
    pub fn refused(&self) -> bool {
        if self.ok() {
            return false;
        }
        if self.default {
            return false;
        }
        true
    }

    /// 结果是否要求 partial-block 标记才算自洽（注解家族 × 块账两者与判）。
    pub fn marker_consistent(&self, b: &Blocks) -> bool {
        if !self.note.wants_partial_marker() {
            return true;
        }
        // 中止家族：有残块就要有标记，没残块就不许有标记；本卡的标记就是
        // records out 的那个 +1，所以这里比的是几何而不是日志。
        b.partial() == (b.tail > 0)
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.rank() as u64);
        h = fold_i32(h, self.code);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        fold_word(h, self.default as u64)
    }
}

// ---------------------------------------------------------------------------
// 申报
// ---------------------------------------------------------------------------

/// 一槽申报（与 `Outcome` 同形的六维，但来自日志的读法）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 申报的终点闸。
    pub stage: Stage,
    /// 申报的退出码。
    pub code: i32,
    /// 申报的落盘字节。
    pub value: u32,
    /// 申报的形态注解。
    pub note: Note,
    /// 申报的锚位。
    pub default: bool,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim {
            stage: Stage::Args,
            code: EXIT_UNJUDGED as i32,
            value: 0,
            note: Note::OutScope,
            default: false,
        }
    }

    /// 构造（语料表逐槽用）。
    pub const fn make(stage: Stage, code: i32, value: u32, note: Note, default: bool) -> Claim {
        Claim { stage, code, value, note, default }
    }

    /// 是否申报成功。
    pub fn success(&self) -> bool {
        self.code == EXIT_OK as i32 && self.note.success()
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.rank() as u64);
        h = fold_i32(h, self.code);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        fold_word(h, self.default as u64)
    }
}

/// 一槽用例：形状（现场）+ 申报（读法）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 现场形状。
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

    /// 实际结果（走一遍九闸）。
    pub fn actual(&self) -> Outcome {
        define(&self.shape)
    }

    /// 块账（释义行与 records 闸共用）。
    pub fn blocks(&self) -> Blocks {
        calc(&self.shape)
    }

    /// 对判。
    pub fn judge(&self) -> Fault {
        judge(self)
    }

    /// 是否申报一致。
    pub fn ok(&self) -> bool {
        judge(self) == Fault::Ok
    }

    /// 用例指纹（形状与申报都折进去）。
    pub fn seal(&self) -> u64 {
        let mut h = self.shape.seal();
        h = fold_word(h, self.claim.seal());
        fold_word(h, self.actual().seal())
    }
}

// ---------------------------------------------------------------------------
// 定义纵线（九闸：args → open → seek → read → pair → records → summary → abort/exit）
// ---------------------------------------------------------------------------

/// 判定一次 dd 观测的现场结果。
///
/// 每一闸只在上一闸放行后执行，顺序不可调：
/// * `args` 先判命令行——`bs=0`、未知 operand、`conv=noerror` 越界都在此出局，
///   一字节未碰，谈不上诉说落盘字节；
/// * `open` 次判目的件——打不开就没有余量这个概念；
/// * `seek` 判偏移算术——洞不吃块，所以满盘也要放行到写侧才真红；
/// * `read` 判读侧红——读红不是空间问题，落锚权在这里就没了；
/// * `pair` 判 (返回, errno) 是否与 F11681 的总锚成对——不对即串档；
/// * `records` 判日志申报与块几何是否逐格一致——它必须在写之后，先有物理
///   落点才谈得上"申报的数字对不对"；
/// * `summary` 判汇总是否在场——`status=none` 让上一闸无从对账，于是判定
///   终点前移到这里，锚位强制归零；
/// * `abort`/`exit` 结算——四件（写真的红、码 1、汇总在场、数字自洽）齐了
///   才在 `abort` 落锚并钉死文案家族，否则走 `exit`。
pub fn define(s: &Shape) -> Outcome {
    // ---- 闸 1：args ----
    if s.bad_option || s.unknown_conv_bits() != 0 || s.eff_ibs() == 0 || s.eff_obs() == 0 {
        return Outcome::make(Stage::Args, EXIT_USAGE as i32, 0, Note::Usage, false);
    }
    if s.out_of_scope_bits() != 0 {
        // conv=noerror/sync：中止轴整个换掉，点名交给 F11697，本卡不裁定。
        return Outcome::make(Stage::Args, EXIT_UNJUDGED as i32, 0, Note::OutScope, false);
    }

    // ---- 闸 2：open ----
    if s.open_fail {
        return Outcome::make(Stage::Open, EXIT_FILE as i32, 0, Note::OpenErr, false);
    }

    let b = calc(s);

    // ---- 闸 3：seek ----
    if b.overflow {
        return Outcome::make(Stage::Seek, EXIT_FILE as i32, 0, Note::SeekErr, false);
    }

    // ---- 闸 4：read ----
    if s.read_fail {
        return Outcome::make(Stage::Read, EXIT_FILE as i32, 0, Note::ReadErr, false);
    }

    // ---- 闸 5：pair（与 F11681 总锚配对）----
    if b.write_failed && s.errno_seen != ANCHOR_ENOSPC {
        return Outcome::make(
            Stage::Abort,
            EXIT_FILE as i32,
            b.landed,
            Note::PairBroken,
            false,
        );
    }

    // ---- 闸 6：records ----
    if !s.status_none && !records_agree(s, &b) {
        return Outcome::make(Stage::Records, EXIT_FILE as i32, b.landed, Note::RecordsLie, false);
    }

    // ---- 闸 7：summary ----
    if s.status_none {
        let code = if b.write_failed || s.commit_fail { EXIT_FILE } else { EXIT_OK };
        return Outcome::make(Stage::Summary, code as i32, b.landed, Note::Silent, false);
    }

    // ---- 闸 8/9：abort 与 exit 的结算 ----
    if b.write_failed || s.commit_fail {
        let note = if s.commit_fail { Note::FsyncGnu } else { family_note(s.flavor) };
        return Outcome::make(Stage::Abort, EXIT_FILE as i32, b.landed, note, true);
    }
    let note = if s.count > 0 && b.cut_bytes > 0 {
        Note::DoneTrunc
    } else if b.out_part == 1 {
        Note::DonePartial
    } else {
        Note::Done
    };
    Outcome::make(Stage::Exit, EXIT_OK as i32, b.landed, note, false)
}

/// 汇总三行（records in / records out / 字节行）的复原文本。
///
/// busybox 打不出字节行，这里用块几何把它**复原**出来并写明是复原，
/// 而不是跟着日志说"未知"——本卡的字节判据量从来不依赖那一行在场。
fn summary_text(s: &Shape, b: &Blocks) -> String {
    let mut out = b.in_text();
    out.push_str(" | ");
    out.push_str(b.out_text().as_str());
    out.push_str(" | ");
    if s.flavor.prints_byte_line() {
        out.push_str(msg_bytes_copied(b.landed).as_str());
    } else {
        out.push_str(msg_bytes_recovered(s.path, b.landed).as_str());
    }
    out
}

/// 现场文案（note × 家族 → 真实文本；成功行给汇总，中止行给"错误 + 汇总"）。
pub fn message(s: &Shape, o: &Outcome) -> String {
    let b = calc(s);
    match o.note {
        Note::Usage => msg_unrecognized_operand(s.operand),
        Note::OutScope => msg_out_of_scope(s.operand),
        Note::OpenErr => msg_failed_to_open(s.path, s.errno_seen),
        Note::SeekErr => msg_cannot_seek(s.path),
        Note::ReadErr => msg_gnu_reading(s.path, s.errno_seen, s.flavor.modern()),
        Note::PairBroken => msg_pair_broken(-1, s.errno_seen),
        Note::RecordsLie => {
            let want;
            let got;
            if s.logged_in_full != b.in_full || s.logged_in_part != b.in_part {
                want = b.in_text();
                got = msg_records_in(s.logged_in_full, s.logged_in_part);
            } else if s.logged_out_full != b.out_full || s.logged_out_part != b.out_part {
                want = b.out_text();
                got = msg_records_out(s.logged_out_full, s.logged_out_part);
            } else {
                want = dec_text(b.landed);
                got = dec_text(s.logged_bytes);
            }
            msg_records_lie(want.as_str(), got.as_str())
        }
        Note::Silent => msg_summary_suppressed(),
        Note::FsyncGnu => {
            let mut m = msg_fsync_failed(s.path, ANCHOR_ENOSPC);
            m.push_str(" | ");
            m.push_str(summary_text(s, &b).as_str());
            m
        }
        Note::AbortGnu8 | Note::AbortGnu9 | Note::AbortBb => {
            let mut m = s.flavor.write_error_text(s.path, ANCHOR_ENOSPC);
            if b.partial() {
                m.push_str(" | ");
                m.push_str(msg_partial_marker(b.out_full, b.out_part, b.tail).as_str());
            }
            m.push_str(" | ");
            m.push_str(summary_text(s, &b).as_str());
            m
        }
        Note::Done | Note::DonePartial | Note::DoneTrunc => summary_text(s, &b)
    }
}

/// 逐槽释义（一行一条，进报表与测试的取证面）。
///
/// 块账只在写侧真的起跑过时才给数：`args`/`open` 两闸一字节未碰，把它们的
/// 落盘字节写成非零就是伪造现场，所以这里按 `Stage::has_ledger()` 归零。
pub fn detail(c: &Case) -> String {
    let o = c.actual();
    let b = c.blocks();
    let msg = message(&c.shape, &o);
    let keep = o.stage.has_ledger();
    // 字节取判定值（不是几何值）：读侧红的行，几何算得出 6144 落盘，
    // 但那一行的现场判据是 0——把几何值抄进来就是替 dd 编造汇总。
    let bytes = if keep { o.value } else { 0 };
    // partial 标记只在"这一行的字节确实来自块几何"时才给，否则标记与字节
    // 互相打脸（读侧红的行正是这种情形）。
    let partial = keep && b.partial() && o.value == b.landed;
    let mut s = String::from(c.shape.path);
    s.push_str(" flavor=");
    s.push_str(c.shape.flavor.name());
    s.push_str(" stage=");
    s.push_str(o.stage.name());
    s.push_str(" exit=");
    s.push_str(i32_text(o.code).as_str());
    s.push_str(" bytes=");
    s.push_str(dec_text(bytes).as_str());
    s.push_str(" partial=");
    s.push_str(bool_text(partial));
    s.push_str(" anchor=");
    s.push_str(bool_text(o.default));
    s.push_str(" note=");
    s.push_str(o.note.name());
    s.push_str(" msg=\"");
    s.push_str(msg.as_str());
    s.push('"');
    s.push('\n');
    s
}

/// 语料与场景的公共基准形状：GNU 9.x、`bs=4096`、输入 8192、余量 6144。
///
/// 这一格就是文档头那段现场。各槽用函数式更新（`..BASE`）只覆盖自己那一维，
/// 语料的差异因此**可读**：一眼能看出这一槽改了 dd 的哪个参数，而不是在 25
/// 个字段的字面量里找不同。
const BASE: Shape = Shape {
    tag: "base",
    path: "disk.img",
    operand: "",
    flavor: Flavor::Gnu9,
    bs: 4096,
    ibs: 0,
    obs: 0,
    src_bytes: 8192,
    free_bytes: 6144,
    count: 0,
    skip: 0,
    seek: 0,
    conv: 0,
    status_none: false,
    errno_seen: ERR_NONE,
    bad_option: false,
    open_fail: false,
    read_fail: false,
    commit_fail: false,
    logged_in_full: 0,
    logged_in_part: 0,
    logged_out_full: 0,
    logged_out_part: 0,
    logged_bytes: 0,
    logged_bytes_known: false,
};

/// 基准形状的六格申报（写死在 `BASE` 现场上，供后 5 个复刻槽复用）。
const BASE_ABORT: Claim =
    Claim { stage: Stage::Abort, code: EXIT_FILE as i32, value: 6144, note: Note::AbortGnu9, default: true };

// ---------------------------------------------------------------------------
// 语料（14 槽：8 活 + 6 死）
// ---------------------------------------------------------------------------

/// 8 槽申报正确的形状 + 6 槽复刻形状（复刻自 `gnu9-abort-half`）。
///
/// 前 8 槽按档位铺开：中止 3（GNU 9.x 半块 / GNU 8.x 整块 / busybox 半块）、
/// 完整 3（对齐收尾 / `ibs!=obs` 的成功残块 / `count=` 截断）、无汇总 1、
/// 用法 1。后 5 槽把 `gnu9-abort-half` 的现场**逐字**复刻，只改申报的一维；
/// 第 6 个死槽（Estate）故意换成 `conv=noerror` 现场并申报成活行——本卡的
/// 成败互报维要抓的就是这个越界，详见文档头的 F11697 边界。
pub const SHAPES: [Shape; CORPUS] = [
    Shape {
        tag: "gnu9-abort-half",
        errno_seen: ERR_ENOSPC,
        logged_in_full: 2,
        logged_out_full: 1,
        logged_out_part: 1,
        logged_bytes: 6144,
        logged_bytes_known: true,
        ..BASE
    },
    Shape {
        tag: "gnu8-abort-full",
        flavor: Flavor::Gnu8,
        bs: 512,
        src_bytes: 4096,
        free_bytes: 2048,
        errno_seen: ERR_ENOSPC,
        logged_in_full: 8,
        logged_out_full: 4,
        logged_bytes: 2048,
        logged_bytes_known: true,
        ..BASE
    },
    Shape {
        tag: "bb-abort-half",
        flavor: Flavor::Busybox,
        bs: 1024,
        src_bytes: 4096,
        free_bytes: 3584,
        errno_seen: ERR_ENOSPC,
        logged_in_full: 4,
        logged_out_full: 3,
        logged_out_part: 1,
        ..BASE
    },
    Shape {
        tag: "clean-full-copy",
        bs: 1024,
        src_bytes: 1024,
        free_bytes: 4096,
        logged_in_full: 1,
        logged_out_full: 1,
        logged_bytes: 1024,
        logged_bytes_known: true,
        ..BASE
    },
    Shape {
        tag: "partial-tail-clean",
        bs: 0,
        ibs: 1024,
        obs: 2048,
        src_bytes: 3072,
        free_bytes: 8192,
        logged_in_full: 3,
        logged_out_full: 1,
        logged_out_part: 1,
        logged_bytes: 3072,
        logged_bytes_known: true,
        ..BASE
    },
    Shape {
        tag: "count-truncated",
        bs: 512,
        src_bytes: 4096,
        free_bytes: 8192,
        count: 4,
        logged_in_full: 4,
        logged_out_full: 4,
        logged_bytes: 2048,
        logged_bytes_known: true,
        ..BASE
    },
    Shape {
        tag: "status-none-clean",
        bs: 1024,
        src_bytes: 2048,
        free_bytes: 4096,
        status_none: true,
        ..BASE
    },
    Shape {
        tag: "usage-bad-option",
        operand: "ibs=abc",
        src_bytes: 0,
        free_bytes: 0,
        bad_option: true,
        ..BASE
    },
    Shape {
        tag: "estate-swap",
        operand: "conv=noerror",
        conv: CONV_NOERROR,
        errno_seen: ERR_ENOSPC,
        logged_in_full: 2,
        logged_out_full: 1,
        logged_out_part: 1,
        logged_bytes: 6144,
        logged_bytes_known: true,
        ..BASE
    },
    Shape {
        tag: "stage-swap",
        errno_seen: ERR_ENOSPC,
        logged_in_full: 2,
        logged_out_full: 1,
        logged_out_part: 1,
        logged_bytes: 6144,
        logged_bytes_known: true,
        ..BASE
    },
    Shape {
        tag: "code-swap",
        errno_seen: ERR_ENOSPC,
        logged_in_full: 2,
        logged_out_full: 1,
        logged_out_part: 1,
        logged_bytes: 6144,
        logged_bytes_known: true,
        ..BASE
    },
    Shape {
        tag: "value-swap",
        errno_seen: ERR_ENOSPC,
        logged_in_full: 2,
        logged_out_full: 1,
        logged_out_part: 1,
        logged_bytes: 6144,
        logged_bytes_known: true,
        ..BASE
    },
    Shape {
        tag: "note-swap",
        errno_seen: ERR_ENOSPC,
        logged_in_full: 2,
        logged_out_full: 1,
        logged_out_part: 1,
        logged_bytes: 6144,
        logged_bytes_known: true,
        ..BASE
    },
    Shape {
        tag: "default-swap",
        errno_seen: ERR_ENOSPC,
        logged_in_full: 2,
        logged_out_full: 1,
        logged_out_part: 1,
        logged_bytes: 6144,
        logged_bytes_known: true,
        ..BASE
    },
];

/// 14 槽申报。前 8 槽与几何自洽（申报即事实），后 6 槽各错一维。
pub const CLAIMS: [Claim; CORPUS] = [
    BASE_ABORT,
    Claim {
        stage: Stage::Abort,
        code: EXIT_FILE as i32,
        value: 2048,
        note: Note::AbortGnu8,
        default: true,
    },
    Claim {
        stage: Stage::Abort,
        code: EXIT_FILE as i32,
        value: 3584,
        note: Note::AbortBb,
        default: true,
    },
    Claim { stage: Stage::Exit, code: EXIT_OK as i32, value: 1024, note: Note::Done, default: false },
    Claim {
        stage: Stage::Exit,
        code: EXIT_OK as i32,
        value: 3072,
        note: Note::DonePartial,
        default: false,
    },
    Claim {
        stage: Stage::Exit,
        code: EXIT_OK as i32,
        value: 2048,
        note: Note::DoneTrunc,
        default: false,
    },
    Claim {
        stage: Stage::Summary,
        code: EXIT_OK as i32,
        value: 2048,
        note: Note::Silent,
        default: false,
    },
    Claim { stage: Stage::Args, code: EXIT_USAGE as i32, value: 0, note: Note::Usage, default: false },
    // estate-swap：把一条 conv=noerror 的越界行申报成"跑完了、写全了"。
    Claim { stage: Stage::Exit, code: EXIT_OK as i32, value: 8192, note: Note::Done, default: false },
    // stage-swap：现场死在 abort 闸，申报成"停在 write 闸"。
    Claim {
        stage: Stage::Write,
        code: EXIT_FILE as i32,
        value: 6144,
        note: Note::AbortGnu9,
        default: true,
    },
    // code-swap：红了却申报退 0（把 dd 的失败读成成功收尾）。
    Claim { stage: Stage::Abort, code: EXIT_OK as i32, value: 6144, note: Note::AbortGnu9, default: true },
    // value-swap：把请求字节当落盘字节（半块账里最贵的一格）。
    Claim {
        stage: Stage::Abort,
        code: EXIT_FILE as i32,
        value: 8192,
        note: Note::AbortGnu9,
        default: true,
    },
    // note-swap：GNU 9.x 现场报成 busybox 句式（家族混认）。
    Claim { stage: Stage::Abort, code: EXIT_FILE as i32, value: 6144, note: Note::AbortBb, default: true },
    // default-swap：四件齐了却不落锚（本卡的锚位判据被绕过）。
    Claim {
        stage: Stage::Abort,
        code: EXIT_FILE as i32,
        value: 6144,
        note: Note::AbortGnu9,
        default: false,
    },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Stage, Fault::Code, Fault::Value, Fault::Note, Fault::Default];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] = [
    DD_ESTATE,
    DD_ESTAGE,
    DD_ECODE,
    DD_EVALUE,
    DD_ENOTE,
    DD_EDEFAULT,
];

/// 账本 8 个成格的值（总槽、成、败、完整档、无汇总档、中止档、拒绝档、红档 ppm）。
///
/// 第 8 格是**注入后的红档率** 929，不是全绿率：账本要留一格非 1000 的档，
/// 否则"rate 硬编码成 1000"这种退化没人抓得住。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 3, 1, 3, 1, 929];

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

/// 覆盖计数：槽数不等于语料规模就是无观测。
pub fn cover(n: usize) -> Result<(), u32> {
    if n != CORPUS {
        return Err(DD_ENOBS);
    }
    Ok(())
}

/// 期望缺陷。
pub fn expected(i: usize) -> Option<Fault> {
    if i < ACCEPT {
        return Some(Fault::Ok);
    }
    match REJECT_FAULTS.get(i - ACCEPT) {
        Some(v) => Some(*v),
        None => None,
    }
}

/// 对判：状态 → 阶段 → 退出码 → 落盘字节 → 注解 → 锚位。
///
/// 次序固定，且**成败互报优先**：一条把中止申报成成功的行，后面五维说什么
/// 都已不可信，先记 Estate。把这条放到最后，就会让 `code-swap` 那类"码错但
/// 注解仍属失败家族"的行同时命中两维，缺陷统计随之失真。
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

// ---------------------------------------------------------------------------
// 期望表（语料的第二、第三种陈述）
// ---------------------------------------------------------------------------

/// 14 槽的期望结束闸（申报表的第二形态）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Abort,
    Stage::Abort,
    Stage::Abort,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Summary,
    Stage::Args,
    Stage::Args,
    Stage::Abort,
    Stage::Abort,
    Stage::Abort,
    Stage::Abort,
    Stage::Abort,
];

/// 14 槽的期望退出码。
pub const WANT_CODE: [i32; CORPUS] = [
    EXIT_FILE as i32,
    EXIT_FILE as i32,
    EXIT_FILE as i32,
    EXIT_OK as i32,
    EXIT_OK as i32,
    EXIT_OK as i32,
    EXIT_OK as i32,
    EXIT_USAGE as i32,
    EXIT_UNJUDGED as i32,
    EXIT_FILE as i32,
    EXIT_FILE as i32,
    EXIT_FILE as i32,
    EXIT_FILE as i32,
    EXIT_FILE as i32,
];

/// 14 槽的期望落盘字节。
pub const WANT_VALUE: [u32; CORPUS] = [
    6144, 2048, 3584, 1024, 3072, 2048, 2048, 0, 0, 6144, 6144, 6144, 6144, 6144,
];

/// 14 槽的期望注解。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::AbortGnu9,
    Note::AbortGnu8,
    Note::AbortBb,
    Note::Done,
    Note::DonePartial,
    Note::DoneTrunc,
    Note::Silent,
    Note::Usage,
    Note::OutScope,
    Note::AbortGnu9,
    Note::AbortGnu9,
    Note::AbortGnu9,
    Note::AbortGnu9,
    Note::AbortGnu9,
];

/// 14 槽的期望锚位。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    true, true, true, false, false, false, false, false, false, true, true, true, true, true,
];

/// 14 槽的期望缺陷秩（0 = 无缺陷；1..6 是 `JUDGE_ORDER` 的下标加一）。
pub const WANT_NDX: [u8; CORPUS] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6];

/// 14 槽标签。
pub const WANT_TAGS: [&str; CORPUS] = [
    "gnu9-abort-half",
    "gnu8-abort-full",
    "bb-abort-half",
    "clean-full-copy",
    "partial-tail-clean",
    "count-truncated",
    "status-none-clean",
    "usage-bad-option",
    "estate-swap",
    "stage-swap",
    "code-swap",
    "value-swap",
    "note-swap",
    "default-swap",
];

// --- 块账的第二形态：records 三对数与三个字节数（逐槽手算，与 calc 交叉核对） ---

/// 14 槽的 `records in` 整块数。
pub const ACT_IN_FULL: [u32; CORPUS] = [2, 8, 4, 1, 3, 4, 2, 0, 2, 2, 2, 2, 2, 2];

/// 14 槽的 `records in` 残块数（本语料没有短读到非整数块的输入）。
pub const ACT_IN_PART: [u32; CORPUS] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

/// 14 槽的 `records out` 整块数。
pub const ACT_OUT_FULL: [u32; CORPUS] = [1, 4, 3, 1, 1, 4, 2, 0, 1, 1, 1, 1, 1, 1];

/// 14 槽的 `records out` 残块数（partial-block 标记的唯一来源）。
pub const ACT_OUT_PART: [u32; CORPUS] = [1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1];

/// 14 槽的落盘字节。
pub const ACT_LANDED: [u32; CORPUS] = [6144, 2048, 3584, 1024, 3072, 2048, 2048, 0, 6144, 6144, 6144, 6144, 6144, 6144];

/// 14 槽的丢失字节（请求减落盘；`count=` 截掉的不算丢失）。
pub const ACT_LOST: [u32; CORPUS] = [2048, 2048, 512, 0, 0, 0, 0, 0, 2048, 2048, 2048, 2048, 2048, 2048];

/// 14 槽被 `count=` 截掉的字节（既不在读账也不在写账）。
pub const ACT_CUT: [u32; CORPUS] = [0, 0, 0, 0, 0, 2048, 0, 0, 0, 0, 0, 0, 0, 0];

/// 申报正确的 8 格下标。
const ACCEPT_SLOTS: [usize; ACCEPT] = [0usize, 1, 2, 3, 4, 5, 6, 7];

/// 申报错误的 6 格下标。
const REJECT_SLOTS: [usize; REJECT] = [8usize, 9, 10, 11, 12, 13];

// ---------------------------------------------------------------------------
// 场景表（语料之外的闸门终例：九闸的每一道都要有一行**停在那儿**的现场）
// ---------------------------------------------------------------------------

/// 语料只有 14 格，装不下全部九闸的终例：`open`/`seek`/`read`/`records` 四道
/// 闸的正例、`conv=fsync` 的迟到爆、`status=none` 的中止都放在这里。
/// 这些行进 `scenes_ok()` 的断言，也进文案家族的取证面，但**不进**语料计数。
pub const SCENE_COUNT: usize = 7;

/// 场景表（下标即 `SCENE_*` 期望表的行号）。
pub fn scenes() -> [Shape; SCENE_COUNT] {
    [
        Shape {
            tag: "open-fail",
            open_fail: true,
            errno_seen: ERR_ENOENT,
            src_bytes: 4096,
            ..BASE
        },
        Shape {
            tag: "seek-hole-full",
            free_bytes: 0,
            seek: 1,
            errno_seen: ERR_ENOSPC,
            logged_in_full: 2,
            logged_out_full: 0,
            logged_bytes: 0,
            logged_bytes_known: true,
            ..BASE
        },
        Shape {
            tag: "read-eio",
            read_fail: true,
            errno_seen: ERR_EIO,
            ..BASE
        },
        Shape {
            tag: "fsync-delalloc",
            conv: CONV_FSYNC,
            bs: 1024,
            src_bytes: 2048,
            free_bytes: 2048,
            commit_fail: true,
            errno_seen: ERR_ENOSPC,
            logged_in_full: 2,
            logged_out_full: 2,
            logged_bytes: 2048,
            logged_bytes_known: true,
            ..BASE
        },
        Shape {
            tag: "records-lie",
            errno_seen: ERR_ENOSPC,
            logged_in_full: 2,
            logged_out_full: 2,
            logged_bytes: 6144,
            logged_bytes_known: true,
            ..BASE
        },
        Shape {
            tag: "pair-broken",
            errno_seen: ERR_EIO,
            logged_in_full: 2,
            logged_out_full: 1,
            logged_out_part: 1,
            logged_bytes: 6144,
            logged_bytes_known: true,
            ..BASE
        },
        Shape {
            tag: "status-none-abort",
            status_none: true,
            errno_seen: ERR_ENOSPC,
            ..BASE
        },
    ]
}

/// 场景标签。
pub const SCENE_TAGS: [&str; SCENE_COUNT] = [
    "open-fail",
    "seek-hole-full",
    "read-eio",
    "fsync-delalloc",
    "records-lie",
    "pair-broken",
    "status-none-abort",
];

/// 场景的期望终点闸。
pub const SCENE_WANT_STAGE: [Stage; SCENE_COUNT] = [
    Stage::Open,
    Stage::Abort,
    Stage::Read,
    Stage::Abort,
    Stage::Records,
    Stage::Abort,
    Stage::Summary,
];

/// 场景的期望退出码。
pub const SCENE_WANT_CODE: [i32; SCENE_COUNT] = [
    EXIT_FILE as i32,
    EXIT_FILE as i32,
    EXIT_FILE as i32,
    EXIT_FILE as i32,
    EXIT_FILE as i32,
    EXIT_FILE as i32,
    EXIT_FILE as i32,
];

/// 场景的期望落盘字节。
pub const SCENE_WANT_VALUE: [u32; SCENE_COUNT] = [0, 0, 0, 2048, 6144, 6144, 6144];

/// 场景的期望注解。
pub const SCENE_WANT_NOTE: [Note; SCENE_COUNT] = [
    Note::OpenErr,
    Note::AbortGnu9,
    Note::ReadErr,
    Note::FsyncGnu,
    Note::RecordsLie,
    Note::PairBroken,
    Note::Silent,
];

/// 场景的期望锚位（只有真红且对得上账的两行亮锚）。
pub const SCENE_WANT_DEFAULT: [bool; SCENE_COUNT] = [false, true, false, true, false, false, false];

/// 第 i 个场景（越界给 None，不回绕）。
pub fn scene_at(i: usize) -> Option<Shape> {
    scenes().get(i).copied()
}

/// 场景是否被本卡受理（`abort` 闸亮锚）。
pub fn scene_anchored(i: usize) -> Option<bool> {
    match scene_at(i) {
        Some(s) => Some(define(&s).anchored()),
        None => None,
    }
}

// ---------------------------------------------------------------------------
// 统计矩阵
// ---------------------------------------------------------------------------

/// 一档统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// 该档的实际槽数（覆盖面）。
    pub total: usize,
    /// 该档申报正确数（可信度）。
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
/// 分档只看**实际结果**落在哪一档，申报是否正确单独计（`good`）。两维混在
/// 一起的话，6 个错报槽会从自己那一档里消失，覆盖面看起来仍是满的——本卡
/// 的 6 个死槽全部复刻 `abort` 现场，正好把这一点放大：`aborted` 那行的
/// `total=9` 而 `good=3`，读表人一眼能看出"这一档覆盖够，但申报有水分"。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 四档行（complete/silent/aborted/refused）。
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
        let tiers = [Tier::Complete, Tier::Silent, Tier::Aborted, Tier::Refused];
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
    /// 完整档（申报正确）数。
    pub complete: u32,
    /// 无汇总档（申报正确）数。
    pub silent: u32,
    /// 中止档（申报正确）数。
    pub aborted: u32,
    /// 拒绝档（申报正确）数。
    pub refused: u32,
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
            Err(DD_ECOUNT)
        }
    }

    /// 成功档合计（申报正确且结果成功）。
    pub fn success_cells(&self) -> u32 {
        self.complete + self.silent
    }

    /// 失败档合计（申报正确且结果失败）。
    pub fn failure_cells(&self) -> u32 {
        self.aborted + self.refused
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("dd-records corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" complete=");
        s.push_str(u32_text(self.complete).as_str());
        s.push_str(" silent=");
        s.push_str(u32_text(self.silent).as_str());
        s.push_str(" aborted=");
        s.push_str(u32_text(self.aborted).as_str());
        s.push_str(" refused=");
        s.push_str(u32_text(self.refused).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 全绿渲染的逐字钉版。
const RENDER_OK: &str =
    "dd-records corpus=14 accept=8 reject=6 complete=3 silent=1 aborted=3 refused=1 match=14 rate=1000ppm\n";

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 可注入一槽缺陷的全跑。
///
/// 注入把"期望表"本身也放进被测面：强行宣称某槽犯了另一种缺陷，报表就必须
/// 掉 `matched`、`rate` 随之离开 1000；档位计数也不许再涨——注入只改 `f`
/// 不改实际结果，`Tier::of(&a)` 的入账前提是 `f == Fault::Ok`。槽位越界返回
/// `DD_ESLOT`，不许静默截断成"最后一槽"。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(DD_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut complete = 0u32;
    let mut silent = 0u32;
    let mut aborted = 0u32;
    let mut refused = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(DD_ENOBS),
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
            None => return Err(DD_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(&a) {
                Tier::Complete => complete += 1,
                Tier::Silent => silent += 1,
                Tier::Aborted => aborted += 1,
                Tier::Refused => refused += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, complete, silent, aborted, refused, matched })
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门（秩从 **1** 起算，只接受"前进到下一秩"）。
///
/// 与允许同段重复的写法相比，这里更严：同一道闸走两次说明观测者在原地打转，
/// 而真实 dd 一行只停一次。跳档（`args` 直接到 `abort`）在语料里对应"没打开
/// 文件就宣称落盘字节"，是本卡最典型的伪造路径，必须留 `DD_EBAL` 痕迹；
/// 倒退对应事后倒推，留 `DD_ESTAGE`；封印后再动留 `DD_ESTATE`。
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    /// 当前秩（0 表示未起跑）。
    rank: u32,
    /// 是否已封印。
    closed: bool,
    /// 尝试次数（含被拒）。
    moves: u32,
    /// 被拒次数。
    rejected: u32,
    /// 走法指纹。
    fp: u64,
}

impl Gate {
    /// 新建：秩 0。
    pub fn new() -> Gate {
        Gate { rank: 0, closed: false, moves: 0, rejected: 0, fp: FNV_OFF }
    }

    /// 当前秩。
    pub const fn rank(&self) -> u32 {
        self.rank
    }

    /// 当前闸（秩 0 没有闸）。
    pub fn stage(&self) -> Option<Stage> {
        Stage::from_rank(self.rank)
    }

    /// 尝试次数。
    pub const fn moves(&self) -> u32 {
        self.moves
    }

    /// 被拒次数。
    pub const fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 是否已封印。
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// 走法指纹。
    pub const fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 前进一道：只有 `rank + 1` 被接受。
    pub fn step(&mut self, to: Stage) -> Result<(), u32> {
        self.moves = self.moves.saturating_add(1);
        self.fp = fold_word(self.fp, to.rank() as u64);
        if self.closed {
            self.rejected = self.rejected.saturating_add(1);
            return Err(DD_ESTATE);
        }
        let want = self.rank.saturating_add(1);
        let r = to.rank();
        if r == want {
            self.rank = r;
            return Ok(());
        }
        self.rejected = self.rejected.saturating_add(1);
        self.fp = fold_str(self.fp, "reject");
        if r < self.rank {
            return Err(DD_ESTAGE);
        }
        Err(DD_EBAL)
    }

    /// 封印：必须已经站到最后一道闸（`exit`，秩 9）。
    ///
    /// 成功时返回**封印时的秩**（而不是尝试次数）：判据要读的是"这条纵线走到
    /// 第几闸就结了账"，次数含被拒尝试，用它当结论会把拒绝也算进成绩。
    pub fn close(&mut self) -> Result<u32, u32> {
        self.moves = self.moves.saturating_add(1);
        if self.closed {
            self.rejected = self.rejected.saturating_add(1);
            return Err(DD_ESTATE);
        }
        if self.rank != STAGE_LAST_RANK {
            self.rejected = self.rejected.saturating_add(1);
            return Err(DD_ESTAGE);
        }
        self.closed = true;
        self.fp = fold_str(self.fp, "close");
        Ok(self.rank)
    }

    /// 沿一行的实际路径把闸门走到底（判据用它复核"这行能不能按闸序走到终点"）。
    pub fn walk(&mut self, stages: &[Stage]) -> Result<(), u32> {
        let mut i = 0usize;
        while i < stages.len() {
            match stages.get(i) {
                Some(s) => {
                    if self.step(*s) != Ok(()) {
                        return Err(DD_ESTAGE);
                    }
                }
                None => return Err(DD_ENOBS),
            }
            i += 1;
        }
        self.close().map(|_| ())
    }
}

/// 沿闸序从 `args` 一路走到该行的终点闸，然后尝试封印。
///
/// 封印只在终点是 `exit`（秩 9）时成功——`Gate::close()` 要求已经站到最后一
/// 道闸。所以中止行返回 `DD_ESTAGE`：它停在自己的终点闸上，闸门**不许**被
/// 当作"整条纵线走完了"来封印。这个不对称是有意设计的：把任何红行都封成
/// 绿，是判据最容易自我原谅的方式。
pub fn gate_walk(o: &Outcome) -> Result<u32, u32> {
    let mut g = Gate::new();
    let all = Stage::all();
    let stop = o.stage.rank();
    let mut i = 0usize;
    while i < STAGE_COUNT as usize {
        let s = match all.get(i) {
            Some(v) => *v,
            None => return Err(DD_ENOBS),
        };
        if s.rank() > stop {
            break;
        }
        if g.step(s) != Ok(()) {
            return Err(DD_ESTAGE);
        }
        i += 1;
    }
    g.close()
}

/// 终点闸的秩（判据与释义行共用，避免各处重复 `stage.rank()`）。
pub fn gate_stop_rank(o: &Outcome) -> u32 {
    o.stage.rank()
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
    /// 错误码（成格恒 0）。
    pub code: u32,
    /// 观测值（败格恒 0）。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }

    /// 成格。
    pub const fn good(slot: u8, op: u8, value: u32) -> Sample {
        Sample { slot, op, ok: true, code: 0, value }
    }

    /// 败格。
    pub const fn bad(slot: u8, op: u8, code: u32) -> Sample {
        Sample { slot, op, ok: false, code, value: 0 }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.slot as u64);
        h = fold_word(h, self.op as u64);
        h = fold_word(h, self.ok as u64);
        h = fold_word(h, self.code as u64);
        fold_word(h, self.value as u64)
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
            return Err(DD_ESLOT);
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
    /// 败格值必归零、成格码必归零。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(DD_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DD_EBAL),
            };
            if s.slot as usize != i {
                return Err(DD_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DD_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DD_EVALUE);
                        }
                    }
                    None => return Err(DD_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(DD_EVALUE);
                }
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DD_EVALUE);
                        }
                    }
                    None => return Err(DD_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(DD_EBAL);
        }
        Ok(())
    }

    /// 全账指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold_word(h, s.seal());
            }
            i += 1;
        }
        fold_word(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DD_EBAL);
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
            Err(DD_ESEAL)
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

    /// 篡改第 i 格的观测值（只给测试与注入用，返回是否改到）。
    ///
    /// 有这个方法而不是让测试直接够到 `rows`，是为了把"改哪一格"这件事留成
    /// 一条可读的判据陈述：`poke(7, 1000)` 就是"把红档格伪造成全绿档"。
    pub fn poke(&mut self, i: usize, value: u32) -> bool {
        match self.rows.get_mut(i) {
            Some(r) => {
                r.value = value;
                true
            }
            None => false,
        }
    }

    /// 篡改第 i 格的操作码（审计面上没有 op 这一列，故 audit 放行、指纹翻脸）。
    pub fn poke_op(&mut self, i: usize, op: u8) -> bool {
        match self.rows.get_mut(i) {
            Some(r) => {
                r.op = op;
                true
            }
            None => false,
        }
    }
}

/// 计数结果 → 样本。
fn count_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample::good(slot, op, v),
        Err(c) => Sample::bad(slot, op, c),
    }
}

/// 建账本：7 格统计 + 1 格红档率（929）+ 6 格缺陷码。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total)));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept)));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject)));
    let _ = l.register(count_cell(3, OP_RECORDS, r.map(|x| x.complete)));
    let _ = l.register(count_cell(4, OP_RECORDS, r.map(|x| x.silent)));
    let _ = l.register(count_cell(5, OP_WRITE, r.map(|x| x.aborted)));
    let _ = l.register(count_cell(6, OP_ARGS, r.map(|x| x.refused)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_RATE, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample::bad((ACCEPT + i) as u8, OP_DEFECT, f.code()));
        }
        i += 1;
    }
    l
}

/// 篡改账本的**值**列：建账 → 改第 `i` 格 → 冻结。
///
/// 值列在 `audit()` 的对账面上，所以这一改会让 `audit()` 当场翻脸；指纹同样
/// 记不住它——用它测"对账拦得住"这条路。
pub fn tampered_ledger(i: usize, v: u32) -> Ledger {
    let mut l = build_ledger();
    if l.poke(i, v) {
        let _ = l.freeze();
    }
    l
}

/// 篡改账本的**操作码**列：建账 → 冻结 → 改第 `i` 格。
///
/// `audit()` 只看 slot/ok/code/value 四列，op 不在对账面上：冻结之后再改 op，
/// 对账照样绿，但 `replay()` 的指纹当场不符。这一对（audit 放过、replay 拦住）
/// 就是本卡"审计有已知盲区、指纹兜底"的可执行陈述。
pub fn tampered_op_ledger(i: usize, op: u8) -> Ledger {
    let mut l = build_ledger();
    let _ = l.freeze();
    let _ = l.poke_op(i, op);
    l
}

// ---------------------------------------------------------------------------
// 段位交叉锚定（与 F11681 / F11682 的链上契约，不 import，只自声明复算）
// ---------------------------------------------------------------------------

/// 后继卡 F11684（tee 半程失败）自声明的本卡段位，写在这里做双向核对。
pub const ANCHOR_F11684_SEG: u32 = 0x6d160;

/// 三条锚是否互相自洽。
///
/// 本卡与邻居卡**不互相 import**（判据卡必须能单独被摘出去跑），所以链上关系
/// 靠各自自声明的常量复算：本卡的 `SEG_BASE - GAP` 必须等于邻居自声明的段位，
/// 且段位严格递增。任一侧改了段基址或位距，这里立刻不平——这是本卡对"链上
/// 结算"唯一的机械守卫，`name_ok()` 与第 1 项检查都消费它。
pub fn anchors_agree() -> bool {
    if SEG_BASE != 0x6d150 {
        return false;
    }
    if ANCHOR_F11681_SEG != 0x6d130 || ANCHOR_F11682_SEG != 0x6d140 {
        return false;
    }
    if ANCHOR_F11681_SEG.saturating_add(ANCHOR_F11681_GAP) != SEG_BASE {
        return false;
    }
    if ANCHOR_F11682_SEG.saturating_add(ANCHOR_F11682_GAP) != SEG_BASE {
        return false;
    }
    if ANCHOR_F11681_SEG >= ANCHOR_F11682_SEG || ANCHOR_F11682_SEG >= SEG_BASE {
        return false;
    }
    if SEG_BASE.saturating_add(0x10) != ANCHOR_F11684_SEG {
        return false;
    }
    // 总锚的两件：errno 与退出码必须与邻居卡同一读法，否则交叉引用即错位。
    if ANCHOR_ENOSPC != ERR_ENOSPC || ANCHOR_ENOSPC != 28 {
        return false;
    }
    if ANCHOR_EXIT_FILE != EXIT_FILE {
        return false;
    }
    if ANCHOR_ERRNO_TEXT != strerror(ERR_ENOSPC) {
        return false;
    }
    // 本卡 13 码必须全部落在本段内，且不与邻居段的任何码重合。
    let mut i = 0usize;
    while i < ALL_CODES.len() {
        match ALL_CODES.get(i) {
            Some(c) => {
                if *c <= SEG_BASE || *c > SEG_BASE + 0x0d {
                    return false;
                }
                if *c >= ANCHOR_F11684_SEG {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 段位的可视化陈述（审计与测试都读它，不读硬编码字符串）。
pub fn anchor_text() -> String {
    let mut s = String::from("f11681=");
    s.push_str(hex_text(ANCHOR_F11681_SEG, 5).as_str());
    s.push_str(" f11682=");
    s.push_str(hex_text(ANCHOR_F11682_SEG, 5).as_str());
    s.push_str(" f11683=");
    s.push_str(hex_text(SEG_BASE, 5).as_str());
    s.push_str(" f11684=");
    s.push_str(hex_text(ANCHOR_F11684_SEG, 5).as_str());
    s
}

// ---------------------------------------------------------------------------
// 四路注入
// ---------------------------------------------------------------------------

/// 四路注入的路径名（表路 / 判路 / 渲染路 / 账本路）。
pub const INJECT_ROUTES: [&str; 4] = ["table", "judge", "render", "ledger"];

/// 四路注入：任何一路静默退化都会在这里变成 false。
///
/// 1. **表路**——段基址与链上锚、枚举划分、设备几何、形状合法域；
/// 2. **判路**——九闸纵线、块算术与半块账、越界拒收、对判次序；
/// 3. **渲染路**——语料三陈、期望表、红档、文案逐字、释义行；
/// 4. **账本路**——阶段闸门、逐格对账、冻结重放。
///
/// 四路各自独立成句：`inject_four()[1]` 单独为 false 就够说明"判据退化"，
/// 不必读者去猜是哪一维。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && enums_ok() && geom_ok() && shape_ok();
    out[1] = define_ok() && records_ok() && scope_ok() && judge_ok();
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

/// 四路是否全捕。
pub fn inject_all_caught() -> bool {
    let v = inject_four();
    let mut i = 0usize;
    while i < 4 {
        match v.get(i) {
            Some(true) => {}
            _ => return false,
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 判据基元
// ---------------------------------------------------------------------------

/// 名称与段基址：常量链、尺寸、errno 真值、退出码、操作码、conv 位、锚定。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d150 {
        return false;
    }
    if DD_ESTAGE != SEG_BASE + 0x01
        || DD_ECODE != SEG_BASE + 0x02
        || DD_EVALUE != SEG_BASE + 0x03
        || DD_ENOTE != SEG_BASE + 0x04
        || DD_EDEFAULT != SEG_BASE + 0x05
        || DD_EPARTIAL != SEG_BASE + 0x06
    {
        return false;
    }
    if DD_ESTATE != SEG_BASE + 0x07
        || DD_ECOUNT != SEG_BASE + 0x08
        || DD_ESLOT != SEG_BASE + 0x09
        || DD_ESEAL != SEG_BASE + 0x0a
        || DD_ENOBS != SEG_BASE + 0x0b
        || DD_EMATRIX != SEG_BASE + 0x0c
        || DD_EBAL != SEG_BASE + 0x0d
    {
        return false;
    }
    if ALL_CODES.len() != 13 {
        return false;
    }
    if CORPUS != 14 || SAMPLES != 14 || ACCEPT != 8 || REJECT != 6 || TIER_COUNT != 4 {
        return false;
    }
    if STAT_CELLS != 7 || RED_RATE != 929 || RATE_FULL != 1000 {
        return false;
    }
    if STAGE_COUNT != 9 || STAGE_LAST_RANK != 9 || STAGE_COUNT != Stage::all().len() as u32 {
        return false;
    }
    // errno 必须是 Linux/POSIX 真值，判据卡不许自造号码。
    if ERR_ENOSPC != 28 || ERR_EIO != 5 || ERR_EACCES != 13 || ERR_ENOENT != 2 {
        return false;
    }
    if ERR_EDQUOT != 122 || ERR_NONE != 0 || ERRNO_SET_SIZE != 6 {
        return false;
    }
    // dd 的退出码：0 成功 / 1 文件错 / 2 用法错；99 是本卡的不裁定哨兵。
    if EXIT_OK != 0 || EXIT_FILE != 1 || EXIT_USAGE != 2 || EXIT_UNJUDGED != 99 {
        return false;
    }
    if EXIT_RANKS != 4 {
        return false;
    }
    if OP_ARGS != 1 || OP_OPEN != 2 || OP_READ != 3 || OP_WRITE != 4 || OP_RECORDS != 5 {
        return false;
    }
    if OP_CORPUS != 6 || OP_RATE != 7 || OP_DEFECT != 8 || OP_REPLAY != 9 {
        return false;
    }
    if OP_COUNT != 9 {
        return false;
    }
    if CONV_NOTRUNC != 1 || CONV_FSYNC != 2 || CONV_EXCL != 4 || CONV_NOERROR != 8 {
        return false;
    }
    if CONV_SYNC != 0x10 || CONV_BIT_COUNT != 5 || CONV_BITS.len() != CONV_BIT_COUNT {
        return false;
    }
    if CONV_OUTSCOPE & CONV_NOERROR == 0 || CONV_OUTSCOPE & CONV_SYNC == 0 {
        return false;
    }
    if CONV_OUTSCOPE & CONV_FSYNC != 0 || CONV_OUTSCOPE & CONV_NOTRUNC != 0 {
        return false;
    }
    if BLOCK_MAX == 0 || BYTES_MAX == 0 || BYTES_MAX > 0x0fff_ffff {
        return false;
    }
    if strerror(ERR_ENOSPC) != "No space left on device" {
        return false;
    }
    if strerror(ERR_EIO) != "Input/output error" || strerror(ERR_EACCES) != "Permission denied" {
        return false;
    }
    if strerror(ERR_ENOENT) != "No such file or directory" {
        return false;
    }
    if strerror(ERR_EDQUOT) != "Quota exceeded" {
        return false;
    }
    if !strerror(ERR_NONE).is_empty() || strerror(4242) != "Unknown error" {
        return false;
    }
    if errno_name(ERR_ENOSPC) != "ENOSPC" || errno_name(ERR_NONE) != "0" {
        return false;
    }
    if errno_name(4242) != "?" {
        return false;
    }
    if JUDGE_ORDER.len() != REJECT || REJECT_FAULTS.len() != REJECT || REJECT_CODES.len() != REJECT
    {
        return false;
    }
    if VALS.len() != ACCEPT {
        return false;
    }
    anchors_agree()
}

/// 枚举：秩、文本唯一性、分类不重不漏。
fn enums_ok() -> bool {
    // 阶段：秩从 1 严格递增到 9，反查闭合，0 与 10 都查不出来。
    let stages = Stage::all();
    let mut i = 0usize;
    while i < stages.len() {
        let s = match stages.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.rank() != (i as u32) + 1 {
            return false;
        }
        match Stage::from_rank(s.rank()) {
            Some(back) => {
                if back != s {
                    return false;
                }
            }
            None => return false,
        }
        if s.name().is_empty() {
            return false;
        }
        let mut j = 0usize;
        while j < stages.len() {
            if i != j {
                if let (Some(a), Some(b)) = (stages.get(i), stages.get(j)) {
                    if a.name() == b.name() || a.rank() == b.rank() {
                        return false;
                    }
                }
            }
            j += 1;
        }
        i += 1;
    }
    if Stage::from_rank(0).is_some() || Stage::from_rank(10).is_some() {
        return false;
    }
    if Stage::Exit.failure() || !Stage::Abort.failure() {
        return false;
    }
    if !Stage::Args.pre_loop() || !Stage::Open.pre_loop() || Stage::Write.pre_loop() {
        return false;
    }
    if !Stage::Write.has_ledger() || !Stage::Abort.has_ledger() || Stage::Open.has_ledger() {
        return false;
    }
    if !Stage::Records.space_family() || !Stage::Summary.space_family() {
        return false;
    }
    if Stage::Read.space_family() || Stage::Seek.space_family() {
        return false;
    }

    // 注解：15 个短名唯一，且六个类别恰好不重不漏。
    let notes = Note::all();
    if notes.len() != 15 {
        return false;
    }
    let mut covered = 0usize;
    let mut k = 0usize;
    while k < notes.len() {
        let n = match notes.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if n.name().is_empty() {
            return false;
        }
        let mut m = 0usize;
        while m < notes.len() {
            if m != k {
                if let Some(o) = notes.get(m) {
                    if o.name() == n.name() {
                        return false;
                    }
                }
            }
            m += 1;
        }
        // 不重：六个类别里至多命中一个。
        let hits = [
            n.success(),
            n.abort_family(),
            n.foreign(),
            n.usage(),
            n.out_of_scope(),
            n.lie(),
        ];
        let mut ones = 0usize;
        let mut t = 0usize;
        while t < hits.len() {
            if matches!(hits.get(t), Some(true)) {
                ones += 1;
            }
            t += 1;
        }
        if ones != 1 {
            return false;
        }
        covered += 1;
        k += 1;
    }
    if covered != 15 {
        return false;
    }
    if !Note::Silent.success() || !Note::DonePartial.success() || Note::PairBroken.success() {
        return false;
    }
    if !Note::FsyncGnu.abort_family() || !Note::FsyncGnu.gnu_text() {
        return false;
    }
    if Note::AbortBb.gnu_text() || !Note::AbortGnu8.gnu_text() {
        return false;
    }
    if Note::Done.wants_partial_marker() || !Note::AbortGnu9.wants_partial_marker() {
        return false;
    }

    // 档位：四档下标闭合、短名唯一、成功档与中止档互斥。
    let tiers = [Tier::Complete, Tier::Silent, Tier::Aborted, Tier::Refused];
    if tiers.len() != TIER_COUNT {
        return false;
    }
    let mut z = 0usize;
    while z < tiers.len() {
        let t = match tiers.get(z) {
            Some(v) => *v,
            None => return false,
        };
        if t.index() != z {
            return false;
        }
        match Tier::from_index(z) {
            Some(back) => {
                if back != t {
                    return false;
                }
            }
            None => return false,
        }
        if t.name().is_empty() {
            return false;
        }
        // 成功与亮锚互斥：一档不可能既算绿又落 ENOSPC 锚。
        if t.success() && t.anchored() {
            return false;
        }
        if (t == Tier::Aborted) != t.anchored() {
            return false;
        }
        if (t == Tier::Complete || t == Tier::Silent) != t.success() {
            return false;
        }
        if t.needs_summary() && t == Tier::Silent {
            return false;
        }
        if t == Tier::Aborted && !t.needs_summary() {
            return false;
        }
        z += 1;
    }
    if Tier::from_index(TIER_COUNT).is_some() {
        return false;
    }

    // 缺陷：14 个码互不相同，六维判序与 judged() 一致。
    let faults = Fault::all();
    if faults.len() != 14 {
        return false;
    }
    if matches!(faults.get(0), Some(f) if *f != Fault::Ok) {
        return false;
    }
    let mut p = 0usize;
    let mut seen = [0u32; 14];
    let mut q = 0usize;
    while q < faults.len() {
        let f = match faults.get(q) {
            Some(v) => *v,
            None => return false,
        };
        if (f.code() == 0) != (f == Fault::Ok) {
            return false;
        }
        if let Some(slot) = seen.get_mut(p) {
            *slot = f.code();
        }
        p += 1;
        q += 1;
    }
    let mut r = 0usize;
    while r < seen.len() {
        let mut s2 = r + 1;
        while s2 < seen.len() {
            if let (Some(a), Some(b)) = (seen.get(r), seen.get(s2)) {
                if a == b {
                    return false;
                }
            }
            s2 += 1;
        }
        r += 1;
    }
    let mut ord = 0usize;
    while ord < JUDGE_ORDER.len() {
        match JUDGE_ORDER.get(ord) {
            Some(f) => {
                if !f.judged() || !f.alive() {
                    return false;
                }
            }
            None => return false,
        }
        ord += 1;
    }
    if Fault::Partial.judged() || Fault::Count.judged() || Fault::Bal.judged() {
        return false;
    }
    if Fault::Ok.alive() || !Fault::Estate.alive() {
        return false;
    }
    if fault_short(Fault::Ok) != "ok" || fault_short(Fault::Estate) != "estate" {
        return false;
    }
    if fault_short(Fault::Partial) != "partial" || fault_short(Fault::Bal) != "bal" {
        return false;
    }
    true
}

/// 设备几何：三块被测件的余量、块大小与满盘旗标。
fn geom_ok() -> bool {
    let gs = geoms();
    if gs.len() != 3 {
        return false;
    }
    let mut i = 0usize;
    while i < gs.len() {
        let g = match gs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if !g.well_formed() {
            return false;
        }
        if g.mount.is_empty() {
            return false;
        }
        if g.bytes_free().is_none() {
            return false;
        }
        if g.room_after_truncate() != g.bytes_free() {
            return false;
        }
        if g.is_full() != (g.blocks_free == 0) {
            return false;
        }
        let mut j = 0usize;
        while j < gs.len() {
            if i != j {
                if let (Some(a), Some(b)) = (gs.get(i), gs.get(j)) {
                    if a.mount == b.mount || a.seal() == b.seal() {
                        return false;
                    }
                }
            }
            j += 1;
        }
        i += 1;
    }
    // 三块的余量是本卡算术的地基：满、剩 6144、满但延迟分配。
    if let Some(g) = gs.get(0) {
        if g.bsize != 4096 || !g.is_full() || g.delalloc {
            return false;
        }
    } else {
        return false;
    }
    if let Some(g) = gs.get(1) {
        if g.bsize != 512 || g.blocks_free != 12 {
            return false;
        }
        match g.bytes_free() {
            Some(v) => {
                if v != 6144 {
                    return false;
                }
            }
            None => return false,
        }
        if g.delalloc {
            return false;
        }
    } else {
        return false;
    }
    if let Some(g) = gs.get(2) {
        if !g.delalloc || g.bsize != 1024 || !g.is_full() {
            return false;
        }
        if g.dst_existing == 0 {
            return false;
        }
    } else {
        return false;
    }
    // 越界查算：None 而不是回绕。
    if geom_room(3).is_some() {
        return false;
    }
    match geom_room(1) {
        Some(v) => {
            if v != 6144 {
                return false;
            }
        }
        None => return false,
    }
    if geom_room(0) != Some(0) {
        return false;
    }
    // 坏几何要能被 well_formed 抓住：块大小不在 {512,1024,4096} 一律不算。
    let bad = DevGeom { mount: "/mnt/weird", bsize: 3000, blocks_free: 1, delalloc: false, dst_existing: 0 };
    if bad.well_formed() {
        return false;
    }
    let anon = DevGeom::blank();
    if anon.well_formed() || anon.bytes_free() != Some(0) {
        return false;
    }
    true
}

/// 形状与合法域：14 槽形状全可判，家族规则与上界生效，克隆辅助名副其实。
fn shape_ok() -> bool {
    if !BASE.valid() {
        return false;
    }
    if Shape::blank().bs != 512 || Shape::blank().path != "disk.img" {
        return false;
    }
    if Shape::blank().flavor != Flavor::Gnu9 || Shape::blank().conv != 0 {
        return false;
    }
    if Shape::blank().eff_ibs() != 512 || Shape::blank().eff_obs() != 512 {
        return false;
    }
    if BASE.eff_ibs() != 4096 || BASE.eff_obs() != 4096 {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let s = match SHAPES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if !s.valid() {
            return false;
        }
        if s.tag.is_empty() {
            return false;
        }
        if s.eff_ibs() == 0 && !s.bad_option {
            return false;
        }
        i += 1;
    }
    let sc = scenes();
    if sc.len() != SCENE_COUNT {
        return false;
    }
    let mut w = 0usize;
    while w < sc.len() {
        let s = match sc.get(w) {
            Some(v) => *v,
            None => return false,
        };
        if !s.valid() {
            return false;
        }
        match SCENE_TAGS.get(w) {
            Some(t) => {
                if s.tag != *t {
                    return false;
                }
            }
            None => return false,
        }
        w += 1;
    }
    // bs 压制 ibs/obs；不给 bs 时 ibs/obs 各自生效。
    let split = Shape { bs: 0, ibs: 512, obs: 4096, ..BASE };
    if split.eff_ibs() != 512 || split.eff_obs() != 4096 {
        return false;
    }
    let both = Shape { bs: 1024, ibs: 512, obs: 4096, ..BASE };
    if both.eff_ibs() != 1024 || both.eff_obs() != 1024 {
        return false;
    }
    // 家族合法域：busybox 申报字节行 = 家族混认；GNU 有汇总却缺字节行 = 无从对账。
    let bb_lie = Shape { flavor: Flavor::Busybox, logged_bytes_known: true, logged_bytes: 6144, logged_in_full: 2, ..BASE };
    if bb_lie.valid() {
        return false;
    }
    let gnu_missing = Shape { logged_bytes_known: false, logged_in_full: 2, ..BASE };
    if gnu_missing.valid() {
        return false;
    }
    let silent_no_summary = Shape { status_none: true, logged_bytes_known: false, ..BASE };
    if !silent_no_summary.valid() {
        return false;
    }
    // 上界与非法 bs：bs=0 又不给 ibs/obs 是形状坏，不是 dd 的用法样本。
    let zero_bs = Shape { bs: 0, ibs: 0, obs: 0, ..BASE };
    if zero_bs.valid() {
        return false;
    }
    let huge = Shape { bs: BLOCK_MAX + 1, ..BASE };
    if huge.valid() {
        return false;
    }
    let over = Shape { free_bytes: BYTES_MAX + 1, ..BASE };
    if over.valid() {
        return false;
    }
    let bad_errno = Shape { errno_seen: 4242, ..BASE };
    if bad_errno.valid() || bad_errno.errno_legal() {
        return false;
    }
    let unknown_conv = Shape { conv: 0x40, operand: "conv=swab", ..BASE };
    if unknown_conv.unknown_conv_bits() == 0 {
        return false;
    }
    if unknown_conv.out_of_scope_bits() != 0 {
        return false;
    }
    let noerror = Shape { conv: CONV_NOERROR, ..BASE };
    if noerror.out_of_scope_bits() == 0 || noerror.unknown_conv_bits() != 0 {
        return false;
    }
    if !noerror.conv_has(CONV_NOERROR) || noerror.conv_has(CONV_FSYNC) {
        return false;
    }
    // 克隆辅助只改自己那一维。
    let base = match SHAPES.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let changed = base.clone_free(0);
    if changed.free_bytes != 0 || changed.tag != base.tag || changed.logged_out_part != base.logged_out_part {
        return false;
    }
    if base.clone_src(1024).src_bytes != 1024 || base.clone_src(1024).free_bytes != base.free_bytes {
        return false;
    }
    if base.clone_count(3).count != 3 || base.clone_count(3).src_bytes != base.src_bytes {
        return false;
    }
    let stacked = noerror.with_conv(CONV_FSYNC);
    if !stacked.conv_has(CONV_FSYNC) || !stacked.conv_has(CONV_NOERROR) {
        return false;
    }
    if base.with_conv(CONV_FSYNC).conv_has(CONV_NOERROR) {
        return false;
    }
    if base.with_flavor(Flavor::Busybox).logged_bytes_known {
        return false;
    }
    if !base.with_flavor(Flavor::Gnu8).logged_bytes_known {
        return false;
    }
    if base.with_flavor(Flavor::Gnu8).flavor != Flavor::Gnu8 {
        return false;
    }
    // 指纹：逐维敏感，且只改 tag 也要变。
    if base.seal() != base.seal() {
        return false;
    }
    let renamed = Shape { tag: "another", ..base };
    if renamed.seal() == base.seal() {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 判据基元（纵线、块账、越界、对判）
// ---------------------------------------------------------------------------

/// 一槽的实际结果是否与期望表逐维一致（六维全比）。
fn outcome_matches_want(i: usize) -> bool {
    let c = match case_at(i) {
        Some(v) => v,
        None => return false,
    };
    let o = c.actual();
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
    if o.stage != st || o.code != code || o.value != val || o.note != note || o.default != def {
        return false;
    }
    // 退出码必须落在 dd 的真实码集里（99 只给不裁定的行）。
    if o.code != EXIT_OK as i32
        && o.code != EXIT_FILE as i32
        && o.code != EXIT_USAGE as i32
        && o.code != EXIT_UNJUDGED as i32
    {
        return false;
    }
    // 锚位只能出现在 abort 闸的失败行上。
    if o.default && (o.stage != Stage::Abort || !o.note.abort_family()) {
        return false;
    }
    if !o.default && o.stage == Stage::Abort && o.note.abort_family() {
        // abort 闸但不亮锚：只允许 errno 串档（PairBroken 走的是 abort 闸）。
        if o.note != Note::PairBroken {
            return false;
        }
    }
    true
}

/// 一槽的块账是否与 `ACT_*` 表逐格一致。
fn blocks_match_act(i: usize) -> bool {
    let s = match SHAPES.get(i) {
        Some(v) => *v,
        None => return false,
    };
    let b = calc(&s);
    let want_in_full = match ACT_IN_FULL.get(i) { Some(v) => *v, None => return false };
    let want_in_part = match ACT_IN_PART.get(i) { Some(v) => *v, None => return false };
    let want_out_full = match ACT_OUT_FULL.get(i) { Some(v) => *v, None => return false };
    let want_out_part = match ACT_OUT_PART.get(i) { Some(v) => *v, None => return false };
    let want_landed = match ACT_LANDED.get(i) { Some(v) => *v, None => return false };
    let want_lost = match ACT_LOST.get(i) { Some(v) => *v, None => return false };
    let want_cut = match ACT_CUT.get(i) { Some(v) => *v, None => return false };
    if b.in_full != want_in_full || b.in_part != want_in_part {
        return false;
    }
    if b.out_full != want_out_full || b.out_part != want_out_part {
        return false;
    }
    if b.landed != want_landed || b.lost != want_lost || b.cut_bytes != want_cut {
        return false;
    }
    // 残块只可能是 0/1，且标记与尾字节必须同步。
    if b.in_part > 1 || b.out_part > 1 {
        return false;
    }
    if (b.out_part == 1) != (b.tail > 0) {
        return false;
    }
    if b.obs != 0 && b.tail >= b.obs {
        return false;
    }
    if b.ibs != 0 && b.in_part == 1 && b.tail_read() == 0 {
        return false;
    }
    if b.overflow {
        return false;
    }
    if b.write_failed != (b.lost > 0) {
        return false;
    }
    if b.all_landed() == (b.lost > 0) {
        return false;
    }
    // 恒等式一：读账 = 落盘 + 丢失（半块账的"钱"不会凭空多也不会凭空少）。
    if b.in_bytes != b.landed.saturating_add(b.lost) {
        return false;
    }
    // 恒等式二：读账 = 整块 + 残块尾（截断的字节不进读账）。
    if b.in_bytes != b.in_full.saturating_mul(b.ibs).saturating_add(b.tail_read()) {
        return false;
    }
    // 恒等式三：写账 = 整块 × obs + 尾。
    if b.landed != b.out_full.saturating_mul(b.obs).saturating_add(b.tail) {
        return false;
    }
    true
}

/// 九闸纵线：逐槽核对判定终点、闸序优先级与档位投影。
fn define_ok() -> bool {
    let mut i = 0usize;
    let mut anchored = 0usize;
    let mut replicas_anchored = 0usize;
    while i < CORPUS {
        if !outcome_matches_want(i) {
            return false;
        }
        if !blocks_match_act(i) {
            return false;
        }
        let c = match case_at(i) {
            Some(v) => v,
            None => return false,
        };
        let o = c.actual();
        if o.ok() == o.failed() {
            return false;
        }
        if o.refused() != (o.failed() && !o.default) {
            return false;
        }
        if Tier::of(&o) == Tier::Aborted && !o.anchored() {
            return false;
        }
        if o.anchored() {
            // 亮锚的四件齐：失败 + 退 1 + abort 闸 + 中止家族文案。
            if o.stage != Stage::Abort || !o.note.abort_family() || o.code != EXIT_FILE as i32 {
                return false;
            }
            // 复刻槽（9..13）与母槽同现场，锚位自然同行；只有 Estate 槽
            // （conv=noerror 的越界行）永不亮锚。
            if i == ACCEPT {
                return false;
            }
            if i >= ACCEPT {
                replicas_anchored += 1;
            } else {
                anchored += 1;
            }
        } else if i >= ACCEPT && i != ACCEPT {
            return false;
        }
        // 判定终点的秩就是走到的那道闸：不许"停在 abort 却申报走过 summary"。
        if o.stage.rank() != gate_stop_rank(&o) {
            return false;
        }
        if !o.stage.has_ledger() && o.value != 0 {
            return false;
        }
        i += 1;
    }
    if anchored != 3 {
        return false;
    }
    if replicas_anchored != REJECT - 1 {
        return false;
    }
    // 闸序优先级：args 压住一切，open 压住 read/write，read 压住 pair。
    let usage_over_scope = Shape { bad_option: true, operand: "ibs=abc", conv: CONV_NOERROR, ..BASE };
    if define(&usage_over_scope).note != Note::Usage {
        return false;
    }
    if define(&usage_over_scope).stage != Stage::Args {
        return false;
    }
    let scope_over_open = Shape { open_fail: true, conv: CONV_NOERROR, operand: "conv=noerror", ..BASE };
    let o1 = define(&scope_over_open);
    if o1.stage != Stage::Args || o1.note != Note::OutScope {
        return false;
    }
    let open_over_read = Shape { open_fail: true, read_fail: true, errno_seen: ERR_ENOENT, ..BASE };
    let o2 = define(&open_over_read);
    if o2.stage != Stage::Open || o2.note != Note::OpenErr {
        return false;
    }
    let read_over_pair = Shape { read_fail: true, free_bytes: 0, errno_seen: ERR_EIO, ..BASE };
    let o3 = define(&read_over_pair);
    if o3.stage != Stage::Read || o3.note != Note::ReadErr {
        return false;
    }
    // 满盘的现场：写真的红，但 errno 串了档 → abort 闸、不亮锚。
    let foreign_errno = Shape { free_bytes: 0, errno_seen: ERR_EACCES, ..BASE };
    let o4 = define(&foreign_errno);
    if o4.stage != Stage::Abort || o4.note != Note::PairBroken || o4.default {
        return false;
    }
    if Tier::of(&o4) != Tier::Refused {
        return false;
    }
    // status=none 把终点前移到 summary 闸，且不论成败都不亮锚。
    let silent_abort = Shape { status_none: true, errno_seen: ERR_ENOSPC, ..BASE };
    let o5 = define(&silent_abort);
    if o5.stage != Stage::Summary || o5.code != EXIT_FILE as i32 || o5.default {
        return false;
    }
    if Tier::of(&o5) != Tier::Refused {
        return false;
    }
    // 半块账谎报停在 records 闸，不进 abort 闸（对账失败的行没有落锚权）。
    let lie = Shape { logged_out_full: 2, errno_seen: ERR_ENOSPC, ..BASE };
    let o6 = define(&lie);
    if o6.stage != Stage::Records || o6.note != Note::RecordsLie || o6.default {
        return false;
    }
    // conv=fsync 的迟到爆：写侧账全绿，锚位仍亮（errno 与码都对得上）。
    let fsync = match scene_at(3) {
        Some(v) => v,
        None => return false,
    };
    let o7 = define(&fsync);
    if o7.stage != Stage::Abort || o7.note != Note::FsyncGnu || !o7.anchored() {
        return false;
    }
    // 七个终例逐维核对（九闸里 args/open/seek/read/records/summary/abort 各有终例）。
    let sc = scenes();
    let mut w = 0usize;
    while w < sc.len() {
        let s = match sc.get(w) {
            Some(v) => *v,
            None => return false,
        };
        let o = define(&s);
        let st = match SCENE_WANT_STAGE.get(w) {
            Some(v) => *v,
            None => return false,
        };
        let code = match SCENE_WANT_CODE.get(w) {
            Some(v) => *v,
            None => return false,
        };
        let val = match SCENE_WANT_VALUE.get(w) {
            Some(v) => *v,
            None => return false,
        };
        let note = match SCENE_WANT_NOTE.get(w) {
            Some(v) => *v,
            None => return false,
        };
        let def = match SCENE_WANT_DEFAULT.get(w) {
            Some(v) => *v,
            None => return false,
        };
        if o.stage != st || o.code != code || o.value != val || o.note != note || o.default != def {
            return false;
        }
        if scene_anchored(w) != Some(o.anchored()) {
            return false;
        }
        w += 1;
    }
    // seek 溢出：skip 大到乘法算不动，判定终点是 seek 闸。
    let overflow = Shape { skip: 0x8000_0000, ..BASE };
    let o8 = define(&overflow);
    if o8.stage != Stage::Seek || o8.note != Note::SeekErr || o8.value != 0 {
        return false;
    }
    // 一字节未碰的行不许带任何落盘字节。
    let mut z = 0usize;
    while z < SCENE_TAGS.len() {
        if let Some(t) = SCENE_TAGS.get(z) {
            if *t == "open-fail" {
                if let Some(s) = scene_at(z) {
                    if define(&s).value != 0 {
                        return false;
                    }
                }
            }
        }
        z += 1;
    }
    true
}

/// 块算术与半块账：records 三对数、partial 标记、skip/seek/count 的各自影响。
fn records_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        if !blocks_match_act(i) {
            return false;
        }
        i += 1;
    }
    // 块大小归零：几何算不动，给 overflow 而不是给 0 账。
    let zero = Shape { bs: 0, ibs: 0, obs: 0, ..BASE };
    if !calc(&zero).overflow {
        return false;
    }
    if !Blocks::zero().all_landed() {
        return false;
    }
    // bs=4096 与 ibs=4096 obs=4096 必须同账（bs 只是简写）。
    let via_bs = match SHAPES.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let via_pair = Shape { bs: 0, ibs: 4096, obs: 4096, ..via_bs };
    if calc(&via_bs).seal() != calc(&via_pair).seal() {
        return false;
    }
    // ibs!=obs：读账与写账天然不等，本卡不许把它们混成一格。
    let split = calc(&via_pair);
    if split.in_full != 2 || split.out_full != 1 || split.out_part != 1 || split.tail != 2048 {
        return false;
    }
    if split.in_text().as_str() != "2+0 records in" {
        return false;
    }
    if split.out_text().as_str() != "1+1 records out" {
        return false;
    }
    // skip 跨过的字节一次都不进读账：同尺寸加 skip=1 少一块。
    let skipped = Shape {
        skip: 1,
        logged_in_full: 1,
        logged_out_full: 1,
        logged_out_part: 0,
        logged_bytes: 4096,
        ..via_bs
    };
    let bs1 = calc(&skipped);
    if bs1.skipped_bytes != 4096 || bs1.in_full != 1 || bs1.in_bytes != 4096 {
        return false;
    }
    if bs1.landed != 4096 || bs1.lost != 0 {
        return false;
    }
    if define(&skipped).stage != Stage::Exit || define(&skipped).note != Note::Done {
        return false;
    }
    if !records_agree(&skipped, &bs1) {
        return false;
    }
    // skip 超过输入尺寸：读账空，写侧一字节不碰，但不是错误。
    let over_skip = Shape {
        skip: 8,
        logged_in_full: 0,
        logged_out_full: 0,
        logged_out_part: 0,
        logged_bytes: 0,
        ..via_bs
    };
    let bo = calc(&over_skip);
    if bo.in_full != 0 || bo.in_bytes != 0 || bo.landed != 0 || bo.lost != 0 {
        return false;
    }
    if define(&over_skip).value != 0 {
        return false;
    }
    if define(&over_skip).stage != Stage::Exit || define(&over_skip).code != EXIT_OK as i32 {
        return false;
    }
    // count 截断算 cut，绝不算 lost；落盘字节等于读账。
    let trunc = match SHAPES.get(5) {
        Some(v) => *v,
        None => return false,
    };
    let bt = calc(&trunc);
    if bt.cut_bytes != 2048 || bt.lost != 0 || bt.landed != 2048 {
        return false;
    }
    if define(&trunc).note != Note::DoneTrunc {
        return false;
    }
    // seek 造洞不吃余量：满盘也能 seek 成功，随后第一笔写整块失败。
    let hole = match scene_at(1) {
        Some(v) => v,
        None => return false,
    };
    let bh = calc(&hole);
    if bh.hole_bytes != 4096 || bh.landed != 0 || bh.out_part != 0 || bh.lost != 8192 {
        return false;
    }
    if bh.partial() {
        return false;
    }
    if define(&hole).note != Note::AbortGnu9 || !define(&hole).anchored() {
        return false;
    }
    // 落盘 = min(请求, 余量)：余量小于请求就红，等于请求就绿。
    let tight = Shape { src_bytes: 8192, free_bytes: 8192, errno_seen: ERR_NONE, logged_in_full: 2, logged_out_full: 2, logged_bytes: 8192, ..BASE };
    let btx = calc(&tight);
    if btx.landed != 8192 || btx.lost != 0 || btx.write_failed || !btx.all_landed() {
        return false;
    }
    if btx.out_full != 2 || btx.out_part != 0 {
        return false;
    }
    let short = Shape {
        free_bytes: 1,
        errno_seen: ERR_ENOSPC,
        logged_in_full: 2,
        logged_out_full: 0,
        logged_out_part: 1,
        logged_bytes: 1,
        logged_bytes_known: true,
        ..BASE
    };
    let bsh = calc(&short);
    if bsh.landed != 1 || bsh.out_full != 0 || bsh.out_part != 1 || bsh.tail != 1 {
        return false;
    }
    if !records_agree(&short, &bsh) {
        return false;
    }
    if define(&short).note != Note::AbortGnu9 {
        return false;
    }
    // records 闸：申报的每一格单独失真都要被抓住（五个可失格的逐一试）。
    let truth = match SHAPES.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let btruth = calc(&truth);
    if !records_agree(&truth, &btruth) {
        return false;
    }
    let wrong_in_full = Shape { logged_in_full: 3, ..truth };
    if records_agree(&wrong_in_full, &calc(&wrong_in_full)) {
        return false;
    }
    let wrong_in_part = Shape { logged_in_part: 1, ..truth };
    if records_agree(&wrong_in_part, &calc(&wrong_in_part)) {
        return false;
    }
    let wrong_out_full = Shape { logged_out_full: 2, ..truth };
    if records_agree(&wrong_out_full, &calc(&wrong_out_full)) {
        return false;
    }
    let wrong_out_part = Shape { logged_out_part: 0, ..truth };
    if records_agree(&wrong_out_part, &calc(&wrong_out_part)) {
        return false;
    }
    let wrong_bytes = Shape { logged_bytes: 8192, ..truth };
    if records_agree(&wrong_bytes, &calc(&wrong_bytes)) {
        return false;
    }
    let byte_absent = Shape { logged_bytes_known: false, ..truth };
    if records_agree(&byte_absent, &calc(&byte_absent)) {
        return false;
    }
    // 残块数只许 0/1：2 个"残块"是形状坏，判据不当它是对账失败。
    let two_parts = Shape { logged_out_part: 2, ..truth };
    if two_parts.logged_legible() || two_parts.valid() {
        return false;
    }
    // busybox 豁免：没有字节行才是对的，硬填字节反而失真。
    let bb = match SHAPES.get(2) {
        Some(v) => *v,
        None => return false,
    };
    let bbb = calc(&bb);
    if !records_agree(&bb, &bbb) {
        return false;
    }
    let bb_claiming = Shape { logged_bytes_known: true, logged_bytes: bbb.landed, ..bb };
    if records_agree(&bb_claiming, &calc(&bb_claiming)) {
        return false;
    }
    // 家族切换必须连带字节行的有无：这是 with_flavor 的语义，不是装饰。
    let to_gnu = bb.with_flavor(Flavor::Gnu9);
    if !to_gnu.logged_bytes_known || !to_gnu.flavor.prints_byte_line() {
        return false;
    }
    // 指纹对逐格敏感：尾字节改了 seal 必须变。
    let mut b2 = bbb;
    b2.tail = b2.tail.saturating_add(1);
    if b2.seal() == bbb.seal() {
        return false;
    }
    if Blocks::zero().seal() == bbb.seal() {
        return false;
    }
    true
}

/// 越界与邻居拒收：`conv=noerror`/`sync` 归 F11697，邻居 errno 串档不落锚。
fn scope_ok() -> bool {
    // 越界 conv 的两个位都要被拒（noerror 换掉中止轴，sync 换掉写账）。
    let bits = [CONV_NOERROR, CONV_SYNC];
    let mut seen = 0usize;
    while seen < bits.len() {
        let bit = match bits.get(seen) {
            Some(v) => *v,
            None => return false,
        };
        let s = Shape { conv: bit, operand: "conv=noerror", errno_seen: ERR_ENOSPC, ..BASE };
        let o = define(&s);
        if o.stage != Stage::Args || o.note != Note::OutScope {
            return false;
        }
        if o.code != EXIT_UNJUDGED as i32 || o.value != 0 || o.default {
            return false;
        }
        if o.anchored() || Tier::of(&o) != Tier::Refused {
            return false;
        }
        // 不裁定的行不许借用 dd 的真实退出码——99 是"本卡不收"的哨兵。
        if o.code == EXIT_OK as i32 || o.code == EXIT_FILE as i32 || o.code == EXIT_USAGE as i32 {
            return false;
        }
        if !msg_out_of_scope("conv=noerror").contains("F11697") {
            return false;
        }
        seen += 1;
    }
    // 同盘叠加：noerror 与 fsync 同时在场时仍按越界处理（中止轴已被换掉）。
    let mixed = Shape { conv: CONV_FSYNC | CONV_NOERROR, operand: "conv=noerror", ..BASE };
    if define(&mixed).note != Note::OutScope {
        return false;
    }
    // 本卡受理的 conv 位：notrunc / fsync / excl 都不换中止轴，不许误拒。
    let accepted = [CONV_NOTRUNC, CONV_FSYNC, CONV_EXCL];
    let mut k = 0usize;
    while k < accepted.len() {
        let bit = match accepted.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let s = Shape { conv: bit, errno_seen: ERR_ENOSPC, ..BASE };
        if s.out_of_scope_bits() != 0 {
            return false;
        }
        let o = define(&s);
        if o.note == Note::OutScope || o.stage == Stage::Args {
            return false;
        }
        k += 1;
    }
    // 未知 conv 值走用法错（不是越界）：dd 自己在 args 就退 2。
    let unknown = Shape { conv: 0x40, operand: "conv=swab", ..BASE };
    let ou = define(&unknown);
    if ou.stage != Stage::Args || ou.note != Note::Usage || ou.code != EXIT_USAGE as i32 {
        return false;
    }
    // 邻居 errno 冒充 ENOSPC：三档串档都要 abort 闸、不亮锚、Refused。
    let neighbors = [ERR_EIO, ERR_EACCES, ERR_EDQUOT];
    let mut n = 0usize;
    while n < neighbors.len() {
        let e = match neighbors.get(n) {
            Some(v) => *v,
            None => return false,
        };
        let s = Shape { errno_seen: e, logged_in_full: 2, logged_out_full: 1, logged_out_part: 1, logged_bytes: 6144, ..BASE };
        if !s.errno_legal() {
            return false;
        }
        let o = define(&s);
        if o.stage != Stage::Abort || o.note != Note::PairBroken || o.default {
            return false;
        }
        if o.anchored() || Tier::of(&o) != Tier::Refused {
            return false;
        }
        // 串档行的文案要落回 errno 自己（EIO 不能写成 No space left）。
        if e != ERR_ENOSPC && message(&s, &o).contains(ANCHOR_ERRNO_TEXT) {
            return false;
        }
        n += 1;
    }
    // ENOENT 只在 open 闸有意义：写侧红却挂着 ENOENT 也是串档。
    let stale_open = Shape { errno_seen: ERR_ENOENT, logged_in_full: 2, logged_out_full: 1, logged_out_part: 1, logged_bytes: 6144, ..BASE };
    if define(&stale_open).note != Note::PairBroken {
        return false;
    }
    // status=none 的行永不亮锚：全语料 + 全场景扫一遍。
    let mut i = 0usize;
    while i < CORPUS {
        let s = match SHAPES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let o = define(&s);
        if s.status_none {
            if o.stage != Stage::Summary {
                return false;
            }
            if o.default || o.anchored() {
                return false;
            }
        }
        if o.note == Note::Silent && !s.status_none {
            return false;
        }
        // 谎报与串档一律不亮锚；亮锚的行必须能报出真实文案家族。
        if (o.note == Note::RecordsLie || o.note == Note::PairBroken) && o.default {
            return false;
        }
        if o.anchored() && !o.note.abort_family() {
            return false;
        }
        i += 1;
    }
    let sc = scenes();
    let mut w = 0usize;
    while w < sc.len() {
        let s = match sc.get(w) {
            Some(v) => *v,
            None => return false,
        };
        let o = define(&s);
        if s.status_none && o.default {
            return false;
        }
        if s.read_fail && (o.stage != Stage::Read || o.default) {
            return false;
        }
        if s.open_fail && (o.stage != Stage::Open || o.default) {
            return false;
        }
        w += 1;
    }
    // 语料第 9 槽（estate-swap）就是那条被申报成活行的 noerror 记录。
    let estate = match case_at(ACCEPT) {
        Some(v) => v,
        None => return false,
    };
    if !estate.shape.conv_has(CONV_NOERROR) {
        return false;
    }
    if estate.actual().note != Note::OutScope {
        return false;
    }
    if !estate.claim.success() {
        return false;
    }
    if judge(&estate) != Fault::Estate {
        return false;
    }
    true
}

/// 申报对判：六维次序、越界处理、缺陷码映射。
fn judge_ok() -> bool {
    let cs = cases();
    if cs.len() != CORPUS {
        return false;
    }
    let mut i = 0usize;
    let mut goods = 0usize;
    let mut bads = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want = match expected(i) {
            Some(v) => v,
            None => return false,
        };
        let f = judge(&c);
        if f != want {
            return false;
        }
        if c.ok() != (f == Fault::Ok) {
            return false;
        }
        if f == Fault::Ok {
            goods += 1;
            // 申报正确的槽：六维逐一对得上。
            let o = c.actual();
            if o.stage != c.claim.stage
                || o.code != c.claim.code
                || o.value != c.claim.value
                || o.note != c.claim.note
                || o.default != c.claim.default
            {
                return false;
            }
            // 申报正确的槽还要经得起结构复核：能走到对账闸的行，日志申报
            // 必须与块几何逐格一致，判定字节必须等于块账落盘字节。
            let b = c.blocks();
            if o.stage.has_ledger() && !c.shape.status_none && !records_agree(&c.shape, &b) {
                return false;
            }
            if o.stage.has_ledger() && o.value != b.landed {
                return false;
            }
        } else {
            bads += 1;
            if !f.judged() || !f.alive() {
                return false;
            }
            if f.code() == 0 {
                return false;
            }
        }
        if i >= ACCEPT {
            let r = match REJECT_SLOTS.get(i - ACCEPT) {
                Some(v) => *v,
                None => return false,
            };
            if r != i {
                return false;
            }
            let code = match REJECT_CODES.get(i - ACCEPT) {
                Some(v) => *v,
                None => return false,
            };
            if f.code() != code {
                return false;
            }
            let ord = match JUDGE_ORDER.get(i - ACCEPT) {
                Some(v) => *v,
                None => return false,
            };
            if f != ord || ord != want {
                return false;
            }
        }
        i += 1;
    }
    if goods != ACCEPT || bads != REJECT {
        return false;
    }
    if cover(CORPUS) != Ok(()) || cover(CORPUS - 1) != Err(DD_ENOBS) {
        return false;
    }
    if case_at(CORPUS).is_some() {
        return false;
    }
    if expected(CORPUS).is_some() {
        return false;
    }
    if expected(ACCEPT - 1) != Some(Fault::Ok) {
        return false;
    }
    // 次序优先级：两维同时失真时，只有判序里在前的那一维被报出来。
    let base = match cs.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let two_stage_code = Case {
        shape: base.shape,
        claim: Claim::make(Stage::Write, EXIT_OK as i32, 6144, Note::AbortGnu9, true),
    };
    if judge(&two_stage_code) != Fault::Stage {
        return false;
    }
    let two_code_value = Case {
        shape: base.shape,
        claim: Claim::make(Stage::Abort, EXIT_OK as i32, 8192, Note::AbortGnu9, true),
    };
    if judge(&two_code_value) != Fault::Code {
        return false;
    }
    let two_value_note = Case {
        shape: base.shape,
        claim: Claim::make(Stage::Abort, EXIT_FILE as i32, 8192, Note::AbortBb, true),
    };
    if judge(&two_value_note) != Fault::Value {
        return false;
    }
    let two_note_default = Case {
        shape: base.shape,
        claim: Claim::make(Stage::Abort, EXIT_FILE as i32, 6144, Note::AbortBb, false),
    };
    if judge(&two_note_default) != Fault::Note {
        return false;
    }
    // 成败互报优先于其余五维：连阶段都报对了也不许躲过 Estate。
    let estate_masked = Case {
        shape: base.shape,
        claim: Claim::make(Stage::Abort, EXIT_OK as i32, 6144, Note::DonePartial, true),
    };
    if judge(&estate_masked) != Fault::Estate {
        return false;
    }
    // 反向互报：现场绿、申报红，同样落 Estate（判据不许只盯一个方向）。
    let green = match cs.get(3) {
        Some(v) => *v,
        None => return false,
    };
    let green_claim_red = Case {
        shape: green.shape,
        claim: Claim::make(Stage::Abort, EXIT_FILE as i32, 1024, Note::AbortGnu9, true),
    };
    if judge(&green_claim_red) != Fault::Estate {
        return false;
    }
    if green_claim_red.claim.success() {
        return false;
    }
    // 六个缺陷码互不相同，且都属于本段。
    let mut p = 0usize;
    while p < REJECT_CODES.len() {
        let a = match REJECT_CODES.get(p) {
            Some(v) => *v,
            None => return false,
        };
        if a <= SEG_BASE || a > SEG_BASE + 0x0d {
            return false;
        }
        let mut q = p + 1;
        while q < REJECT_CODES.len() {
            if let Some(b) = REJECT_CODES.get(q) {
                if a == *b {
                    return false;
                }
            }
            q += 1;
        }
        p += 1;
    }
    if REJECT_FAULTS.len() != JUDGE_ORDER.len() {
        return false;
    }
    let mut m = 0usize;
    while m < JUDGE_ORDER.len() {
        if let (Some(x), Some(y)) = (JUDGE_ORDER.get(m), REJECT_FAULTS.get(m)) {
            if x != y {
                return false;
            }
        }
        m += 1;
    }
    if Fault::Ok.code() != 0 {
        return false;
    }
    if fault_short(Fault::Estate) != "estate" {
        return false;
    }
    // 用例指纹对申报敏感：改一维就变；同形复刻的两槽靠申报区分，不靠形状。
    let replay0 = match case_at(0) {
        Some(v) => v,
        None => return false,
    };
    if base.seal() != replay0.seal() {
        return false;
    }
    let tampered = Case { shape: base.shape, claim: Claim::make(Stage::Abort, EXIT_FILE as i32, 6144, Note::AbortGnu9, false) };
    if tampered.seal() == base.seal() {
        return false;
    }
    let other = match case_at(9) {
        Some(v) => v,
        None => return false,
    };
    if calc(&other.shape).seal() != calc(&base.shape).seal() {
        // 第 10 槽复刻第 1 槽的现场：块账必须一模一样，差别只允许在申报上。
        return false;
    }
    if other.actual().seal() != base.actual().seal() {
        return false;
    }
    if other.seal() == base.seal() {
        return false;
    }
    true
}

/// 矩阵分档与渲染：覆盖面（total）与可信度（good）两维独立。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    if m.slots() != CORPUS {
        return false;
    }
    if m.goods() != ACCEPT {
        return false;
    }
    if !m.covered() {
        return false;
    }
    // 四档的实际分布：3 完整 / 1 无汇总 / 8 中止 / 2 拒绝。
    let want_total = [3usize, 1usize, 8usize, 2usize];
    let want_good = [3usize, 1usize, 3usize, 1usize];
    let tiers = [Tier::Complete, Tier::Silent, Tier::Aborted, Tier::Refused];
    let mut i = 0usize;
    while i < tiers.len() {
        let t = match tiers.get(i) {
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
        if r.total != wt || r.good != wg {
            return false;
        }
        if r.red() != wt.saturating_sub(wg) {
            return false;
        }
        if r.perfect() != (wt == wg) {
            return false;
        }
        if !r.render(t).contains(t.name()) {
            return false;
        }
        i += 1;
    }
    if m.perfect_rows() != 2 {
        return false;
    }
    // 中止档红 5 格：6 个死槽里 5 个复刻同一现场，覆盖面看着满、申报有水分。
    if let Some(r) = m.row(Tier::Aborted) {
        if r.red() != 5 {
            return false;
        }
        if r.total != m.slots() - 6 {
            return false;
        }
    } else {
        return false;
    }
    let rendered = m.render();
    let want = "complete total=3 good=3 red=0\nsilent total=1 good=1 red=0\naborted total=8 good=3 red=5\nrefused total=2 good=1 red=1\n";
    if rendered.as_str() != want {
        return false;
    }
    if rendered.chars().filter(|c| *c == '\n').count() != TIER_COUNT {
        return false;
    }
    // 空矩阵：无覆盖、无全对档，slots 归零。
    let e = Matrix::blank();
    if e.slots() != 0 || e.goods() != 0 || e.perfect_rows() != 0 || e.covered() {
        return false;
    }
    // 空矩阵不是空字符串：四档照样各占一行，只是全零——覆盖面与可信度分开记账。
    let blank_render = e.render();
    let want_blank = "complete total=0 good=0 red=0\nsilent total=0 good=0 red=0\naborted total=0 good=0 red=0\nrefused total=0 good=0 red=0\n";
    if blank_render.as_str() != want_blank {
        return false;
    }
    if blank_render.chars().filter(|c| *c == '\n').count() != TIER_COUNT {
        return false;
    }
    if let Some(r) = e.row(Tier::Complete) {
        if r.total != 0 || r.red() != 0 || r.perfect() {
            return false;
        }
    } else {
        return false;
    }
    // Row::red 的防呆：good 大于 total 时不得回绕成天文数字。
    let broken = Row { total: 2, good: 5 };
    if broken.red() != 2 {
        return false;
    }
    if broken.perfect() {
        return false;
    }
    true
}

/// 语料三陈与全绿渲染：形状、申报、期望表互相对得上。
fn corpus_ok() -> bool {
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS {
        return false;
    }
    if WANT_STAGE.len() != CORPUS
        || WANT_CODE.len() != CORPUS
        || WANT_VALUE.len() != CORPUS
        || WANT_NOTE.len() != CORPUS
        || WANT_DEFAULT.len() != CORPUS
        || WANT_NDX.len() != CORPUS
        || WANT_TAGS.len() != CORPUS
    {
        return false;
    }
    if ACT_IN_FULL.len() != CORPUS
        || ACT_IN_PART.len() != CORPUS
        || ACT_OUT_FULL.len() != CORPUS
        || ACT_OUT_PART.len() != CORPUS
        || ACT_LANDED.len() != CORPUS
        || ACT_LOST.len() != CORPUS
        || ACT_CUT.len() != CORPUS
    {
        return false;
    }
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let tag = match WANT_TAGS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.tag() != tag || c.shape.tag != tag {
            return false;
        }
        // 标签唯一（14 槽两两不同）。
        let mut j = i + 1;
        while j < CORPUS {
            if let Some(o) = WANT_TAGS.get(j) {
                if *o == tag {
                    return false;
                }
            }
            if let (Some(a), Some(b)) = (cs.get(i), cs.get(j)) {
                if a.tag() == b.tag() {
                    return false;
                }
            }
            j += 1;
        }
        // 申报表与期望表的第二形态必须逐维相等。
        if let Some(st) = WANT_STAGE.get(i) {
            if c.claim.stage != *st && i < ACCEPT {
                return false;
            }
            if c.actual().stage != *st {
                return false;
            }
        }
        if let Some(code) = WANT_CODE.get(i) {
            if c.actual().code != *code {
                return false;
            }
            if i < ACCEPT && c.claim.code != *code {
                return false;
            }
        }
        if let Some(v) = WANT_VALUE.get(i) {
            if c.actual().value != *v {
                return false;
            }
            if i < ACCEPT && c.claim.value != *v {
                return false;
            }
        }
        if let Some(n) = WANT_NOTE.get(i) {
            if c.actual().note != *n {
                return false;
            }
            if i < ACCEPT && c.claim.note != *n {
                return false;
            }
        }
        if let Some(d) = WANT_DEFAULT.get(i) {
            if c.actual().default != *d {
                return false;
            }
            if i < ACCEPT && c.claim.default != *d {
                return false;
            }
        }
        i += 1;
    }
    // 报表与逐字钉版必须完全一致。
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if r.render().as_str() != RENDER_OK {
        return false;
    }
    if !r.green() || r.rate() != RATE_FULL {
        return false;
    }
    if r.gate() != Ok(()) {
        return false;
    }
    if r.success_cells() != 4 || r.failure_cells() != 4 {
        return false;
    }
    if r.total != CORPUS as u32 || r.accept != ACCEPT as u32 || r.reject != REJECT as u32 {
        return false;
    }
    if r.complete != 3 || r.silent != 1 || r.aborted != 3 || r.refused != 1 {
        return false;
    }
    if r.complete + r.silent + r.aborted + r.refused != r.accept {
        return false;
    }
    if r.matched != r.total {
        return false;
    }
    if !RENDER_OK.contains("dd-records corpus=14") {
        return false;
    }
    true
}

/// 槽位与复刻纪律：8 活 6 死的下标、复刻形状、缺陷秩与标签三处一致。
fn slots_ok() -> bool {
    if ACCEPT_SLOTS.len() != ACCEPT || REJECT_SLOTS.len() != REJECT {
        return false;
    }
    let mut i = 0usize;
    while i < ACCEPT {
        match ACCEPT_SLOTS.get(i) {
            Some(v) => {
                if *v != i || *v >= CORPUS {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < REJECT {
        match REJECT_SLOTS.get(j) {
            Some(v) => {
                if *v != ACCEPT + j || *v >= CORPUS {
                    return false;
                }
            }
            None => return false,
        }
        j += 1;
    }
    // 两不重不漏。
    let mut seen = [0u8; CORPUS];
    let mut k = 0usize;
    while k < ACCEPT {
        if let Some(v) = ACCEPT_SLOTS.get(k) {
            if let Some(cell) = seen.get_mut(*v) {
                if *cell != 0 {
                    return false;
                }
                *cell = 1;
            }
        }
        k += 1;
    }
    let mut w = 0usize;
    while w < REJECT {
        if let Some(v) = REJECT_SLOTS.get(w) {
            if let Some(cell) = seen.get_mut(*v) {
                if *cell != 0 {
                    return false;
                }
                *cell = 2;
            }
        }
        w += 1;
    }
    let mut z = 0usize;
    while z < CORPUS {
        match seen.get(z) {
            Some(0) | None => return false,
            _ => {}
        }
        z += 1;
    }
    // 死槽的标签必须是 `<缺陷短名>-swap`，与 JUDGE_ORDER 一一对应。
    let mut p = 0usize;
    while p < REJECT {
        let f = match REJECT_FAULTS.get(p) {
            Some(v) => *v,
            None => return false,
        };
        let tag = match WANT_TAGS.get(ACCEPT + p) {
            Some(v) => *v,
            None => return false,
        };
        let mut want = String::from(fault_short(f));
        want.push_str("-swap");
        if want.as_str() != tag {
            return false;
        }
        let ndx = match WANT_NDX.get(ACCEPT + p) {
            Some(v) => *v,
            None => return false,
        };
        if ndx != (p as u8) + 1 {
            return false;
        }
        p += 1;
    }
    let mut q = 0usize;
    while q < ACCEPT {
        match WANT_NDX.get(q) {
            Some(0) => {}
            _ => return false,
        }
        q += 1;
    }
    // 5 个复刻槽去掉标签后与第 1 槽同形；estate 槽的差别只在 conv。
    let first = match SHAPES.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let first_plain = Shape { tag: "", ..first };
    let mut m2 = 1usize;
    while m2 < REJECT {
        let s = match SHAPES.get(ACCEPT + m2) {
            Some(v) => *v,
            None => return false,
        };
        let plain = Shape { tag: "", ..s };
        if plain.seal() != first_plain.seal() {
            return false;
        }
        m2 += 1;
    }
    let estate = match SHAPES.get(ACCEPT) {
        Some(v) => *v,
        None => return false,
    };
    if !estate.conv_has(CONV_NOERROR) {
        return false;
    }
    if estate.out_of_scope_bits() == 0 {
        return false;
    }
    if first.conv != 0 {
        return false;
    }
    // 复刻槽的实际结果必须与第 1 槽一致（差别只在申报）。
    let a0 = first.actual_seal();
    let mut n2 = 1usize;
    while n2 < REJECT {
        let s = match SHAPES.get(ACCEPT + n2) {
            Some(v) => *v,
            None => return false,
        };
        if s.actual_seal() != a0 {
            return false;
        }
        n2 += 1;
    }
    if estate.actual_seal() == a0 {
        return false;
    }
    // 活槽的申报与实际必须同形；死槽的申报必须与实际不同形。
    let mut f2 = 0usize;
    while f2 < CORPUS {
        let c = match case_at(f2) {
            Some(v) => v,
            None => return false,
        };
        let same = c.actual().seal() == c.claim.seal();
        if f2 < ACCEPT && !same {
            return false;
        }
        if f2 >= ACCEPT && same {
            return false;
        }
        f2 += 1;
    }
    true
}

/// 红档显式：注入后 matched 与 rate 必须离开全绿，越界槽必须报错。
fn reds_ok() -> bool {
    if VALS.len() != ACCEPT {
        return false;
    }
    let rate = match VALS.get(ACCEPT - 1) {
        Some(v) => *v,
        None => return false,
    };
    if rate != RED_RATE || RED_RATE != 929 {
        return false;
    }
    let clean = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if clean.render().as_str() != RENDER_OK {
        return false;
    }
    let red = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if red.matched != CORPUS as u32 - 1 || red.rate() != RED_RATE {
        return false;
    }
    if red.green() || red.gate() == Ok(()) {
        return false;
    }
    if red.gate() != Err(DD_ECOUNT) {
        return false;
    }
    let want = "dd-records corpus=14 accept=7 reject=7 complete=3 silent=1 aborted=2 refused=1 match=13 rate=929ppm\n";
    if red.render().as_str() != want {
        return false;
    }
    if red.accept != ACCEPT as u32 - 1 || red.reject != REJECT as u32 + 1 {
        return false;
    }
    // 注入不改实际结果：档位只能从"申报正确"的行里计，所以 aborted 掉 1。
    if red.aborted + red.complete + red.silent + red.refused != red.accept {
        return false;
    }
    if clean.aborted == red.aborted {
        return false;
    }
    // 槽位越界：给 ESLOT，不静默回绕。
    if run_with(Some((CORPUS, Fault::Code))).err() != Some(DD_ESLOT) {
        return false;
    }
    if run_with(Some((CORPUS + 8, Fault::Ok))).err() != Some(DD_ESLOT) {
        return false;
    }
    // 每个死槽强行改判都会掉一格 matched（期望表真的在被测面里）。
    let mut i = 0usize;
    while i < REJECT {
        let slot = ACCEPT + i;
        let r = match run_with(Some((slot, Fault::Partial))) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if r.matched != CORPUS as u32 - 1 {
            return false;
        }
        if r.rate() != RED_RATE {
            return false;
        }
        i += 1;
    }
    // 活槽被强行判缺陷同样掉档；活槽改判成另一种缺陷（仍非 Ok）也掉。
    let mut j = 0usize;
    while j < ACCEPT {
        let r = match run_with(Some((j, Fault::Seal))) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if r.matched != CORPUS as u32 - 1 {
            return false;
        }
        if r.accept != ACCEPT as u32 - 1 {
            return false;
        }
        j += 1;
    }
    // 注入 Fault::Ok 到活槽是恒等操作：matched 不动。
    let same = match run_with(Some((3, Fault::Ok))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if same.matched != clean.matched || same.render().as_str() != RENDER_OK {
        return false;
    }
    // rate 的取整：13/14 → 929，14/14 → 1000，0/14 → 0。
    let zero = Report { total: 14, accept: 0, reject: 14, complete: 0, silent: 0, aborted: 0, refused: 0, matched: 0 };
    if zero.rate() != 0 || zero.green() || zero.gate() != Err(DD_ECOUNT) {
        return false;
    }
    if zero.success_cells() != 0 || zero.failure_cells() != 0 {
        return false;
    }
    let odd = Report { total: 3, accept: 1, reject: 2, complete: 1, silent: 0, aborted: 0, refused: 0, matched: 2 };
    if odd.rate() != 667 {
        return false;
    }
    let none = Report { total: 0, accept: 0, reject: 0, complete: 0, silent: 0, aborted: 0, refused: 0, matched: 0 };
    if none.rate() != 0 {
        return false;
    }
    true
}

/// 文案逐字与家族分表：三式原文、partial 标记、复原字节行、越界点名。
fn message_ok() -> bool {
    // 三式的中止原文必须逐字不同，且都带同一个 errno 文本。
    let gnu8 = msg_gnu8_writing("disk.img", ERR_ENOSPC);
    let gnu9 = msg_gnu9_error_writing("disk.img", ERR_ENOSPC);
    let bb = msg_bb_write_error("disk.img", ERR_ENOSPC);
    if gnu8.as_str() != "dd: writing 'disk.img': No space left on device" {
        return false;
    }
    if gnu9.as_str() != "dd: error writing 'disk.img': No space left on device" {
        return false;
    }
    if bb.as_str() != "dd: write error to 'disk.img': No space left on device" {
        return false;
    }
    if gnu8 == gnu9 || gnu8 == bb || gnu9 == bb {
        return false;
    }
    if Flavor::Gnu8.write_error_text("x", ERR_ENOSPC) != msg_gnu8_writing("x", ERR_ENOSPC) {
        return false;
    }
    if Flavor::Busybox.write_error_text("x", ERR_ENOSPC) != msg_bb_write_error("x", ERR_ENOSPC) {
        return false;
    }
    // 家族名与字节行的有无。
    if Flavor::Gnu8.name() != "gnu8" || Flavor::Gnu9.name() != "gnu9" {
        return false;
    }
    if Flavor::Busybox.name() != "busybox" || Flavor::Busybox.gnu() {
        return false;
    }
    if Flavor::ALL.len() != 3 {
        return false;
    }
    // 其余文案：读、打开、fsync、用法、未知选项、seek。
    if msg_gnu_reading("in.img", ERR_EIO, true).as_str() != "dd: error reading 'in.img': Input/output error"
    {
        return false;
    }
    if msg_gnu_reading("in.img", ERR_EIO, false).as_str() != "dd: reading 'in.img': Input/output error" {
        return false;
    }
    if msg_failed_to_open("disk.img", ERR_ENOENT).as_str()
        != "dd: failed to open 'disk.img': No such file or directory"
    {
        return false;
    }
    if msg_fsync_failed("disk.img", ERR_ENOSPC).as_str()
        != "dd: fsync failed for 'disk.img': No space left on device"
    {
        return false;
    }
    if msg_invalid_option('z').as_str() != "dd: invalid option -- 'z'" {
        return false;
    }
    if msg_cannot_seek("disk.img").as_str() != "dd: 'disk.img': cannot seek: Invalid argument" {
        return false;
    }
    // 汇总三行与字节行的 SI/IEC 双括号。
    if msg_records_in(2, 0).as_str() != "2+0 records in" {
        return false;
    }
    if msg_records_out(1, 1).as_str() != "1+1 records out" {
        return false;
    }
    if msg_bytes_copied(6144).as_str() != "6144 bytes (6.1 kB, 6.0 KiB) copied" {
        return false;
    }
    if msg_bytes_copied(0).as_str() != "0 bytes (0 B) copied" {
        return false;
    }
    if human_pair(1536) != "1.6 kB, 1.5 KiB" {
        return false;
    }
    if human_pair(6144) != "6.1 kB, 6.0 KiB" || human_pair(0) != "0 B" {
        return false;
    }
    // 表外的字节数给人 `n/a`：人类可读尾巴不进判据，判据看的是字节数本身。
    if human_pair(1537) != "n/a" || human_tail_judged() {
        return false;
    }
    // partial-block 标记与本卡自研的两条审计句。
    if msg_partial_marker(1, 1, 2048).as_str() != "partial block: records out 1+1, tail bytes 2048" {
        return false;
    }
    if msg_records_lie("1+1 records out", "2+0 records out").as_str()
        != "dd records lie: log says 2+0 records out but geometry gives 1+1 records out"
    {
        return false;
    }
    if msg_pair_broken(-1, ERR_EIO).as_str() != "dd write/errno pair broken (ret=-1, errno=5)" {
        return false;
    }
    if !msg_pair_broken(-1, ERR_ENOSPC).contains("errno=28") {
        return false;
    }
    if msg_out_of_scope("conv=noerror").as_str()
        != "dd conv out of scope: conv=noerror belongs to UNX-F11697"
    {
        return false;
    }
    if msg_summary_suppressed().as_str()
        != "dd status=none: records summary suppressed, anchor withheld"
    {
        return false;
    }
    if msg_bytes_recovered("disk.img", 3584).as_str()
        != "dd byte line absent for 'disk.img': recovered 3584 bytes from block geometry"
    {
        return false;
    }
    // 逐槽现场文案（九闸的每一格都在这里被逐字钉一次）。
    let wants = [
        "dd: error writing 'disk.img': No space left on device | partial block: records out 1+1, tail bytes 2048 | 2+0 records in | 1+1 records out | 6144 bytes (6.1 kB, 6.0 KiB) copied",
        "dd: writing 'disk.img': No space left on device | 8+0 records in | 4+0 records out | 2048 bytes (2.0 kB, 2.0 KiB) copied",
        "dd: write error to 'disk.img': No space left on device | partial block: records out 3+1, tail bytes 512 | 4+0 records in | 3+1 records out | dd byte line absent for 'disk.img': recovered 3584 bytes from block geometry",
        "1+0 records in | 1+0 records out | 1024 bytes (1.0 kB, 1.0 KiB) copied",
        "3+0 records in | 1+1 records out | 3072 bytes (3.1 kB, 3.0 KiB) copied",
        "4+0 records in | 4+0 records out | 2048 bytes (2.0 kB, 2.0 KiB) copied",
        "dd status=none: records summary suppressed, anchor withheld",
        "dd: unrecognized operand 'ibs=abc'",
        "dd conv out of scope: conv=noerror belongs to UNX-F11697",
    ];
    let mut i = 0usize;
    while i < wants.len() {
        let c = match case_at(i) {
            Some(v) => v,
            None => return false,
        };
        let want = match wants.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if message(&c.shape, &c.actual()).as_str() != want {
            return false;
        }
        i += 1;
    }
    let scene_wants = [
        "dd: failed to open 'disk.img': No such file or directory",
        "dd: error writing 'disk.img': No space left on device | 2+0 records in | 0+0 records out | 0 bytes (0 B) copied",
        "dd: error reading 'disk.img': Input/output error",
        "dd: fsync failed for 'disk.img': No space left on device | 2+0 records in | 2+0 records out | 2048 bytes (2.0 kB, 2.0 KiB) copied",
        "dd records lie: log says 2+0 records out but geometry gives 1+1 records out",
        "dd write/errno pair broken (ret=-1, errno=5)",
        "dd status=none: records summary suppressed, anchor withheld",
    ];
    let mut w = 0usize;
    while w < scene_wants.len() {
        let s = match scene_at(w) {
            Some(v) => v,
            None => return false,
        };
        let want = match scene_wants.get(w) {
            Some(v) => *v,
            None => return false,
        };
        if message(&s, &define(&s)).as_str() != want {
            return false;
        }
        w += 1;
    }
    // 中止家族的行必须报出 errno 原文；成功行不许出现错误句式。
    let mut a = 0usize;
    while a < CORPUS {
        let c = match case_at(a) {
            Some(v) => v,
            None => return false,
        };
        let o = c.actual();
        let m = message(&c.shape, &o);
        if o.note.abort_family() && !m.contains(ANCHOR_ERRNO_TEXT) {
            return false;
        }
        if o.note.success() && o.stage == Stage::Exit && m.contains("dd: ") {
            return false;
        }
        // 计时与速率尾巴不进判据。
        if m.contains("second") || m.contains("/s") {
            return false;
        }
        // partial 标记只在有残块的中止行上出现。
        let has_marker = m.contains("partial block:");
        if has_marker && !(o.note.wants_partial_marker() && calc(&c.shape).partial()) {
            return false;
        }
        a += 1;
    }
    true
}

/// 释义行：逐槽一行、格式固定、字节只在写之后给、锚位文本与判据同源。
fn detail_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    let mut anchored_lines = 0usize;
    let mut seen = String::new();
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let line = detail(&c);
        if !line.ends_with('\n') {
            return false;
        }
        // 释义行只由**实际结果**生成：8 个活槽各说一件事，行行不同。
        if i < ACCEPT && seen.contains(line.as_str()) {
            return false;
        }
        seen.push_str(line.as_str());
        let o = c.actual();
        let frags = [
            c.shape.path,
            " flavor=",
            c.shape.flavor.name(),
            " stage=",
            o.stage.name(),
            " exit=",
            " bytes=",
            " partial=",
            " anchor=",
            " note=",
            o.note.name(),
            " msg=\"",
        ];
        let mut f = 0usize;
        while f < frags.len() {
            let frag = match frags.get(f) {
                Some(v) => *v,
                None => return false,
            };
            if !line.contains(frag) {
                return false;
            }
            f += 1;
        }
        if line.contains("msg=\"\"") {
            return false;
        }
        if !o.stage.has_ledger() {
            if !line.contains(" bytes=0 ") || !line.contains(" partial=false ") {
                return false;
            }
        }
        if o.default {
            anchored_lines += 1;
            if !line.contains("anchor=true") {
                return false;
            }
        } else if line.contains("anchor=true") {
            return false;
        }
        i += 1;
    }
    if seen.chars().filter(|c| *c == '\n').count() != CORPUS {
        return false;
    }
    // 8 个活槽里恰好 3 行亮锚（三式中止），复刻的 5 行同样亮锚但不计入活槽。
    let mut live_anchor = 0usize;
    let mut k = 0usize;
    while k < ACCEPT {
        if let Some(c) = cs.get(k) {
            if c.actual().default {
                live_anchor += 1;
            }
        }
        k += 1;
    }
    if live_anchor != 3 || anchored_lines != 8 {
        return false;
    }
    // 第一行的逐字形态（文档头那段现场的释义陈述）。
    let first = match case_at(0) {
        Some(v) => v,
        None => return false,
    };
    let head = detail(&first);
    let want_head = "disk.img flavor=gnu9 stage=abort exit=1 bytes=6144 partial=true anchor=true note=abort-gnu9 msg=\"dd: error writing 'disk.img': No space left on device | partial block: records out 1+1, tail bytes 2048 | 2+0 records in | 1+1 records out | 6144 bytes (6.1 kB, 6.0 KiB) copied\"\n";
    if head.as_str() != want_head {
        return false;
    }
    // 5 个复刻槽（stage/code/value/note/default）的释义与母槽**逐字**相同：
    // 它们的现场就是母槽的现场，错只错在申报。Estate 槽换了现场，必须不同行。
    let mut rep = ACCEPT + 1;
    while rep < CORPUS {
        let rc = match cs.get(rep) {
            Some(v) => v,
            None => return false,
        };
        if detail(&rc).as_str() != head.as_str() {
            return false;
        }
        rep += 1;
    }
    if let Some(ec) = cs.get(ACCEPT) {
        if detail(&ec).as_str() == head.as_str() {
            return false;
        }
    } else {
        return false;
    }
    // busybox 行：家族名进释义，字节行走复原口径。
    let bbline = match case_at(2) {
        Some(v) => v,
        None => return false,
    };
    let bl = detail(&bbline);
    if !bl.contains("flavor=busybox") || !bl.contains("recovered 3584 bytes") {
        return false;
    }
    if !bl.contains("note=abort-busybox") {
        return false;
    }
    // 无汇总行：anchor=false 且 msg 是被压掉的声明。
    let silent = match case_at(6) {
        Some(v) => v,
        None => return false,
    };
    let sl = detail(&silent);
    if !sl.contains("stage=summary") || !sl.contains("anchor=false") {
        return false;
    }
    if !sl.contains("bytes=2048") {
        return false;
    }
    // 用法错的行一字节未碰，块账不进释义。
    let usage = match case_at(7) {
        Some(v) => v,
        None => return false,
    };
    let ul = detail(&usage);
    if !ul.contains("stage=args") || !ul.contains("exit=2") || !ul.contains("bytes=0") {
        return false;
    }
    true
}

/// 阶段闸门：秩从 1 起、只许前进一格、封印必须在最后一闸、封后皆拒。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    if g.rank() != 0 || g.moves() != 0 || g.rejected() != 0 || g.is_closed() {
        return false;
    }
    if g.stage().is_some() {
        return false;
    }
    // 起跑必须从 args（秩 1）开始：直接跳 abort 是本卡最典型的伪造路径。
    if g.step(Stage::Abort) != Err(DD_EBAL) || g.rank() != 0 || g.rejected() != 1 {
        return false;
    }
    if g.step(Stage::Args) != Ok(()) || g.rank() != 1 {
        return false;
    }
    if g.stage() != Some(Stage::Args) {
        return false;
    }
    // 原地打转（同秩重复）按跳档记，不推进秩。
    if g.step(Stage::Args) != Err(DD_EBAL) || g.rank() != 1 || g.rejected() != 2 {
        return false;
    }
    // 跳档：open 之后直接到 read。
    if g.step(Stage::Open) != Ok(()) {
        return false;
    }
    if g.step(Stage::Read) != Err(DD_EBAL) || g.rank() != 2 {
        return false;
    }
    if g.step(Stage::Seek) != Ok(()) {
        return false;
    }
    // 倒退：事后倒推的申报。
    if g.step(Stage::Open) != Err(DD_ESTAGE) || g.rank() != 3 || g.rejected() != 4 {
        return false;
    }
    // 走完全程并封印。
    let ladder = [
        Stage::Read,
        Stage::Write,
        Stage::Records,
        Stage::Summary,
        Stage::Abort,
        Stage::Exit,
    ];
    let mut li = 0usize;
    while li < ladder.len() {
        let st = match ladder.get(li) {
            Some(v) => *v,
            None => return false,
        };
        if g.step(st) != Ok(()) {
            return false;
        }
        li += 1;
    }
    if g.rank() != STAGE_LAST_RANK {
        return false;
    }
    if g.close() != Ok(STAGE_LAST_RANK) || !g.is_closed() {
        return false;
    }
    if g.step(Stage::Exit) != Err(DD_ESTATE) {
        return false;
    }
    if g.close() != Err(DD_ESTATE) {
        return false;
    }
    if g.fingerprint() == FNV_OFF || g.rejected() != 6 {
        return false;
    }
    // 未走到最后一闸的行不许封印。
    let mut h = Gate::new();
    if h.step(Stage::Args) != Ok(()) {
        return false;
    }
    if h.close() != Err(DD_ESTAGE) || h.is_closed() {
        return false;
    }
    // 沿实际行的闸序走：绿行封得住，红行封不住（红行不得被当成走完纵线）。
    let clean_shape = match SHAPES.get(3) {
        Some(v) => *v,
        None => return false,
    };
    if gate_walk(&define(&clean_shape)) != Ok(STAGE_LAST_RANK) {
        return false;
    }
    let abort = match SHAPES.get(0) {
        Some(v) => *v,
        None => return false,
    };
    if gate_walk(&define(&abort)) != Err(DD_ESTAGE) {
        return false;
    }
    let silent = match SHAPES.get(6) {
        Some(v) => *v,
        None => return false,
    };
    if gate_walk(&define(&silent)) != Err(DD_ESTAGE) {
        return false;
    }
    // 封印后的闸门指纹与未走的闸门指纹必须不同（留痕可查）。
    let mut x = Gate::new();
    if x.step(Stage::Args) != Ok(()) {
        return false;
    }
    if x.fingerprint() == FNV_OFF {
        return false;
    }
    if x.fingerprint() == g.fingerprint() {
        return false;
    }
    true
}

/// 账本：14 格 8 成 6 败，逐格对账，冻结后拒登记，篡改留痕。
fn ledger_ok() -> bool {
    let mut l = build_ledger();
    if l.len() != SAMPLES || l.ok_count() != ACCEPT || l.err_count() != REJECT {
        return false;
    }
    if l.is_empty() || l.audit() != Ok(()) {
        return false;
    }
    if l.seal() == 0 || l.rejects() != 0 {
        return false;
    }
    // 成格的值逐格等于 VALS（含第 8 格 929），败格的码逐格等于 REJECT_CODES。
    let mut vi = 0usize;
    let mut ci = 0usize;
    let mut i = 0usize;
    while i < SAMPLES {
        let s = match l.sample(i) {
            Some(v) => v,
            None => return false,
        };
        if s.slot as usize != i {
            return false;
        }
        if s.ok {
            if let Some(w) = VALS.get(vi) {
                if s.value != *w || s.code != 0 {
                    return false;
                }
            } else {
                return false;
            }
            vi += 1;
        } else {
            if let Some(w) = REJECT_CODES.get(ci) {
                if s.code != *w || s.value != 0 {
                    return false;
                }
            } else {
                return false;
            }
            ci += 1;
        }
        i += 1;
    }
    if vi != ACCEPT || ci != REJECT {
        return false;
    }
    if l.sample(SAMPLES).is_some() {
        return false;
    }
    // 冻结：指纹非零、再登记被拒、再冻结被拒、重放同指纹。
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if root == 0 || !l.is_frozen() {
        return false;
    }
    if l.root() != root {
        return false;
    }
    if l.replay() != Ok(root) {
        return false;
    }
    if l.register(Sample::good(SAMPLES as u8, OP_REPLAY, 1)) != Err(DD_ESLOT) {
        return false;
    }
    if l.rejects() != 1 || l.len() != SAMPLES {
        return false;
    }
    if l.freeze() != Err(DD_EBAL) {
        return false;
    }
    if l.rejects() != 2 {
        return false;
    }
    // 拒绝计数也折进指纹：两次被拒之后账面数字没变，账本已经不是那本账，
    // 重放必须不平（这是 freeze/replay 组合的最后一道防线）。
    if l.replay() != Err(DD_ESEAL) {
        return false;
    }
    // 槽位纪律：跳号与越界都不入账，只加拒绝计数。
    let mut fresh = Ledger::new();
    if !fresh.is_empty() || fresh.len() != 0 {
        return false;
    }
    if fresh.register(Sample::good(1, OP_CORPUS, 14)) != Err(DD_ESLOT) {
        return false;
    }
    if fresh.rejects() != 1 || fresh.len() != 0 {
        return false;
    }
    if fresh.register(Sample::good(0, OP_CORPUS, 14)) != Ok(()) {
        return false;
    }
    if fresh.register(Sample { slot: SAMPLES as u8, op: OP_CORPUS, ok: true, code: 0, value: 14 })
        != Err(DD_ESLOT)
    {
        return false;
    }
    if fresh.len() != 1 || fresh.rejects() != 2 {
        return false;
    }
    if fresh.audit() == Ok(()) {
        return false;
    }
    if fresh.audit() != Err(DD_EBAL) {
        return false;
    }
    if fresh.sample(0).map(|x| x.value) != Some(14) {
        return false;
    }
    // 空白账本对账必须不平；Sample 的构造器与指纹自洽。
    let empty = Ledger::new();
    if empty.audit() != Err(DD_EBAL) || empty.replay() == Ok(0) {
        return false;
    }
    if Sample::blank().ok || Sample::good(0, 0, 1).code != 0 || Sample::bad(0, 0, 1).value != 0 {
        return false;
    }
    if Sample::good(1, 2, 3).seal() != Sample::good(1, 2, 3).seal() {
        return false;
    }
    if Sample::good(1, 2, 3).seal() == Sample::bad(1, 2, 3).seal() {
        return false;
    }
    // 值列篡改：audit 当场拦下（值在对账面上）。
    let tampered = tampered_ledger(7, RATE_FULL);
    if tampered.audit() != Err(DD_EVALUE) {
        return false;
    }
    if tampered.replay() != Err(DD_ESEAL) {
        return false;
    }
    // 操作码篡改：audit 放过、replay 拦下（审计的已知盲区由指纹兜底）。
    let blind = tampered_op_ledger(3, OP_ARGS);
    if blind.audit() != Ok(()) {
        return false;
    }
    if blind.replay() != Err(DD_ESEAL) {
        return false;
    }
    let clean_root = match build_ledger().freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    // `root()` 只记冻结那一刻的账：值改在冻结**之前**（root 跟着失真），
    // op 改在冻结**之后**（root 仍是干净账的指纹，但 seal/replay 当场翻脸）。
    if blind.root() != clean_root {
        return false;
    }
    if tampered.root() == clean_root {
        return false;
    }
    if blind.seal() == clean_root || tampered.seal() == clean_root {
        return false;
    }
    if !blind.is_frozen() || !tampered.is_frozen() {
        return false;
    }
    // 缺陷格必须与语料后 6 槽的实际缺陷同码（账本不是第二套判据）。
    let mut d = 0usize;
    while d < REJECT {
        let c = match case_at(ACCEPT + d) {
            Some(v) => v,
            None => return false,
        };
        if let Some(s) = build_ledger().sample(ACCEPT + d) {
            if s.ok || s.code != judge(&c).code() || s.op != OP_DEFECT {
                return false;
            }
        } else {
            return false;
        }
        d += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 检查器（恰好 13 项）
// ---------------------------------------------------------------------------

/// 本卡的 13 项判据检查。
pub fn run_unx11683_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11683_dd_enospc_records");

    s.add("DD01-名称与段基址", name_ok(), "static");

    s.add("DD02-枚举与九闸纵线", enums_ok(), "static");

    s.add("DD03-设备几何与余量", geom_ok(), "static");

    s.add("DD04-形状与合法域", shape_ok(), "static");

    s.add("DD05-九闸判定纵线", define_ok(), "static");

    s.add("DD06-块算术与半块账", records_ok(), "static");

    s.add("DD07-越界与邻居拒收", scope_ok(), "static");

    s.add("DD08-申报对判", judge_ok(), "static");

    s.add("DD09-矩阵分档与渲染", matrix_ok(), "static");

    s.add("DD10-语料三陈与全绿", corpus_ok() && slots_ok(), "static");

    s.add("DD11-文案家族逐字", message_ok() && reds_ok(), "static");

    s.add("DD12-释义闸门与账本", detail_ok() && gate_ok() && ledger_ok(), "static");

    s.add(
        "DD13-冻结重放四注入",
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

    // -----------------------------------------------------------------------
    // 名称、段基址与链上锚
    // -----------------------------------------------------------------------

    #[test]
    fn t_seg_base_and_chain_anchors() {
        assert_eq!(SEG_BASE, 0x6d150);
        assert_eq!(ANCHOR_F11681_SEG, 0x6d130);
        assert_eq!(ANCHOR_F11681_GAP, 0x20);
        assert_eq!(ANCHOR_F11682_SEG, 0x6d140);
        assert_eq!(ANCHOR_F11682_GAP, 0x10);
        assert_eq!(ANCHOR_F11684_SEG, 0x6d160);
        assert_eq!(SEG_BASE - ANCHOR_F11681_GAP, ANCHOR_F11681_SEG);
        assert_eq!(SEG_BASE - ANCHOR_F11682_GAP, ANCHOR_F11682_SEG);
        assert!(anchors_agree());
        assert!(name_ok());
        let text = anchor_text();
        assert!(text.contains("f11681=6d130"));
        assert!(text.contains("f11683=6d150"));
        assert!(text.contains("f11684=6d160"));
    }

    #[test]
    fn t_thirteen_codes_fill_the_segment() {
        assert_eq!(ALL_CODES.len(), 13);
        assert_eq!(DD_ESTAGE, 0x6d151);
        assert_eq!(DD_ECODE, 0x6d152);
        assert_eq!(DD_EVALUE, 0x6d153);
        assert_eq!(DD_ENOTE, 0x6d154);
        assert_eq!(DD_EDEFAULT, 0x6d155);
        assert_eq!(DD_EPARTIAL, 0x6d156);
        assert_eq!(DD_ESTATE, 0x6d157);
        assert_eq!(DD_ECOUNT, 0x6d158);
        assert_eq!(DD_ESLOT, 0x6d159);
        assert_eq!(DD_ESEAL, 0x6d15a);
        assert_eq!(DD_ENOBS, 0x6d15b);
        assert_eq!(DD_EMATRIX, 0x6d15c);
        assert_eq!(DD_EBAL, 0x6d15d);
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            let c = match ALL_CODES.get(i) {
                Some(v) => *v,
                None => panic!("code slot"),
            };
            assert!(c > SEG_BASE && c <= SEG_BASE + 0x0d);
            let mut j = i + 1;
            while j < ALL_CODES.len() {
                assert_ne!(c, ALL_CODES[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn t_corpus_sizes_and_ledger_dims() {
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(STAT_CELLS, 7);
        assert_eq!(RED_RATE, 929);
        assert_eq!(RATE_FULL, 1000);
        assert_eq!(VALS, [14, 8, 6, 3, 1, 3, 1, 929]);
        assert_eq!(cases().len(), CORPUS);
        assert_eq!(SHAPES.len(), CORPUS);
        assert_eq!(CLAIMS.len(), CORPUS);
        assert_eq!(WANT_TAGS.len(), CORPUS);
    }

    #[test]
    fn t_errno_truths_and_names() {
        assert_eq!(ERR_ENOSPC, 28);
        assert_eq!(ERR_EIO, 5);
        assert_eq!(ERR_EACCES, 13);
        assert_eq!(ERR_ENOENT, 2);
        assert_eq!(ERR_EDQUOT, 122);
        assert_eq!(ERR_NONE, 0);
        assert_eq!(ANCHOR_ENOSPC, 28);
        assert_eq!(ANCHOR_ERRNO_TEXT, "No space left on device");
        assert_eq!(ANCHOR_EXIT_FILE, 1);
        assert_eq!(strerror(ERR_ENOSPC), ANCHOR_ERRNO_TEXT);
        assert_eq!(strerror(ERR_EIO), "Input/output error");
        assert_eq!(strerror(ERR_EDQUOT), "Quota exceeded");
        assert_eq!(errno_name(ERR_ENOSPC), "ENOSPC");
        assert_eq!(errno_name(ERR_NONE), "0");
        assert_eq!(errno_name(4242), "?");
        assert_eq!(ERRNO_SET_SIZE, 6);
    }

    #[test]
    fn t_exit_codes_and_usage_sentinel() {
        assert_eq!(EXIT_OK, 0);
        assert_eq!(EXIT_FILE, 1);
        assert_eq!(EXIT_USAGE, 2);
        assert_eq!(EXIT_UNJUDGED, 99);
        assert_eq!(EXIT_RANKS, 4);
        assert_eq!(STAGE_COUNT, 9);
        assert_eq!(STAGE_LAST_RANK, 9);
    }

    #[test]
    fn t_conv_bit_partition() {
        assert_eq!(CONV_NOTRUNC, 0x01);
        assert_eq!(CONV_FSYNC, 0x02);
        assert_eq!(CONV_EXCL, 0x04);
        assert_eq!(CONV_NOERROR, 0x08);
        assert_eq!(CONV_SYNC, 0x10);
        assert_eq!(CONV_OUTSCOPE, CONV_NOERROR | CONV_SYNC);
        assert_eq!(CONV_BITS.len(), CONV_BIT_COUNT);
        let noerror = Shape { conv: CONV_NOERROR, ..BASE };
        assert_eq!(noerror.out_of_scope_bits(), CONV_NOERROR);
        assert_eq!(noerror.unknown_conv_bits(), 0);
        let swab = Shape { conv: 0x40, operand: "conv=swab", ..BASE };
        assert_eq!(swab.out_of_scope_bits(), 0);
        assert_ne!(swab.unknown_conv_bits(), 0);
        assert!(noerror.valid());
        assert_eq!(noerror.usage_bad(), false);
        assert_eq!(define(&noerror).note, Note::OutScope);
        let fsync = Shape { conv: CONV_FSYNC, ..BASE };
        assert_eq!(fsync.out_of_scope_bits(), 0);
        assert_eq!(fsync.conv_has(CONV_FSYNC), true);
        assert_eq!(fsync.conv_has(CONV_SYNC), false);
    }

    // -----------------------------------------------------------------------
    // 枚举：九闸、注解、家族、档位、缺陷
    // -----------------------------------------------------------------------

    #[test]
    fn t_stage_ranks_run_one_to_nine() {
        let all = Stage::all();
        assert_eq!(all.len(), 9);
        let names = [
            "args", "open", "seek", "read", "write", "records", "summary", "abort", "exit",
        ];
        let mut i = 0usize;
        while i < all.len() {
            let s = match all.get(i) {
                Some(v) => *v,
                None => panic!("stage"),
            };
            assert_eq!(s.rank(), (i as u32) + 1);
            assert_eq!(s.name(), names[i]);
            assert_eq!(Stage::from_rank(s.rank()), Some(s));
            i += 1;
        }
        assert_eq!(Stage::from_rank(0), None);
        assert_eq!(Stage::from_rank(10), None);
    }

    #[test]
    fn t_stage_class_partitions() {
        assert_eq!(Stage::Args.pre_loop(), true);
        assert_eq!(Stage::Open.pre_loop(), true);
        assert_eq!(Stage::Seek.pre_loop(), true);
        assert_eq!(Stage::Read.pre_loop(), false);
        assert_eq!(Stage::Exit.failure(), false);
        assert_eq!(Stage::Abort.failure(), true);
        assert_eq!(Stage::Args.has_ledger(), false);
        assert_eq!(Stage::Read.has_ledger(), true);
        assert_eq!(Stage::Write.has_ledger(), true);
        assert_eq!(Stage::Write.space_family(), true);
        assert_eq!(Stage::Records.space_family(), true);
        assert_eq!(Stage::Summary.space_family(), true);
        assert_eq!(Stage::Abort.space_family(), true);
        assert_eq!(Stage::Read.space_family(), false);
        assert_eq!(Stage::Exit.space_family(), false);
        assert_eq!(Stage::Abort.rank() < Stage::Exit.rank(), true);
    }

    #[test]
    fn t_note_families_are_disjoint() {
        let ns = Note::all();
        assert_eq!(ns.len(), 15);
        let mut abort_n = 0usize;
        let mut foreign_n = 0usize;
        let mut success_n = 0usize;
        let mut gnu_text_n = 0usize;
        let mut marker_n = 0usize;
        let mut i = 0usize;
        while i < ns.len() {
            let n = match ns.get(i) {
                Some(v) => *v,
                None => panic!("note"),
            };
            if n.abort_family() {
                abort_n += 1;
            }
            if n.foreign() {
                foreign_n += 1;
            }
            if n.success() {
                success_n += 1;
            }
            if n.gnu_text() {
                gnu_text_n += 1;
            }
            if n.wants_partial_marker() {
                marker_n += 1;
            }
            assert_eq!(n.abort_family() && n.foreign(), false);
            assert_eq!(n.success() && n.abort_family(), false);
            let mut j = i + 1;
            while j < ns.len() {
                assert_ne!(n.name(), ns[j].name());
                j += 1;
            }
            i += 1;
        }
        assert_eq!(abort_n, 4);
        assert_eq!(foreign_n, 4);
        assert_eq!(success_n, 4);
        assert_eq!(gnu_text_n, 3);
        assert_eq!(marker_n, 3);
        assert_eq!(Note::Silent.name(), "summary-none");
        assert_eq!(Note::AbortBb.name(), "abort-busybox");
        assert_eq!(Note::OutScope.name(), "out-of-scope");
        assert_eq!(Note::OutScope.out_of_scope(), true);
        assert_eq!(Note::RecordsLie.lie(), true);
        assert_eq!(Note::Usage.usage(), true);
        assert_eq!(Note::FsyncGnu.abort_family(), true);
        assert_eq!(Note::FsyncGnu.wants_partial_marker(), false);
        assert_eq!(Note::PairBroken.foreign(), true);
    }

    #[test]
    fn t_flavor_family_rules() {
        assert_eq!(Flavor::ALL.len(), 3);
        assert_eq!(Flavor::Gnu8.name(), "gnu8");
        assert_eq!(Flavor::Gnu9.name(), "gnu9");
        assert_eq!(Flavor::Busybox.name(), "busybox");
        assert_eq!(Flavor::Gnu8.gnu(), true);
        assert_eq!(Flavor::Gnu9.gnu(), true);
        assert_eq!(Flavor::Busybox.gnu(), false);
        assert_eq!(Flavor::Gnu9.modern(), true);
        assert_eq!(Flavor::Gnu8.modern(), false);
        assert_eq!(Flavor::Busybox.prints_byte_line(), false);
        assert_eq!(Flavor::Gnu8.prints_byte_line(), true);
        assert_eq!(Flavor::Busybox.prefix(), "dd: ");
        assert_eq!(Flavor::Gnu8.index(), 0);
        assert_eq!(Flavor::Busybox.index(), 2);
        assert_eq!(
            Flavor::Gnu8.write_error_text("disk.img", ERR_ENOSPC).as_str(),
            "dd: writing 'disk.img': No space left on device"
        );
        assert_eq!(
            Flavor::Gnu9.write_error_text("disk.img", ERR_ENOSPC).as_str(),
            "dd: error writing 'disk.img': No space left on device"
        );
        assert_eq!(
            Flavor::Busybox.write_error_text("disk.img", ERR_ENOSPC).as_str(),
            "dd: write error to 'disk.img': No space left on device"
        );
        assert_eq!(family_note(Flavor::Gnu8), Note::AbortGnu8);
        assert_eq!(family_note(Flavor::Busybox), Note::AbortBb);
    }

    #[test]
    fn t_tier_projection_uses_the_anchor_bit_only() {
        let green = Outcome::make(Stage::Exit, EXIT_OK as i32, 1024, Note::Done, false);
        assert_eq!(Tier::of(&green), Tier::Complete);
        let silent = Outcome::make(Stage::Summary, EXIT_OK as i32, 2048, Note::Silent, false);
        assert_eq!(Tier::of(&silent), Tier::Silent);
        let abort = Outcome::make(Stage::Abort, EXIT_FILE as i32, 6144, Note::AbortGnu9, true);
        assert_eq!(Tier::of(&abort), Tier::Aborted);
        assert_eq!(abort.anchored(), true);
        assert_eq!(abort.refused(), false);
        let red_no_anchor = Outcome::make(Stage::Abort, EXIT_FILE as i32, 6144, Note::PairBroken, false);
        assert_eq!(Tier::of(&red_no_anchor), Tier::Refused);
        assert_eq!(red_no_anchor.anchored(), false);
        assert_eq!(red_no_anchor.refused(), true);
        let blank = Outcome::blank();
        assert_eq!(Tier::of(&blank), Tier::Refused);
        assert_eq!(blank.code, EXIT_UNJUDGED as i32);
        assert_eq!(Tier::Complete.success(), true);
        assert_eq!(Tier::Silent.success(), true);
        assert_eq!(Tier::Aborted.success(), false);
        assert_eq!(Tier::Aborted.anchored(), true);
        assert_eq!(Tier::Complete.needs_summary(), true);
        assert_eq!(Tier::Silent.needs_summary(), false);
        assert_eq!(Tier::from_index(4), None);
        assert_eq!(Tier::Refused.index(), 3);
        assert_eq!(Tier::of(&abort).name(), "aborted");
    }

    #[test]
    fn t_fault_codes_and_judged_dimensions() {
        assert_eq!(Fault::all().len(), 14);
        assert_eq!(Fault::Ok.code(), 0);
        assert_eq!(Fault::Estate.code(), DD_ESTATE);
        assert_eq!(Fault::Stage.code(), DD_ESTAGE);
        assert_eq!(Fault::Code.code(), DD_ECODE);
        assert_eq!(Fault::Value.code(), DD_EVALUE);
        assert_eq!(Fault::Note.code(), DD_ENOTE);
        assert_eq!(Fault::Default.code(), DD_EDEFAULT);
        assert_eq!(Fault::Partial.code(), DD_EPARTIAL);
        assert_eq!(Fault::Count.code(), DD_ECOUNT);
        assert_eq!(Fault::Slot.code(), DD_ESLOT);
        assert_eq!(Fault::Seal.code(), DD_ESEAL);
        assert_eq!(Fault::Nobs.code(), DD_ENOBS);
        assert_eq!(Fault::Matrix.code(), DD_EMATRIX);
        assert_eq!(Fault::Bal.code(), DD_EBAL);
        assert_eq!(Fault::Ok.alive(), false);
        assert_eq!(Fault::Estate.alive(), true);
        assert_eq!(Fault::Partial.judged(), false);
        assert_eq!(Fault::Seal.judged(), false);
        let judged = [
            Fault::Estate,
            Fault::Stage,
            Fault::Code,
            Fault::Value,
            Fault::Note,
            Fault::Default,
        ];
        let mut i = 0usize;
        let mut n = 0usize;
        while i < Fault::all().len() {
            if let Some(f) = Fault::all().get(i) {
                if f.judged() {
                    n += 1;
                    assert!(judged.contains(f));
                }
            }
            i += 1;
        }
        assert_eq!(n, REJECT);
        assert_eq!(JUDGE_ORDER, REJECT_FAULTS);
        assert_eq!(fault_short(Fault::Estate), "estate");
        assert_eq!(fault_short(Fault::Default), "default");
        assert_eq!(fault_short(Fault::Partial), "partial");
    }

    // -----------------------------------------------------------------------
    // 设备几何与形状合法域
    // -----------------------------------------------------------------------

    #[test]
    fn t_device_geoms_and_room() {
        let gs = geoms();
        assert_eq!(gs.len(), 3);
        assert_eq!(geom_room(0), Some(0));
        assert_eq!(geom_room(1), Some(6144));
        assert_eq!(geom_room(2), Some(0));
        assert_eq!(geom_room(3), None);
        assert_eq!(geom_room(99), None);
        assert_eq!(gs[0].mount, "/mnt/full");
        assert_eq!(gs[0].is_full(), true);
        assert_eq!(gs[1].bsize, 512);
        assert_eq!(gs[1].blocks_free, 12);
        assert_eq!(gs[1].delalloc, false);
        assert_eq!(gs[2].delalloc, true);
        assert_eq!(gs[2].dst_existing, 2048);
        assert_eq!(gs[2].is_full(), true);
        let mut i = 0usize;
        while i < gs.len() {
            assert!(gs[i].well_formed());
            assert_eq!(gs[i].room_after_truncate(), gs[i].bytes_free());
            i += 1;
        }
        let weird =
            DevGeom { mount: "/mnt/weird", bsize: 3000, blocks_free: 1, delalloc: false, dst_existing: 0 };
        assert_eq!(weird.well_formed(), false);
        let anon = DevGeom::blank();
        assert_eq!(anon.well_formed(), false);
        assert_eq!(anon.bytes_free(), Some(0));
        assert_ne!(gs[0].seal(), gs[1].seal());
        assert!(geom_ok());
    }

    #[test]
    fn t_shape_valid_domain_and_family_rules() {
        assert!(BASE.valid());
        let mut i = 0usize;
        while i < CORPUS {
            if let Some(s) = SHAPES.get(i) {
                assert!(s.valid());
                assert!(!s.tag.is_empty());
            }
            i += 1;
        }
        let bb_lie = Shape {
            flavor: Flavor::Busybox,
            logged_bytes_known: true,
            logged_bytes: 6144,
            logged_in_full: 2,
            ..BASE
        };
        assert_eq!(bb_lie.valid(), false);
        assert_eq!(bb_lie.logged_legible(), false);
        let gnu_missing = Shape { logged_bytes_known: false, logged_in_full: 2, ..BASE };
        assert_eq!(gnu_missing.valid(), false);
        let silent = Shape { status_none: true, logged_bytes_known: false, ..BASE };
        assert!(silent.valid());
        let zero_bs = Shape { bs: 0, ibs: 0, obs: 0, ..BASE };
        assert_eq!(zero_bs.valid(), false);
        let huge = Shape { bs: BLOCK_MAX + 1, ..BASE };
        assert_eq!(huge.valid(), false);
        let over = Shape { free_bytes: BYTES_MAX + 1, ..BASE };
        assert_eq!(over.valid(), false);
        let bad_errno = Shape { errno_seen: 4242, ..BASE };
        assert_eq!(bad_errno.errno_legal(), false);
        assert_eq!(bad_errno.valid(), false);
        let two_parts = Shape { logged_out_part: 2, ..BASE };
        assert_eq!(two_parts.logged_legible(), false);
        let blank = Shape::blank();
        assert_eq!(blank.bs, 512);
        assert_eq!(blank.eff_ibs(), 512);
        assert_eq!(blank.eff_obs(), 512);
        assert_eq!(blank.flavor, Flavor::Gnu9);
        assert_eq!(blank.path, "disk.img");
        assert!(shape_ok());
    }

    #[test]
    fn t_bs_suppresses_ibs_and_obs() {
        let split = Shape { bs: 0, ibs: 512, obs: 4096, ..BASE };
        assert_eq!(split.eff_ibs(), 512);
        assert_eq!(split.eff_obs(), 4096);
        let both = Shape { bs: 1024, ibs: 512, obs: 4096, ..BASE };
        assert_eq!(both.eff_ibs(), 1024);
        assert_eq!(both.eff_obs(), 1024);
        assert_eq!(BASE.eff_ibs(), 4096);
        assert_eq!(BASE.eff_obs(), 4096);
        let via_bs = match case_at(0) {
            Some(v) => v.shape,
            None => panic!("slot 0"),
        };
        let via_pair = Shape { bs: 0, ibs: 4096, obs: 4096, ..via_bs };
        assert_eq!(calc(&via_bs).seal(), calc(&via_pair).seal());
    }

    #[test]
    fn t_clone_helpers_touch_one_dimension() {
        let base = match SHAPES.get(0) {
            Some(v) => *v,
            None => panic!("slot 0"),
        };
        let freed = base.clone_free(0);
        assert_eq!(freed.free_bytes, 0);
        assert_eq!(freed.tag, base.tag);
        assert_eq!(freed.logged_out_part, base.logged_out_part);
        let src = base.clone_src(1024);
        assert_eq!(src.src_bytes, 1024);
        assert_eq!(src.free_bytes, base.free_bytes);
        let cnt = base.clone_count(3);
        assert_eq!(cnt.count, 3);
        assert_eq!(cnt.src_bytes, base.src_bytes);
        let noerror = Shape { conv: CONV_NOERROR, ..BASE };
        let stacked = noerror.with_conv(CONV_FSYNC);
        assert_eq!(stacked.conv_has(CONV_FSYNC), true);
        assert_eq!(stacked.conv_has(CONV_NOERROR), true);
        assert_eq!(base.with_conv(CONV_FSYNC).conv_has(CONV_NOERROR), false);
        let to_bb = base.with_flavor(Flavor::Busybox);
        assert_eq!(to_bb.flavor, Flavor::Busybox);
        assert_eq!(to_bb.logged_bytes_known, false);
        let to_gnu = base.with_flavor(Flavor::Gnu8);
        assert_eq!(to_gnu.logged_bytes_known, true);
        assert_ne!(Shape { tag: "another", ..base }.seal(), base.seal());
    }

    // -----------------------------------------------------------------------
    // 语料三陈：实际 × 块账 × 申报
    // -----------------------------------------------------------------------

    #[test]
    fn t_corpus_actual_matches_want_tables() {
        let cs = cases();
        let mut i = 0usize;
        while i < CORPUS {
            let c = match cs.get(i) {
                Some(v) => v,
                None => panic!("case"),
            };
            let o = c.actual();
            assert_eq!(o.stage, WANT_STAGE[i]);
            assert_eq!(o.code, WANT_CODE[i]);
            assert_eq!(o.value, WANT_VALUE[i]);
            assert_eq!(o.note, WANT_NOTE[i]);
            assert_eq!(o.default, WANT_DEFAULT[i]);
            assert_eq!(c.tag(), WANT_TAGS[i]);
            assert_eq!(o.ok(), o.note.success() && o.code == EXIT_OK as i32);
            assert_eq!(o.failed(), !o.ok());
            i += 1;
        }
        assert_eq!(WANT_TAGS[0], "gnu9-abort-half");
        assert_eq!(WANT_TAGS[7], "usage-bad-option");
        assert_eq!(WANT_TAGS[8], "estate-swap");
        assert_eq!(WANT_STAGE[6], Stage::Summary);
        assert_eq!(WANT_CODE[8], EXIT_UNJUDGED as i32);
        assert_eq!(WANT_DEFAULT[13], false);
    }

    #[test]
    fn t_corpus_blocks_match_act_tables() {
        let cs = cases();
        let mut i = 0usize;
        while i < CORPUS {
            let c = match cs.get(i) {
                Some(v) => v,
                None => panic!("case"),
            };
            let b = c.blocks();
            assert_eq!(b.in_full, ACT_IN_FULL[i]);
            assert_eq!(b.in_part, ACT_IN_PART[i]);
            assert_eq!(b.out_full, ACT_OUT_FULL[i]);
            assert_eq!(b.out_part, ACT_OUT_PART[i]);
            assert_eq!(b.landed, ACT_LANDED[i]);
            assert_eq!(b.lost, ACT_LOST[i]);
            assert_eq!(b.cut_bytes, ACT_CUT[i]);
            assert!(b.in_part <= 1 && b.out_part <= 1);
            assert_eq!(b.in_bytes, b.landed + b.lost);
            assert_eq!(b.landed, b.out_full * b.obs + b.tail);
            assert_eq!(b.write_failed, b.lost > 0);
            assert_eq!(b.overflow, false);
            i += 1;
        }
        assert_eq!(ACT_LANDED[8], 6144);
        assert_eq!(ACT_LOST[8], 2048);
        assert_eq!(ACT_CUT[5], 2048);
    }

    #[test]
    fn t_corpus_judge_matches_expected_table() {
        let cs = cases();
        let mut goods = 0usize;
        let mut bads = 0usize;
        let mut i = 0usize;
        while i < CORPUS {
            let c = match cs.get(i) {
                Some(v) => v,
                None => panic!("case"),
            };
            let want = match expected(i) {
                Some(v) => v,
                None => panic!("expected"),
            };
            let f = judge(&c);
            assert_eq!(f, want);
            assert_eq!(c.ok(), f == Fault::Ok);
            if f == Fault::Ok {
                goods += 1;
            } else {
                bads += 1;
                assert_eq!(f.code(), REJECT_CODES[i - ACCEPT]);
                assert_eq!(f, JUDGE_ORDER[i - ACCEPT]);
            }
            i += 1;
        }
        assert_eq!(goods, ACCEPT);
        assert_eq!(bads, REJECT);
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(CORPUS - 1), Err(DD_ENOBS));
        assert!(case_at(CORPUS).is_none());
        assert!(expected(CORPUS).is_none());
    }

    #[test]
    fn t_accept_reject_slots_partition() {
        assert_eq!(ACCEPT_SLOTS, [0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(REJECT_SLOTS, [8, 9, 10, 11, 12, 13]);
        assert_eq!(WANT_NDX, [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6]);
        assert_eq!(REJECT_FAULTS, [Fault::Estate, Fault::Stage, Fault::Code, Fault::Value, Fault::Note, Fault::Default]);
        assert_eq!(REJECT_CODES, [DD_ESTATE, DD_ESTAGE, DD_ECODE, DD_EVALUE, DD_ENOTE, DD_EDEFAULT]);
    }

    #[test]
    fn t_five_replica_slots_share_the_mother_scene() {
        let first = match case_at(0) {
            Some(v) => v,
            None => panic!("slot 0"),
        };
        let mut rep = ACCEPT + 1;
        while rep < CORPUS {
            let c = match case_at(rep) {
                Some(v) => v,
                None => panic!("replica"),
            };
            assert_eq!(c.actual(), first.actual());
            assert_eq!(c.blocks(), first.blocks());
            assert_eq!(c.shape.tag, WANT_TAGS[rep]);
            assert_ne!(c.claim, first.claim);
            assert_eq!(detail(&c), detail(&first));
            assert_eq!(message(&c.shape, &c.actual()), message(&first.shape, &first.actual()));
            assert_ne!(c.seal(), first.seal());
            rep += 1;
        }
        let estate = match case_at(ACCEPT) {
            Some(v) => v,
            None => panic!("estate"),
        };
        assert_eq!(estate.shape.out_of_scope_bits(), CONV_NOERROR);
        assert_ne!(estate.actual(), first.actual());
        assert_eq!(estate.actual().stage, Stage::Args);
        assert_eq!(estate.actual().code, EXIT_UNJUDGED as i32);
        assert_eq!(estate.claim.success(), true);
        assert_eq!(judge(&estate), Fault::Estate);
        assert_eq!(Tier::of(&estate.actual()), Tier::Refused);
    }

    // -----------------------------------------------------------------------
    // 块算术：半块、整块、bs/ibs/obs、skip/seek/count
    // -----------------------------------------------------------------------

    #[test]
    fn t_half_block_marker_on_partial_abort() {
        let c = match case_at(0) {
            Some(v) => v,
            None => panic!("slot 0"),
        };
        let b = c.blocks();
        assert_eq!(b.in_full, 2);
        assert_eq!(b.out_full, 1);
        assert_eq!(b.out_part, 1);
        assert_eq!(b.tail, 2048);
        assert_eq!(b.landed, 6144);
        assert_eq!(b.lost, 2048);
        assert_eq!(b.partial(), true);
        assert_eq!(b.all_landed(), false);
        assert_eq!(b.obs, 4096);
        assert_eq!(b.in_text().as_str(), "2+0 records in");
        assert_eq!(b.out_text().as_str(), "1+1 records out");
        assert_eq!(msg_partial_marker(b.out_full, b.out_part, b.tail).as_str(),
            "partial block: records out 1+1, tail bytes 2048");
        let m = c.actual();
        assert_eq!(m.marker_consistent(&b), true);
        assert_eq!(b.landed, b.out_full * b.obs + b.tail);
        assert_eq!(b.lost, b.in_bytes - b.landed);
    }

    #[test]
    fn t_whole_block_failure_has_no_partial_marker() {
        let c = match case_at(1) {
            Some(v) => v,
            None => panic!("slot 1"),
        };
        let b = c.blocks();
        assert_eq!(b.in_full, 8);
        assert_eq!(b.out_full, 4);
        assert_eq!(b.out_part, 0);
        assert_eq!(b.tail, 0);
        assert_eq!(b.landed, 2048);
        assert_eq!(b.lost, 2048);
        assert_eq!(b.partial(), false);
        assert_eq!(b.obs, 512);
        assert_eq!(c.actual().note, Note::AbortGnu8);
        let msg = message(&c.shape, &c.actual());
        assert!(!msg.contains("partial block:"));
        assert_eq!(msg.as_str(),
            "dd: writing 'disk.img': No space left on device | 8+0 records in | 4+0 records out | 2048 bytes (2.0 kB, 2.0 KiB) copied");
    }

    #[test]
    fn t_ibs_ne_obs_keeps_two_ledgers_apart() {
        let c = match case_at(4) {
            Some(v) => v,
            None => panic!("slot 4"),
        };
        let b = c.blocks();
        assert_eq!(b.ibs, 1024);
        assert_eq!(b.obs, 2048);
        assert_eq!(b.in_full, 3);
        assert_eq!(b.out_full, 1);
        assert_eq!(b.out_part, 1);
        assert_eq!(b.tail, 1024);
        assert_eq!(b.landed, 3072);
        assert_eq!(b.lost, 0);
        assert_eq!(b.in_bytes, 3072);
        assert_eq!(c.actual().note, Note::DonePartial);
        assert_eq!(c.actual().stage, Stage::Exit);
        assert_eq!(Tier::of(&c.actual()), Tier::Complete);
        assert_eq!(b.in_full * b.ibs, b.landed);
        assert_eq!(message(&c.shape, &c.actual()).as_str(),
            "3+0 records in | 1+1 records out | 3072 bytes (3.1 kB, 3.0 KiB) copied");
    }

    #[test]
    fn t_skip_bytes_never_enter_the_read_ledger() {
        let base = match SHAPES.get(0) {
            Some(v) => *v,
            None => panic!("slot 0"),
        };
        let skipped = Shape {
            skip: 1,
            logged_in_full: 1,
            logged_out_full: 1,
            logged_out_part: 0,
            logged_bytes: 4096,
            ..base
        };
        let b = calc(&skipped);
        assert_eq!(b.skipped_bytes, 4096);
        assert_eq!(b.in_full, 1);
        assert_eq!(b.in_bytes, 4096);
        assert_eq!(b.landed, 4096);
        assert_eq!(b.lost, 0);
        assert_eq!(records_agree(&skipped, &b), true);
        assert_eq!(define(&skipped).stage, Stage::Exit);
        assert_eq!(define(&skipped).note, Note::Done);
        let over = Shape {
            skip: 8,
            logged_in_full: 0,
            logged_out_full: 0,
            logged_out_part: 0,
            logged_bytes: 0,
            ..base
        };
        let bo = calc(&over);
        assert_eq!(bo.skipped_bytes, 8192);
        assert_eq!(bo.in_full, 0);
        assert_eq!(bo.in_bytes, 0);
        assert_eq!(bo.landed, 0);
        assert_eq!(bo.lost, 0);
        assert_eq!(define(&over).code, EXIT_OK as i32);
        assert_eq!(define(&over).value, 0);
        let overflowing = Shape { skip: 0x8000_0000, ..BASE };
        assert_eq!(calc(&overflowing).overflow, true);
        assert_eq!(define(&overflowing).stage, Stage::Seek);
        assert_eq!(define(&overflowing).note, Note::SeekErr);
    }

    #[test]
    fn t_seek_hole_does_not_consume_space() {
        let hole = match scene_at(1) {
            Some(v) => v,
            None => panic!("scene 1"),
        };
        let b = calc(&hole);
        assert_eq!(b.hole_bytes, 4096);
        assert_eq!(b.landed, 0);
        assert_eq!(b.out_full, 0);
        assert_eq!(b.out_part, 0);
        assert_eq!(b.lost, 8192);
        assert_eq!(b.partial(), false);
        let o = define(&hole);
        assert_eq!(o.stage, Stage::Abort);
        assert_eq!(o.note, Note::AbortGnu9);
        assert_eq!(o.value, 0);
        assert_eq!(o.default, true);
        assert_eq!(gate_stop_rank(&o), Stage::Abort.rank());
        assert_eq!(message(&hole, &o).as_str(),
            "dd: error writing 'disk.img': No space left on device | 2+0 records in | 0+0 records out | 0 bytes (0 B) copied");
        assert_eq!(msg_bytes_copied(0).as_str(), "0 bytes (0 B) copied");
    }

    #[test]
    fn t_count_cut_is_not_lost_bytes() {
        let c = match case_at(5) {
            Some(v) => v,
            None => panic!("slot 5"),
        };
        let b = c.blocks();
        assert_eq!(c.shape.count, 4);
        assert_eq!(b.in_full, 4);
        assert_eq!(b.out_full, 4);
        assert_eq!(b.landed, 2048);
        assert_eq!(b.lost, 0);
        assert_eq!(b.cut_bytes, 2048);
        assert_eq!(b.all_landed(), true);
        assert_eq!(c.actual().note, Note::DoneTrunc);
        assert_eq!(c.actual().stage, Stage::Exit);
        assert_eq!(message(&c.shape, &c.actual()).as_str(),
            "4+0 records in | 4+0 records out | 2048 bytes (2.0 kB, 2.0 KiB) copied");
        let base = match SHAPES.get(0) {
            Some(v) => *v,
            None => panic!("slot 0"),
        };
        let cut_only = base.clone_count(1);
        let bc = calc(&cut_only);
        assert_eq!(bc.in_full, 1);
        assert_eq!(bc.cut_bytes, 4096);
        assert_eq!(bc.lost, 0);
    }

    // -----------------------------------------------------------------------
    // status=none、越界、对账谎报、errno 串档、fsync
    // -----------------------------------------------------------------------

    #[test]
    fn t_status_none_refuses_fake_green() {
        let clean = match case_at(6) {
            Some(v) => v,
            None => panic!("slot 6"),
        };
        assert_eq!(clean.actual().stage, Stage::Summary);
        assert_eq!(clean.actual().note, Note::Silent);
        assert_eq!(clean.actual().default, false);
        assert_eq!(clean.actual().anchored(), false);
        assert_eq!(Tier::of(&clean.actual()), Tier::Silent);
        assert_eq!(Tier::of(&clean.actual()).needs_summary(), false);
        assert_eq!(message(&clean.shape, &clean.actual()).as_str(),
            "dd status=none: records summary suppressed, anchor withheld");
        let abort = match scene_at(6) {
            Some(v) => v,
            None => panic!("scene 6"),
        };
        let oa = define(&abort);
        assert_eq!(oa.stage, Stage::Summary);
        assert_eq!(oa.code, EXIT_FILE as i32);
        assert_eq!(oa.value, 6144);
        assert_eq!(oa.default, false);
        assert_eq!(Tier::of(&oa), Tier::Refused);
        assert_eq!(oa.ok(), false);
        assert_eq!(oa.anchored(), false);
        assert_eq!(SCENE_WANT_NOTE[6], Note::Silent);
        assert_eq!(msg_summary_suppressed().as_str(),
            "dd status=none: records summary suppressed, anchor withheld");
    }

    #[test]
    fn t_conv_noerror_is_refused_and_named() {
        let c = match case_at(8) {
            Some(v) => v,
            None => panic!("slot 8"),
        };
        assert_eq!(c.shape.conv_has(CONV_NOERROR), true);
        assert_eq!(c.shape.out_of_scope_bits(), CONV_NOERROR);
        assert_eq!(c.actual().stage, Stage::Args);
        assert_eq!(c.actual().code, EXIT_UNJUDGED as i32);
        assert_eq!(c.actual().note, Note::OutScope);
        assert_eq!(c.actual().default, false);
        assert_eq!(c.actual().anchored(), false);
        assert_eq!(message(&c.shape, &c.actual()).as_str(),
            "dd conv out of scope: conv=noerror belongs to UNX-F11697");
        assert_eq!(msg_out_of_scope("conv=sync").as_str(),
            "dd conv out of scope: conv=sync belongs to UNX-F11697");
        let sync = Shape { conv: CONV_SYNC, operand: "conv=sync", ..BASE };
        assert_eq!(define(&sync).note, Note::OutScope);
        assert_eq!(scope_ok(), true);
    }

    #[test]
    fn t_records_lie_stops_at_the_records_gate() {
        let lie = match scene_at(4) {
            Some(v) => v,
            None => panic!("scene 4"),
        };
        let o = define(&lie);
        let b = calc(&lie);
        assert_eq!(o.stage, Stage::Records);
        assert_eq!(o.note, Note::RecordsLie);
        assert_eq!(o.default, false);
        assert_eq!(o.value, 6144);
        assert_eq!(Tier::of(&o), Tier::Refused);
        assert_eq!(records_agree(&lie, &b), false);
        assert_eq!(o.stage.has_ledger(), true);
        assert_eq!(o.stage.space_family(), true);
        assert_eq!(message(&lie, &o).as_str(),
            "dd records lie: log says 2+0 records out but geometry gives 1+1 records out");
        assert_eq!(msg_records_lie("1+1 records out", "2+0 records out").as_str(),
            "dd records lie: log says 2+0 records out but geometry gives 1+1 records out");
        assert_eq!(gate_stop_rank(&o), Stage::Records.rank());
    }

    #[test]
    fn t_errno_off_family_never_anchors() {
        let broken = match scene_at(5) {
            Some(v) => v,
            None => panic!("scene 5"),
        };
        let o = define(&broken);
        assert_eq!(o.stage, Stage::Abort);
        assert_eq!(o.note, Note::PairBroken);
        assert_eq!(o.default, false);
        assert_eq!(o.anchored(), false);
        assert_eq!(Tier::of(&o), Tier::Refused);
        assert_eq!(message(&broken, &o).as_str(), "dd write/errno pair broken (ret=-1, errno=5)");
        assert_eq!(broken.errno_seen, ERR_EIO);
        assert_eq!(ANCHOR_ENOSPC, ERR_ENOSPC);
        let acc = Shape { free_bytes: 0, errno_seen: ERR_EACCES, ..BASE };
        assert_eq!(define(&acc).note, Note::PairBroken);
        let quot = Shape { free_bytes: 0, errno_seen: ERR_EDQUOT, ..BASE };
        assert_eq!(define(&quot).stage, Stage::Abort);
        assert_eq!(msg_pair_broken(-1, ERR_ENOSPC).as_str(),
            "dd write/errno pair broken (ret=-1, errno=28)");
    }

    #[test]
    fn t_fsync_late_abort_still_anchors() {
        let fs = match scene_at(3) {
            Some(v) => v,
            None => panic!("scene 3"),
        };
        let o = define(&fs);
        let b = calc(&fs);
        assert_eq!(o.stage, Stage::Abort);
        assert_eq!(o.note, Note::FsyncGnu);
        assert_eq!(o.code, EXIT_FILE as i32);
        assert_eq!(o.value, 2048);
        assert_eq!(o.default, true);
        assert_eq!(o.anchored(), true);
        assert_eq!(b.lost, 0);
        assert_eq!(b.out_part, 0);
        assert_eq!(fs.commit_fail, true);
        assert_eq!(fs.conv_has(CONV_FSYNC), true);
        assert_eq!(Tier::of(&o), Tier::Aborted);
        assert_eq!(message(&fs, &o).as_str(),
            "dd: fsync failed for 'disk.img': No space left on device | 2+0 records in | 2+0 records out | 2048 bytes (2.0 kB, 2.0 KiB) copied");
        assert_eq!(msg_fsync_failed("disk.img", ERR_ENOSPC).as_str(),
            "dd: fsync failed for 'disk.img': No space left on device");
    }

    #[test]
    fn t_open_and_read_failures_are_neighbours() {
        let open = match scene_at(0) {
            Some(v) => v,
            None => panic!("scene 0"),
        };
        let oo = define(&open);
        assert_eq!(oo.stage, Stage::Open);
        assert_eq!(oo.note, Note::OpenErr);
        assert_eq!(oo.value, 0);
        assert_eq!(oo.default, false);
        assert_eq!(oo.stage.pre_loop(), true);
        assert_eq!(message(&open, &oo).as_str(),
            "dd: failed to open 'disk.img': No such file or directory");
        assert_eq!(msg_failed_to_open("disk.img", ERR_ENOENT).as_str(),
            "dd: failed to open 'disk.img': No such file or directory");
        let read = match scene_at(2) {
            Some(v) => v,
            None => panic!("scene 2"),
        };
        let ro = define(&read);
        assert_eq!(ro.stage, Stage::Read);
        assert_eq!(ro.note, Note::ReadErr);
        assert_eq!(ro.default, false);
        assert_eq!(ro.stage.has_ledger(), true);
        assert_eq!(message(&read, &ro).as_str(), "dd: error reading 'disk.img': Input/output error");
        assert_eq!(msg_gnu_reading("in.img", ERR_EIO, true).as_str(),
            "dd: error reading 'in.img': Input/output error");
        assert_eq!(msg_gnu_reading("in.img", ERR_EIO, false).as_str(),
            "dd: reading 'in.img': Input/output error");
        assert_eq!(msg_cannot_seek("disk.img").as_str(), "dd: 'disk.img': cannot seek: Invalid argument");
        assert_eq!(scene_at(1).unwrap_or(BASE).seek, 1);
    }

    #[test]
    fn t_usage_error_never_carries_a_ledger() {
        let c = match case_at(7) {
            Some(v) => v,
            None => panic!("slot 7"),
        };
        assert_eq!(c.actual().stage, Stage::Args);
        assert_eq!(c.actual().code, EXIT_USAGE as i32);
        assert_eq!(c.actual().value, 0);
        assert_eq!(c.actual().note, Note::Usage);
        assert_eq!(Tier::of(&c.actual()), Tier::Refused);
        assert_eq!(c.blocks().in_full, 0);
        assert_eq!(c.blocks().in_bytes, 0);
        assert_eq!(c.blocks().out_full, 0);
        assert_eq!(c.blocks().landed, 0);
        assert_eq!(c.blocks().write_failed, false);
        assert_eq!(c.blocks().overflow, false);
        assert_eq!(message(&c.shape, &c.actual()).as_str(), "dd: unrecognized operand 'ibs=abc'");
        assert_eq!(msg_unrecognized_operand("ibs=abc").as_str(), "dd: unrecognized operand 'ibs=abc'");
        assert_eq!(msg_invalid_option('z').as_str(), "dd: invalid option -- 'z'");
        let bad_conv = Shape { conv: 0x40, operand: "conv=swab", ..BASE };
        assert_eq!(define(&bad_conv).stage, Stage::Args);
        assert_eq!(define(&bad_conv).code, EXIT_USAGE as i32);
        assert_eq!(pair_text(2, 0).as_str(), "2+0");
        assert_eq!(pair_text(0, 1).as_str(), "0+1");
    }

    // -----------------------------------------------------------------------
    // 三式文案与释义行
    // -----------------------------------------------------------------------

    #[test]
    fn t_three_flavour_texts_are_byte_distinct() {
        let gnu8 = msg_gnu8_writing("disk.img", ERR_ENOSPC);
        let gnu9 = msg_gnu9_error_writing("disk.img", ERR_ENOSPC);
        let bb = msg_bb_write_error("disk.img", ERR_ENOSPC);
        assert_eq!(gnu8.as_str(), "dd: writing 'disk.img': No space left on device");
        assert_eq!(gnu9.as_str(), "dd: error writing 'disk.img': No space left on device");
        assert_eq!(bb.as_str(), "dd: write error to 'disk.img': No space left on device");
        assert_ne!(gnu8.as_str(), gnu9.as_str());
        assert_ne!(gnu9.as_str(), bb.as_str());
        let c0 = match case_at(0) {
            Some(v) => v,
            None => panic!("slot 0"),
        };
        let m0 = message(&c0.shape, &c0.actual());
        let m2 = match case_at(2) {
            Some(c) => message(&c.shape, &c.actual()),
            None => panic!("slot 2"),
        };
        assert!(m0.contains("error writing"));
        assert!(m2.contains("write error to"));
        assert!(m2.contains("recovered 3584 bytes from block geometry"));
        assert_eq!(msg_bytes_recovered("disk.img", 3584).as_str(),
            "dd byte line absent for 'disk.img': recovered 3584 bytes from block geometry");
        assert_eq!(msg_records_in(4, 0).as_str(), "4+0 records in");
        assert_eq!(msg_records_out(3, 1).as_str(), "3+1 records out");
    }

    #[test]
    fn t_human_tail_is_not_a_judgement_value() {
        assert_eq!(human_pair(0), "0 B");
        assert_eq!(human_pair(1024), "1.0 kB, 1.0 KiB");
        assert_eq!(human_pair(2048), "2.0 kB, 2.0 KiB");
        assert_eq!(human_pair(3072), "3.1 kB, 3.0 KiB");
        assert_eq!(human_pair(6144), "6.1 kB, 6.0 KiB");
        assert_eq!(human_pair(8192), "8.2 kB, 8.0 KiB");
        assert_eq!(human_pair(1537), "n/a");
        assert_eq!(human_tail_judged(), false);
        assert_eq!(msg_bytes_copied(6144).as_str(), "6144 bytes (6.1 kB, 6.0 KiB) copied");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(seg_text(DD_ESLOT).as_str(), "0x6d159");
    }

    #[test]
    fn t_detail_lines_verbatim() {
        let head = match case_at(0) {
            Some(v) => v,
            None => panic!("slot 0"),
        };
        assert_eq!(
            detail(&head).as_str(),
            "disk.img flavor=gnu9 stage=abort exit=1 bytes=6144 partial=true anchor=true note=abort-gnu9 msg=\"dd: error writing 'disk.img': No space left on device | partial block: records out 1+1, tail bytes 2048 | 2+0 records in | 1+1 records out | 6144 bytes (6.1 kB, 6.0 KiB) copied\"\n"
        );
        let bb = match case_at(2) {
            Some(v) => v,
            None => panic!("slot 2"),
        };
        assert_eq!(
            detail(&bb).as_str(),
            "disk.img flavor=busybox stage=abort exit=1 bytes=3584 partial=true anchor=true note=abort-busybox msg=\"dd: write error to 'disk.img': No space left on device | partial block: records out 3+1, tail bytes 512 | 4+0 records in | 3+1 records out | dd byte line absent for 'disk.img': recovered 3584 bytes from block geometry\"\n"
        );
        let silent = match case_at(6) {
            Some(v) => v,
            None => panic!("slot 6"),
        };
        assert!(detail(&silent).contains("stage=summary"));
        assert!(detail(&silent).contains("anchor=false"));
        assert!(detail(&silent).contains("bytes=2048"));
        let usage = match case_at(7) {
            Some(v) => v,
            None => panic!("slot 7"),
        };
        assert!(detail(&usage).contains("stage=args exit=2 bytes=0"));
        let cs = cases();
        let mut i = 0usize;
        let mut unique = String::new();
        while i < ACCEPT {
            let line = match cs.get(i) {
                Some(v) => detail(v),
                None => panic!("case"),
            };
            assert!(line.ends_with('\n'));
            assert!(!unique.contains(line.as_str()));
            unique.push_str(line.as_str());
            i += 1;
        }
        assert_eq!(unique.chars().filter(|c| *c == '\n').count(), ACCEPT);
        assert!(detail_ok());
    }

    // -----------------------------------------------------------------------
    // 矩阵、报表与注入
    // -----------------------------------------------------------------------

    #[test]
    fn t_matrix_rows_and_render_verbatim() {
        let m = Matrix::of();
        assert_eq!(m.slots(), CORPUS);
        assert_eq!(m.goods(), ACCEPT);
        assert!(m.covered());
        assert_eq!(m.perfect_rows(), 2);
        let complete = match m.row(Tier::Complete) {
            Some(v) => v,
            None => panic!("row"),
        };
        assert_eq!(complete.total, 3);
        assert_eq!(complete.good, 3);
        assert!(complete.perfect());
        let aborted = match m.row(Tier::Aborted) {
            Some(v) => v,
            None => panic!("row"),
        };
        assert_eq!(aborted.total, 8);
        assert_eq!(aborted.good, 3);
        assert_eq!(aborted.red(), 5);
        assert_eq!(aborted.perfect(), false);
        assert_eq!(m.row(Tier::Silent).unwrap_or(Row { total: 0, good: 0 }).total, 1);
        assert_eq!(
            m.render().as_str(),
            "complete total=3 good=3 red=0\nsilent total=1 good=1 red=0\naborted total=8 good=3 red=5\nrefused total=2 good=1 red=1\n"
        );
        assert!(matrix_ok());
    }

    #[test]
    fn t_matrix_row_guard_and_blank() {
        let broken = Row { total: 2, good: 5 };
        assert_eq!(broken.red(), 2);
        assert_eq!(broken.perfect(), false);
        let e = Matrix::blank();
        assert_eq!(e.slots(), 0);
        assert_eq!(e.goods(), 0);
        assert_eq!(e.perfect_rows(), 0);
        assert_eq!(e.covered(), false);
        assert_eq!(
            e.render().as_str(),
            "complete total=0 good=0 red=0\nsilent total=0 good=0 red=0\naborted total=0 good=0 red=0\nrefused total=0 good=0 red=0\n"
        );
        assert_eq!(Row::blank().red(), 0);
        assert_eq!(Row::blank().perfect(), false);
    }

    #[test]
    fn t_report_render_is_verbatim() {
        let rep = match run() {
            Ok(v) => v,
            Err(_) => panic!("run"),
        };
        assert_eq!(
            rep.render().as_str(),
            "dd-records corpus=14 accept=8 reject=6 complete=3 silent=1 aborted=3 refused=1 match=14 rate=1000ppm\n"
        );
        assert_eq!(rep.render().as_str(), RENDER_OK);
        assert_eq!(rep.rate(), RATE_FULL);
        assert!(rep.green());
        assert_eq!(rep.gate(), Ok(()));
        assert_eq!(rep.success_cells(), 4);
        assert_eq!(rep.failure_cells(), 4);
        assert_eq!(rep.total, CORPUS as u32);
        assert_eq!(rep.accept, ACCEPT as u32);
        assert_eq!(rep.reject, REJECT as u32);
        assert_eq!(rep.matched, CORPUS as u32);
    }

    #[test]
    fn t_injection_moves_rate_off_full_green() {
        let red = match run_with(Some((0, Fault::Code))) {
            Ok(v) => v,
            Err(_) => panic!("run_with"),
        };
        assert_eq!(red.matched, CORPUS as u32 - 1);
        assert_eq!(red.rate(), RED_RATE);
        assert_eq!(red.rate(), 929);
        assert_eq!(red.green(), false);
        assert_eq!(red.gate(), Err(DD_ECOUNT));
        assert_eq!(red.accept, ACCEPT as u32 - 1);
        assert_eq!(red.reject, REJECT as u32 + 1);
        assert_eq!(red.aborted, 2);
        assert_eq!(
            red.render().as_str(),
            "dd-records corpus=14 accept=7 reject=7 complete=3 silent=1 aborted=2 refused=1 match=13 rate=929ppm\n"
        );
        let revive = match run_with(Some((8, Fault::Ok))) {
            Ok(v) => v,
            Err(_) => panic!("run_with"),
        };
        assert_eq!(revive.accept, ACCEPT as u32 + 1);
        assert_eq!(revive.refused, 2);
        assert_eq!(revive.matched, CORPUS as u32 - 1);
        assert_eq!(revive.rate(), RED_RATE);
        assert_eq!(revive.aborted, 3);
        let same = match run_with(Some((3, Fault::Ok))) {
            Ok(v) => v,
            Err(_) => panic!("run_with"),
        };
        assert_eq!(same.render().as_str(), RENDER_OK);
    }

    #[test]
    fn t_run_with_out_of_range_slot_is_rejected() {
        // 收库修正（AI-INTAKE-1107）：Report 无 PartialEq/Debug，改用 matches! 判错码。
        assert!(matches!(run_with(Some((CORPUS, Fault::Code))), Err(DD_ESLOT)));
        assert!(matches!(run_with(Some((CORPUS + 8, Fault::Ok))), Err(DD_ESLOT)));
        assert!(matches!(run_with(Some((usize::MAX, Fault::Seal))), Err(DD_ESLOT)));
        assert_eq!(Fault::Slot.code(), DD_ESLOT);
        assert!(reds_ok());
    }

    #[test]
    fn t_rate_rounding_edges() {
        let zero = Report {
            total: 14,
            accept: 0,
            reject: 14,
            complete: 0,
            silent: 0,
            aborted: 0,
            refused: 0,
            matched: 0,
        };
        assert_eq!(zero.rate(), 0);
        assert_eq!(zero.green(), false);
        assert_eq!(zero.gate(), Err(DD_ECOUNT));
        let odd = Report {
            total: 3,
            accept: 1,
            reject: 2,
            complete: 1,
            silent: 0,
            aborted: 0,
            refused: 0,
            matched: 2,
        };
        assert_eq!(odd.rate(), 667);
        let none = Report {
            total: 0,
            accept: 0,
            reject: 0,
            complete: 0,
            silent: 0,
            aborted: 0,
            refused: 0,
            matched: 0,
        };
        assert_eq!(none.rate(), 0);
        assert_eq!(none.green(), false);
    }

    // -----------------------------------------------------------------------
    // 九闸阶段机
    // -----------------------------------------------------------------------

    #[test]
    fn t_gate_only_advances_one_rank() {
        let mut g = Gate::new();
        assert_eq!(g.rank(), 0);
        assert_eq!(g.moves(), 0);
        assert_eq!(g.rejected(), 0);
        assert_eq!(g.is_closed(), false);
        assert!(g.stage().is_none());
        assert_eq!(g.step(Stage::Open), Err(DD_EBAL));
        assert_eq!(g.rank(), 0);
        assert_eq!(g.rejected(), 1);
        assert_eq!(g.step(Stage::Args), Ok(()));
        assert_eq!(g.rank(), 1);
        assert_eq!(g.stage(), Some(Stage::Args));
        assert_eq!(g.step(Stage::Args), Err(DD_EBAL));
        assert_eq!(g.step(Stage::Records), Err(DD_EBAL));
        assert_eq!(g.rank(), 1);
        assert_eq!(g.close(), Err(DD_ESTAGE));
        assert_eq!(g.rejected(), 4);
        assert_eq!(g.step(Stage::Open), Ok(()));
        assert_eq!(g.step(Stage::Seek), Ok(()));
        assert_eq!(g.step(Stage::Args), Err(DD_ESTAGE));
        assert_eq!(g.rank(), 3);
        assert_eq!(g.rejected(), 5);
        assert_eq!(g.moves(), 8);
        assert_ne!(g.moves(), g.rank());
    }

    #[test]
    fn t_gate_close_requires_last_rank_and_then_refuses_all() {
        let mut g = Gate::new();
        assert_eq!(g.walk(&Stage::all()), Ok(()));
        assert_eq!(g.rank(), STAGE_LAST_RANK);
        assert!(g.is_closed());
        assert_eq!(g.stage(), Some(Stage::Exit));
        assert_eq!(g.close(), Err(DD_ESTATE));
        assert_eq!(g.step(Stage::Exit), Err(DD_ESTATE));
        assert_eq!(g.step(Stage::Args), Err(DD_ESTATE));
        assert_eq!(g.rank(), STAGE_LAST_RANK);
        assert_eq!(g.rejected(), 3);
        let mut early = Gate::new();
        assert_eq!(early.walk(&[Stage::Args, Stage::Open]), Err(DD_ESTAGE));
        let mut fp = Gate::new();
        let before = fp.fingerprint();
        assert_eq!(fp.step(Stage::Args), Ok(()));
        assert_ne!(fp.fingerprint(), before);
        let mut same = Gate::new();
        assert_eq!(same.fingerprint(), before);
    }

    #[test]
    fn t_gate_walk_seals_only_clean_exit_rows() {
        let clean = match case_at(3) {
            Some(v) => v.actual(),
            None => panic!("slot 3"),
        };
        assert_eq!(clean.stage, Stage::Exit);
        assert_eq!(gate_walk(&clean), Ok(STAGE_LAST_RANK));
        let abort = match case_at(0) {
            Some(v) => v.actual(),
            None => panic!("slot 0"),
        };
        assert_eq!(gate_walk(&abort), Err(DD_ESTAGE));
        assert_eq!(gate_stop_rank(&abort), Stage::Abort.rank());
        let args = match case_at(7) {
            Some(v) => v.actual(),
            None => panic!("slot 7"),
        };
        assert_eq!(gate_walk(&args), Err(DD_ESTAGE));
        assert_eq!(gate_stop_rank(&args), 1);
        let summary = match case_at(6) {
            Some(v) => v.actual(),
            None => panic!("slot 6"),
        };
        assert_eq!(gate_stop_rank(&summary), Stage::Summary.rank());
        assert_eq!(gate_walk(&summary), Err(DD_ESTAGE));
        assert_eq!(gate_walk(&Outcome::blank()), Err(DD_ESTAGE));
    }

    // -----------------------------------------------------------------------
    // 账本：登记、对账、冻结、重放
    // -----------------------------------------------------------------------

    #[test]
    fn t_ledger_cells_are_the_corpus_second_statement() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.is_empty(), false);
        assert_eq!(l.ok_count(), ACCEPT);
        assert_eq!(l.err_count(), REJECT);
        assert_eq!(l.rejects(), 0);
        assert_eq!(l.audit(), Ok(()));
        assert_ne!(l.seal(), 0);
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < SAMPLES {
            let s = match l.sample(i) {
                Some(v) => v,
                None => panic!("sample"),
            };
            assert_eq!(s.slot as usize, i);
            if s.ok {
                assert_eq!(s.code, 0);
                assert_eq!(s.value, match VALS.get(vi) { Some(v) => *v, None => panic!("vals") });
                vi += 1;
            } else {
                assert_eq!(s.value, 0);
                assert_eq!(s.code, match REJECT_CODES.get(ci) { Some(v) => *v, None => panic!("codes") });
                ci += 1;
            }
            i += 1;
        }
        assert_eq!(vi, ACCEPT);
        assert_eq!(ci, REJECT);
        assert!(l.sample(SAMPLES).is_none());
        assert_eq!(l.sample(7).unwrap_or(Sample::blank()).value, RED_RATE);
        assert_eq!(l.sample(0).unwrap_or(Sample::blank()).op, OP_CORPUS);
        assert_eq!(l.sample(7).unwrap_or(Sample::blank()).op, OP_RATE);
        let mut d = 0usize;
        while d < REJECT {
            let s = match l.sample(ACCEPT + d) {
                Some(v) => v,
                None => panic!("defect cell"),
            };
            assert_eq!(s.op, OP_DEFECT);
            let c = match case_at(ACCEPT + d) {
                Some(v) => v,
                None => panic!("case"),
            };
            assert_eq!(s.code, judge(&c).code());
            d += 1;
        }
    }

    #[test]
    fn t_ledger_register_demands_the_next_slot() {
        let mut l = Ledger::new();
        assert!(l.is_empty());
        assert_eq!(l.len(), 0);
        assert_eq!(l.register(Sample::good(1, OP_CORPUS, 14)), Err(DD_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.len(), 0);
        assert!(l.sample(0).is_none());
        assert_eq!(l.register(Sample::good(0, OP_CORPUS, 14)), Ok(()));
        assert_eq!(l.len(), 1);
        assert_eq!(l.sample(0).unwrap_or(Sample::blank()).value, 14);
        assert_eq!(l.register(Sample::good(2, OP_CORPUS, 8)), Err(DD_ESLOT));
        assert_eq!(l.register(Sample::good(SAMPLES as u8, OP_CORPUS, 8)), Err(DD_ESLOT));
        assert_eq!(l.rejects(), 3);
        assert_eq!(l.len(), 1);
        assert_eq!(l.audit(), Err(DD_EBAL));
        assert_eq!(Ledger::new().audit(), Err(DD_EBAL));
        assert_eq!(Sample::blank().ok, false);
        assert_eq!(Sample::good(3, 4, 5).code, 0);
        assert_eq!(Sample::bad(3, 4, 5).value, 0);
        assert_eq!(Sample::good(1, 2, 3).seal(), Sample::good(1, 2, 3).seal());
        assert_ne!(Sample::good(1, 2, 3).seal(), Sample::bad(1, 2, 3).seal());
    }

    #[test]
    fn t_ledger_freeze_replay_and_tampering() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => panic!("freeze"),
        };
        assert_ne!(root, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.register(Sample::good(SAMPLES as u8, OP_REPLAY, 1)), Err(DD_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze(), Err(DD_EBAL));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.replay(), Err(DD_ESEAL));
        assert_eq!(Ledger::new().replay(), Err(DD_ESEAL));
        let clean_root = match build_ledger().freeze() {
            Ok(v) => v,
            Err(_) => panic!("freeze"),
        };
        assert_eq!(clean_root, root);
        let value_tampered = tampered_ledger(7, RATE_FULL);
        assert_eq!(value_tampered.audit(), Err(DD_EVALUE));
        assert_eq!(value_tampered.replay(), Err(DD_ESEAL));
        assert_ne!(value_tampered.root(), clean_root);
        let op_tampered = tampered_op_ledger(3, OP_ARGS);
        assert_eq!(op_tampered.audit(), Ok(()));
        assert_eq!(op_tampered.replay(), Err(DD_ESEAL));
        assert_eq!(op_tampered.root(), clean_root);
        assert!(ledger_ok());
    }

    #[test]
    fn t_ledger_audit_zeroes_failing_cells() {
        let mut l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.poke(9, 7), true);
        assert_eq!(l.poke(9, 7), true);
        assert_eq!(l.audit(), Err(DD_EVALUE));
        let mut m = build_ledger();
        assert_eq!(m.sample(9).unwrap_or(Sample::blank()).value, 0);
        assert_eq!(m.poke(SAMPLES, 1), false);
        assert_eq!(m.poke_op(SAMPLES, 1), false);
        let mut n = build_ledger();
        assert_eq!(n.audit(), Ok(()));
        if let Some(cell) = n.sample(0) {
            assert_eq!(n.poke(0, cell.value.saturating_add(1)), true);
        }
        assert_eq!(n.audit(), Err(DD_EVALUE));
        let mut frozen_first = build_ledger();
        let r = match frozen_first.freeze() {
            Ok(v) => v,
            Err(_) => panic!("freeze"),
        };
        assert_eq!(frozen_first.audit(), Ok(()));
        assert!(frozen_first.poke_op(5, OP_ARGS));
        assert_ne!(frozen_first.seal(), r);
    }

    #[test]
    fn t_four_injection_routes_all_caught() {
        let v = inject_four();
        assert_eq!(v.len(), 4);
        assert!(v[0]);
        assert!(v[1]);
        assert!(v[2]);
        assert!(v[3]);
        assert!(inject_all_caught());
        assert_eq!(INJECT_ROUTES, ["table", "judge", "render", "ledger"]);
        assert_eq!(INJECT_ROUTES[1], "judge");
    }

    #[test]
    fn t_thirteen_checks_named_and_green() {
        let s = run_unx11683_checks();
        assert_eq!(s.domain, "unx11683_dd_enospc_records");
        assert_eq!(s.len(), 13);
        let want = [
            "DD01-名称与段基址",
            "DD02-枚举与九闸纵线",
            "DD03-设备几何与余量",
            "DD04-形状与合法域",
            "DD05-九闸判定纵线",
            "DD06-块算术与半块账",
            "DD07-越界与邻居拒收",
            "DD08-申报对判",
            "DD09-矩阵分档与渲染",
            "DD10-语料三陈与全绿",
            "DD11-文案家族逐字",
            "DD12-释义闸门与账本",
            "DD13-冻结重放四注入",
        ];
        let mut i = 0usize;
        while i < 13 {
            let c = match s.get(i) {
                Some(v) => v,
                None => panic!("check"),
            };
            assert_eq!(c.name, want[i]);
            assert!(c.passed);
            assert_eq!(c.detail, "static");
            i += 1;
        }
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
        assert_eq!(s.passed_count(), 13);
        assert_eq!(s.red_items_text().is_empty(), true);
        assert_eq!(s.truncated(), false);
    }

    #[test]
    fn t_scene_table_is_the_third_statement() {
        let sc = scenes();
        assert_eq!(sc.len(), SCENE_COUNT);
        assert_eq!(SCENE_COUNT, 7);
        let mut anchored = 0usize;
        let mut i = 0usize;
        while i < sc.len() {
            let s = match sc.get(i) {
                Some(v) => v,
                None => panic!("scene"),
            };
            let o = define(s);
            assert_eq!(o.stage, SCENE_WANT_STAGE[i]);
            assert_eq!(o.code, SCENE_WANT_CODE[i]);
            assert_eq!(o.value, SCENE_WANT_VALUE[i]);
            assert_eq!(o.note, SCENE_WANT_NOTE[i]);
            assert_eq!(o.default, SCENE_WANT_DEFAULT[i]);
            assert_eq!(s.tag, match SCENE_TAGS.get(i) { Some(v) => *v, None => panic!("tag") });
            assert_eq!(scene_anchored(i), Some(o.anchored()));
            if o.anchored() {
                anchored += 1;
            }
            assert_eq!(o.stage.rank(), gate_stop_rank(&o));
            assert_eq!(o.default && !o.note.abort_family(), false);
            i += 1;
        }
        assert_eq!(anchored, 2);
        assert_eq!(scene_at(SCENE_COUNT).is_none(), true);
        assert_eq!(SCENE_TAGS[1], "seek-hole-full");
        assert_eq!(SCENE_TAGS[6], "status-none-abort");
        assert_eq!(SCENE_WANT_STAGE[4], Stage::Records);
        assert_eq!(SCENE_WANT_STAGE[0], Stage::Open);
    }
}

//<<APPEND>>
