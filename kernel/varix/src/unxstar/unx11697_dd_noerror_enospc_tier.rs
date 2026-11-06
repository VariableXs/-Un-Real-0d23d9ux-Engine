//! UNX-F11697 dd conv=noerror 对 ENOSPC 的档位判据
//!
//! 这张卡管的是**「出错续传」这四个字到底管到哪儿**。`dd` 的 `conv=noerror`
//! 在 GNU coreutils 手册里只有一行原文：
//!
//! ```text
//! 'noerror'
//!      Continue after read errors.
//! ```
//!
//! 一行字，被运维读成一万种意思。最常见、也最要命的一种读法是：**"我加了
//! noerror，所以这次 dd 不会被任何 I/O 错打断"**——于是一块满盘把整个救数据
//! 的任务静默成"跑完了"。上游的实现根本不是这么写的：`dd.c` 的吸收分支只挂在
//! **读**那一次 `read()` 上，写侧的 `write()` 失败直接走 `die()`；`conv=fsync`
//! 那一档甚至明写"**even if there were write errors**"也要同步，反证写侧错误
//! 从来不在"可忽略"的集合里。所以本卡的红线只有一句、但必须是绿的：
//!
//! > `conv=noerror` **不吞 ENOSPC**。
//!
//! 这句话要能判，就得把"吞"拆成可数的东西。本卡把它拆成四件互相顶不上的事实：
//!
//! ```text
//! $ dd if=/dev/sr0 of=/mnt/disk/img bs=4096 conv=noerror
//! dd: error reading '/dev/sr0': Input/output error     ← 读侧：吸收，计 skipped
//! dd: error writing '/mnt/disk/img': No space left on device  ← 写侧：当场中止
//! 1+0 records in
//! 0+0 records out
//! ```
//!
//! * **读侧被吸收 ≠ 这次运行成功**。`noerror` 只让 `dd` 继续往下跑，退出码
//!   照旧是 **1**（上游对所有 I/O 错一律 `EXIT_FAILURE`）。把"跑到底"写成
//!   "码 0"是本卡判死的第二档假绿——它比当场中止更难发现，因为盘上留下一件
//!   看着完整、实际中间被抽掉一段的像。
//! * **吸收之后紧跟一个对齐问题**。`noerror` 单独用时，坏块被跳过而不是补零，
//!   于是**此后每一个字节都往前挪了一个坏块的量**：镜像的分区表、inode 表、
//!   引导扇区全部落错偏移。`noerror,sync` 才把坏块补零到 `ibs`，把对齐钉回去。
//!   本卡把"有没有 sync"立成独立一闸（`pad`），并且要求 `sync` 缺席时注解必须
//!   落在错位档上——把错位档写成补齐档，就是在给一块已经错位的像盖"可用"的章。
//! * **写侧的 ENOSPC 有三个落点，`noerror` 一个都救不了**：第一次 `write()`
//!   当场红（`error writing`）、延迟分配下 `close()` 才红
//!   （`closing output file`）、`conv=fsync`/`fdatasync` 收尾同步才红
//!   （`fsync failed for`）。三个落点文案不同、码同为 1、errno 同为 28，
//!   唯一共同点是**都与 noerror 无关**。本卡把这三个落点各自立闸或在锚上留名，
//!   不许任何人把后两个解释成"数据其实写进去了，只是同步没跟上"。
//! * **`ENOSPC` 不允许从读侧冒出来**。读一个已存在的块设备不会"没空间"；
//!   日志里若真出现 `error reading 'x': No space left on device`，那要么是
//!   把写侧行读串了列，要么是被 `status=none` 压掉汇总后剩下的孤行。本卡把这条
//!   写成结构性谎报（`consistent=false`），落在第四档。
//!
//! `status=` 是本卡的第二条深线。`status=none` 压掉的只有那三行汇总，**不压
//! 错误文案、也不改退出码**；`status=noxfer` 只压最后一行的速率。于是一条
//! `status=none` 的红行在日志里只剩文案、没有 `records in/out`，与一条
//! `status=none` 的绿行**长得一模一样**——唯一能分开它们的是退出码。本卡把
//! "无汇总"立成不许给锚的一档，并要求 `status=none` 的行必须自带码，否则判死。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 八道闸，秩从 **1** 起算；`Gate` 只接受"前进到下一秩"，倒退、跳档、封印后
//! 再动各留痕迹。前一闸不产出事实，后一闸就没有输入，倒过来必先产生假绿。
//!
//! 1. `argv`（秩 1）——`conv=`/`iflag=`/`oflag=`/`status=` 这些 `key=value`
//!    operand 的解析。未知 conv 名（`convs=noerror` 这种拼错的 operand 也在
//!    这里）、未知 status 档、`bs=0`、`lcase` 与 `ucase` 并存，全部在这一闸退
//!    **2**，一次 `read()`/`write()` 都没发生。这一闸排最前是为了钉死：**用法
//!    错永远不许被记成 noerror 吸收**——它连块都没见过。
//! 2. `open`（秩 2）——`if=`/`of=` 的打开与创建，含 `conv=excl` 的"已存在就
//!    不许开"。`ENOENT`/`EACCES`/`EROFS`/`EEXIST` 在这里出局，盘还没写，
//!    落盘字节必为 0。本卡在这一闸只问一件事：**这一闸产不产 skipped**（产 0）。
//! 3. `read`（秩 3）——读侧那一次 `read()`。`EIO`/`ENXIO`/`ENOMEDIUM` 在这
//!    一闸定性。**`ENOSPC` 出现在读侧即结构性谎报**。它必须排在 absorb 之前：
//!    absorb 的输入就是"这一闸红了、红在哪个码上"。
//! 4. `absorb`（秩 4）——`noerror` 吸收闸。只吸收读侧错，吸收量记进 `skipped`；
//!    `noerror` 缺席时读侧错直接终止运行（这一闸不产出续传账）。吸收成功
//!    **不许**把运行改写成绿：吸收档的运行仍是红（码 1）。
//! 5. `pad`（秩 5）——`sync` 补齐闸。把被吸收的坏块补零到 `ibs`，字节算术
//!    （`in_full*ibs + pad` 与 `out_full*obs`）在这一闸对账；`sync` 缺席而
//!    吸收过坏块，输出即错位，注解必须落错位档。排在 absorb 之后：没被吸收的
//!    块没有"补"一说。
//! 6. `write`（秩 6）——写侧那一次 `write()`。**本卡的心脏**：`ENOSPC` 落在
//!    这里，`noerror` 与 `sync` 在本闸一律无效，当场中止，码 1。红线锚只在这一
//!    闸（或第 7 闸）与 `noerror` 同时在场时才允许亮。
//! 7. `fsync`（秩 7）——延迟分配与收尾同步闸：`close()` 才红的
//!    `closing output file`、`conv=fsync`/`fdatasync` 才红的 `fsync failed for`。
//!    这一闸排在 write 之后是硬要求：**先有"写过"，才谈得上"同步没跟上"**；
//!    排在它前面就会把"一次都没写出去"的现场读成"写完没同步"。
//! 8. `exit`（秩 8）——退出码与汇总结算闸。0 只给真绿；吸收过读错的运行给 1；
//!    写侧/同步侧红给 1；用法给 2。`status=none` 在本闸只允许压掉汇总行，
//!    压掉错误文案或改码即判死。闸序、errno、skipped 三件都自洽，才许落锚。
//!
//! ## 关键差异（本卡深度所在）
//!
//! * **与 F11489（dd conv= 族与 status 档，段基址 `0x20e60`，位距 `0x4c5b0`）
//!   的联签边界**：F11489 判 **conv 位集本身的合法性**——`lcase`/`ucase` 互斥、
//!   未知 conv 名、空列表、`sync` 源超块、`excl` 撞已存在、`status=` 三档；
//!   本卡**不重判**这些解析对错，只在 `argv` 闸复用它的结论，把"位集合法"当
//!   外部事实引进来。本卡的独有一维是**这些位在满盘现场的效力**：同一组位，
//!   F11489 说"这组合能解析"，本卡说"这组合救不了写侧 28"。两卡不互相 import，
//!   段位与位距由本卡的 `ANCHOR_F11489_SEG`/`GAP` 声明并由 `anchors_agree()`
//!   复算对齐——这是卡片明写的联签要求。
//! * **与 F11683（dd ENOSPC 中止与半块账，段基址 `0x6d150`，位距 `0x2c0`）的
//!   边界**：F11683 判**块算术**（`records in/out` 的 `N+M`、partial 标记、
//!   `bs`/`ibs`/`obs`、`skip`/`seek`/`count` 与落盘/丢失字节），并且**点名**把
//!   `conv=noerror`/`sync` 的行在自己的 `args` 闸判成"不裁定"，交给本卡。
//!   本卡正好反向：`records` 的逐格对账**不在本卡重判**，本卡只在 absorb/pad
//!   两闸算 `skipped` 与补齐字节。同一现场，F11683 报"1+1 records out、6000
//!   字节落盘"，本卡报"noerror 吸收了 1 个坏块、sync 未在场、输出自该点错位
//!   4096 字节、写侧 28 仍当场中止"。两套数字各管一件事，互相顶替即错位。
//! * **与 F11681（ENOSPC 总锚，段基址 `0x6d130`，位距 `0x2e0`）的边界**：总锚
//!   判一次 `write()`/`fsync()` 的 (返回, errno) 配对是否正确；本卡判**这次
//!   配对被工具解释成了什么**（吸收 / 中止 / 迟到）。本卡只在 `errno==28` 与
//!   总锚同数这一维上署名，不裁总锚的账。
//! * **与 F11688（touch/mkdir 建件，位距 `0x200`）的边界**：那边一次 `write`
//!   都没有，本卡反过来——**没有块几何就没有吸收**，本卡不许用建件语义松绑。
//! * **与 F11698（工具退出码矩阵，段基址 `0x6d420`，位距 `0x10`，直接后继）的
//!   边界**：本卡只裁 `dd` 一个工具的 (0,1,2) 三档码怎么被 noerror 影响；
//!   F11698 裁**跨 12 个工具**的码归档，不重判吸收语义。dd 那一格在 F11698
//!   的矩阵里是 (文件错 1 / 用法错 2)，两卡各判各的层。
//! * **本卡的排他职责**：① 「noerror 不吞 ENOSPC」这条红线锚（只有 noerror
//!   在场 + 空间类 errno 落在 write/fsync 两闸之一 + 判成硬中止，锚才许亮）；
//!   ② 吸收后的退出码仍是 1（把跑到底写成成功是本卡第二颗钉）；③ `noerror`
//!   与 `sync` 的错位分档（补齐 vs 抽块，注解必须与位集一致）；④ 读侧 ENOSPC
//!   结构性谎报档；⑤ `status=none` 的假绿压账档。
//! * **谁判死谁放行**：位集合法、吸收只发生在读侧、补齐档与 `sync` 在场配对、
//!   写侧 28 落 write/fsync 闸并硬中止、码与闸序自洽、红线锚只在该亮时亮 →
//!   放行并落锚；把写侧 28 写成被吸收、把吸收后的运行写成码 0、`sync` 缺席却
//!   申报已补齐、读侧冒出 28、`status=none` 顺手压掉错误文案 → 一律判死。
//!
//! 判据六维：结束阶段（gate）、errno（code）、skipped 字节（value）、吸收/补齐/
//! 文案家族（note）、红线锚（default）、成败申报（estate）。
//! 判序固定：状态 → 阶段 → errno → skipped → 注解 → 锚位。
//!
//! 语料 14 槽：8 槽申报正确（纯复制 1、noerror 空转 1、吸收 1、吸收+补齐 1、
//! 红线 2、迟到同步 1、用法 1），6 槽一槽只坏一维，依次挂 Estate/Stage/Code/
//! Value/Note/Default。账本 14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码，
//! 冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11697）。
pub const SEG_BASE: u32 = 0x6d410;

/// 交叉署名：F11489「dd conv= 族与 status 档」的段基址（卡片要求联签的那一张）。
/// 本卡**不 import** 那个模块的任何符号，只把它的段位与位距原样声明一遍，
/// 由 `anchors_agree()` 复算对齐。
pub const ANCHOR_F11489_SEG: u32 = 0x20e60;
/// F11489 与本卡的位距（0x6d410 - 0x20e60）。
pub const ANCHOR_F11489_GAP: u32 = 0x4c5b0;

/// 交叉署名：F11683「dd ENOSPC 中止与半块账」的段基址（本卡的点名移交方）。
pub const ANCHOR_F11683_SEG: u32 = 0x6d150;
/// F11683 与本卡的位距（0x6d410 - 0x6d150）。
pub const ANCHOR_F11683_GAP: u32 = 0x2c0;

/// 交叉署名：F11681「ENOSPC 总锚」的段基址。
pub const ANCHOR_F11681_SEG: u32 = 0x6d130;
/// F11681 与本卡的位距（0x6d410 - 0x6d130）。
pub const ANCHOR_F11681_GAP: u32 = 0x2e0;

/// 交叉署名：F11698「工具退出码矩阵」的段基址（本卡的直接后继）。
pub const ANCHOR_F11698_SEG: u32 = 0x6d420;
/// F11698 与本卡的位距（0x6d420 - 0x6d410）。
pub const ANCHOR_F11698_GAP: u32 = 0x10;

/// 与 F11681/F11694 在 errno 面的署名：ENOSPC 在 Linux 就是 28，三卡同数。
pub const ANCHOR_ENOSPC: i32 = 28;
/// 与 F11694 在文案面的署名：只认这一串 strerror 原文，逐字。
pub const ANCHOR_ERRNO_TEXT: &str = "No space left on device";
/// 与 F11683 在退出码面的署名：工具面 I/O 错统一退 1，本卡不许自造。
pub const ANCHOR_EXIT_IO: u32 = 1;

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

/// 闸门道数（秩 1..8）。
pub const STAGE_COUNT: u32 = 8;

/// 最后一道闸的秩（封印只接受站在这里的账）。
pub const STAGE_LAST_RANK: u32 = 8;

/// 账本里统计格的个数（红档格与缺陷格之外）。
pub const STAT_CELLS: usize = 7;

/// 红档率标定值：14 格里恰好中 13 格时的千分率 `(13*1000 + 14/2) / 14`。
pub const RED_RATE: u32 = 929;

/// 全绿千分率。
pub const RATE_FULL: u32 = 1000;

/// 档位矩阵行数（conv 档位四行：无 / noerror / sync / noerror+sync）。
pub const CONV_ROW_COUNT: usize = 4;

/// 档位矩阵列数（错误落点四列：无错 / 读侧 EIO / 写侧 28 / 收尾同步 28）。
pub const FALL_COL_COUNT: usize = 4;

/// 档位矩阵总格数（4×16，一格都不许悬空）。
pub const CONV_CELLS: usize = 16;

// ---------------------------------------------------------------------------
// errno（POSIX / Linux 真值；本卡认得的邻居码一个都不许冒充 ENOSPC）
// ---------------------------------------------------------------------------

/// 设备无空间（strerror: `No space left on device`）。
pub const ERR_ENOSPC: i32 = 28;
/// 输入输出错（strerror: `Input/output error`）——读侧吸收闸的正主。
pub const ERR_EIO: i32 = 5;
/// 没有这个设备/地址（strerror: `No such device or address`）。
pub const ERR_ENXIO: i32 = 6;
/// 路径上没这一层（strerror: `No such file or directory`）。
pub const ERR_ENOENT: i32 = 2;
/// 权限不足（strerror: `Permission denied`）。
pub const ERR_EACCES: i32 = 13;
/// 参数不合法（strerror: `Invalid argument`）。
pub const ERR_EINVAL: i32 = 22;
/// 文件描述符不对（strerror: `Bad file descriptor`）。
pub const ERR_EBADF: i32 = 9;
/// 只读文件系统（strerror: `Read-only file system`）。
pub const ERR_EROFS: i32 = 30;
/// 已存在（strerror: `File exists`）——`conv=excl` 撞的那一档。
pub const ERR_EEXIST: i32 = 17;
/// 配额超限（strerror: `Quota exceeded`）——邻居，"盘还满得起来"。
pub const ERR_EDQUOT: i32 = 122;
/// 没插介质（strerror: `No medium found`）——光驱救数据的常客。
pub const ERR_ENOMEDIUM: i32 = 123;
/// 值超出类型上限（strerror: `Value too large for defined data type`）。
pub const ERR_EOVERFLOW: i32 = 75;
/// 成功时 errno 应停放的零值。
pub const ERR_NONE: i32 = 0;

/// 本卡认得的 errno 全集大小（含零值）。
pub const ERRNO_SET_SIZE: usize = 13;

/// 真"空间类" errno 的个数：ENOSPC 与 EDQUOT 两档（后者的档归 F11695）。
pub const SPACE_ERRNO_COUNT: usize = 2;

/// 读侧允许出现的错误码个数（EIO/ENXIO/ENOMEDIUM/EINVAL/EOVERFLOW）。
pub const READ_ERRNO_COUNT: usize = 5;

// ---------------------------------------------------------------------------
// 退出码（GNU coreutils 与 busybox 的真值；本卡只认 dd 一家的三档）
// ---------------------------------------------------------------------------

/// 全部落定，正常收尾。
pub const EXIT_OK: u32 = 0;
/// 一切 I/O 错：读侧被吸收也照样是这个码。
pub const EXIT_IO: u32 = 1;
/// 用法错误：operand 本身不合法，一次 `read()`/`write()` 都没发生。
pub const EXIT_USAGE: u32 = 2;
/// 本卡不裁定的占位码：故意取一个撞不上的值，越界行不可能被读成正常行。
pub const EXIT_UNJUDGED: u32 = 99;

/// 退出码档位数量（0/1/2 三档真实码 + 1 档不裁定）。
pub const EXIT_RANKS: usize = 4;

/// `ibs`/`obs` 的下界：`bs=0` 在 argv 闸就退 2。
pub const BLOCK_MIN: u32 = 1;

/// `ibs`/`obs` 的上界（本卡语料只到 1 MiB，超出即形状非法）。
pub const BLOCK_MAX: u32 = 1_048_576;

/// 一次运行最多几个坏块（超出即形状非法；本卡语料最多 3 个）。
pub const BAD_BLOCKS_MAX: u32 = 8;

/// 记录数上限（本卡语料最深 4096 块，超出即形状非法）。
pub const RECORD_MAX: u32 = 8192;

// ---------------------------------------------------------------------------
// conv= 位集（本卡认得的八位；解析对错归 F11489，效力归本卡）
// ---------------------------------------------------------------------------

/// `conv=noerror`：读错续传。**只**管读侧。
pub const CONV_NOERROR: u8 = 0x01;
/// `conv=sync`：每条输入记录补零到 `ibs`；与 noerror 同开才保住对齐。
pub const CONV_SYNC: u8 = 0x02;
/// `conv=notrunc`：不清空目的件；本卡只把它当打开语义，不裁其后果。
pub const CONV_NOTRUNC: u8 = 0x04;
/// `conv=fsync`：收尾 `fsync()`，手册原话"even if there were write errors"。
pub const CONV_FSYNC: u8 = 0x08;
/// `conv=fdatasync`：收尾 `fdatasync()`，不保元数据。
pub const CONV_FDATASYNC: u8 = 0x10;
/// `conv=sparse`：把全零块 seek 过去；与吸收同开时对齐账要重算。
pub const CONV_SPARSE: u8 = 0x20;
/// `conv=excl`：目的件已存在就不许开。
pub const CONV_EXCL: u8 = 0x40;
/// `conv=swab`：逐字节对调；本卡只认它在 argv 闸的合法性。
pub const CONV_SWAB: u8 = 0x80;

/// 本卡认得的 conv 位全集。
pub const CONV_KNOWN: u8 = CONV_NOERROR
    | CONV_SYNC
    | CONV_NOTRUNC
    | CONV_FSYNC
    | CONV_FDATASYNC
    | CONV_SPARSE
    | CONV_EXCL
    | CONV_SWAB;

/// conv 位数（语料与谓词按位遍历用）。
pub const CONV_BIT_COUNT: usize = 8;

/// conv 位表（顺序与 `CONV_*` 值一致）。
pub const CONV_BITS: [u8; CONV_BIT_COUNT] = [
    CONV_NOERROR,
    CONV_SYNC,
    CONV_NOTRUNC,
    CONV_FSYNC,
    CONV_FDATASYNC,
    CONV_SPARSE,
    CONV_EXCL,
    CONV_SWAB,
];

/// 吸收位：只有这一位能把读侧错误变成续传。
pub const CONVE_ABSORB: u8 = CONV_NOERROR;

/// 补齐位：只有这一位能把被吸收的坏块补回原长度。
pub const CONVE_PAD: u8 = CONV_SYNC;

/// 收尾同步位：`fsync`/`fdatasync` 两位之一即进入第 7 闸的迟到档。
pub const CONVE_LATE: u8 = CONV_FSYNC | CONV_FDATASYNC;

/// 与本卡红线无关的位（不影响吸收/补齐/迟到三闸的判定）。
pub const CONVE_NEUTRAL: u8 = CONV_NOTRUNC | CONV_SPARSE | CONV_EXCL | CONV_SWAB;

// ---------------------------------------------------------------------------
// 错误码（13 个，SEG_BASE + 0x01..0x0d）
// ---------------------------------------------------------------------------

/// 01 阶段申报不符：28 落在哪道闸说错了（写侧红却申报成被 absorb 闸吸收）。
pub const DN_ESTAGE: u32 = SEG_BASE + 0x01;
/// 02 errno 申报不符：EDQUOT/EROFS 等邻居码冒充 28，或 28 写成邻居码。
pub const DN_ECODE: u32 = SEG_BASE + 0x02;
/// 03 skipped 申报不符：吸收量数错，或把写侧失败的字节也计进 skipped。
pub const DN_EVALUE: u32 = SEG_BASE + 0x03;
/// 04 注解申报不符：错位档写成补齐档、文案家族混认、status 档错位。
pub const DN_ENOTE: u32 = SEG_BASE + 0x04;
/// 05 锚位申报不符：红线锚该亮不亮、不该亮却亮（noerror 缺席却亮即在此判死）。
pub const DN_EDEFAULT: u32 = SEG_BASE + 0x05;
/// 06 几何不符：块大小/坏块数/吸收量三者算不出同一段字节。
pub const DN_EGEOM: u32 = SEG_BASE + 0x06;
/// 07 状态违例：成败互报、闸门封印后再动、账本冻结后再登记。
pub const DN_ESTATE: u32 = SEG_BASE + 0x07;
/// 08 计数不符：报表不满、矩阵不平、档位格数对不上 4×4。
pub const DN_ECOUNT: u32 = SEG_BASE + 0x08;
/// 09 槽位非法：注入槽越界，或账本登记时 slot != len。
pub const DN_ESLOT: u32 = SEG_BASE + 0x09;
/// 10 指纹不符：冻结后重放对不上，或篡改留痕。
pub const DN_ESEAL: u32 = SEG_BASE + 0x0a;
/// 11 无观测：槽号超成语料、期望表缺格。
pub const DN_ENOBS: u32 = SEG_BASE + 0x0b;
/// 12 矩阵不平：conv 档位矩阵出现空洞格或行列对不上。
pub const DN_EMATRIX: u32 = SEG_BASE + 0x0c;
/// 13 账本不平：格数、成败比或对账表任一失衡。
pub const DN_EBAL: u32 = SEG_BASE + 0x0d;

/// 13 个错误码的有序全集（检查器与账本共用）。
pub const ALL_CODES: [u32; 13] = [
    DN_ESTAGE,
    DN_ECODE,
    DN_EVALUE,
    DN_ENOTE,
    DN_EDEFAULT,
    DN_EGEOM,
    DN_ESTATE,
    DN_ECOUNT,
    DN_ESLOT,
    DN_ESEAL,
    DN_ENOBS,
    DN_EMATRIX,
    DN_EBAL,
];

/// 错误码名字（渲染与释义用，顺序同 `ALL_CODES`）。
pub const CODE_NAMES: [&str; 13] = [
    "DN_ESTAGE",
    "DN_ECODE",
    "DN_EVALUE",
    "DN_ENOTE",
    "DN_EDEFAULT",
    "DN_EGEOM",
    "DN_ESTATE",
    "DN_ECOUNT",
    "DN_ESLOT",
    "DN_ESEAL",
    "DN_ENOBS",
    "DN_EMATRIX",
    "DN_EBAL",
];

// ---------------------------------------------------------------------------
// 账本操作码
// ---------------------------------------------------------------------------

/// 01 argv 闸统计。
pub const OP_ARGV: u8 = 0x01;
/// 02 open 闸统计。
pub const OP_OPEN: u8 = 0x02;
/// 03 read 闸统计。
pub const OP_READ: u8 = 0x03;
/// 04 absorb 闸统计。
pub const OP_ABSORB: u8 = 0x04;
/// 05 pad 闸统计。
pub const OP_PAD: u8 = 0x05;
/// 06 write 闸统计。
pub const OP_WRITE: u8 = 0x06;
/// 07 fsync 闸统计。
pub const OP_FSYNC: u8 = 0x07;
/// 08 语料格数。
pub const OP_CORPUS: u8 = 0x08;
/// 09 红档率。
pub const OP_RATE: u8 = 0x09;
/// 10 缺陷码格。
pub const OP_DEFECT: u8 = 0x0a;
/// 11 重放格。
pub const OP_REPLAY: u8 = 0x0b;

/// 操作码个数。
pub const OP_COUNT: usize = 11;

/// 操作码表（遍历用）。
pub const OP_BITS: [u8; OP_COUNT] = [
    OP_ARGV,
    OP_OPEN,
    OP_READ,
    OP_ABSORB,
    OP_PAD,
    OP_WRITE,
    OP_FSYNC,
    OP_CORPUS,
    OP_RATE,
    OP_DEFECT,
    OP_REPLAY,
];

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

/// 折入一个布尔。
fn fold_bool(h: u64, v: bool) -> u64 {
    fold_byte(h, if v { 1 } else { 0 })
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
        if n > 8 {
            break;
        }
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
        return s;
    }
    dec_text(v as u32)
}

/// 无符号十进制文本（对外）。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// 布尔文本。
fn bool_text(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

/// 段位文本：固定 `0x6d4xx` 式样，宽度钉死，防止指纹里段位串位。
pub fn seg_text(v: u32) -> String {
    let mut s = String::from("0x");
    s.push_str(hex_text(v, 5).as_str());
    s
}

/// errno 与名字拼成一格：`28/ENOSPC`。
pub fn errno_pair_text(code: i32) -> String {
    let mut s = i32_text(code);
    s.push('/');
    s.push_str(errno_name(code));
    s
}

/// `N+M` 记账文本（records in/out 的那一格，partial 给 1）。
pub fn nm_text(full: u32, partial: bool) -> String {
    let mut s = dec_text(full);
    s.push('+');
    s.push_str(dec_text(if partial { 1 } else { 0 }).as_str());
    s
}

/// skipped 文本：`skipped=1024`。
pub fn skipped_text(n: u32) -> String {
    let mut s = String::from("skipped=");
    s.push_str(dec_text(n).as_str());
    s
}

// ---------------------------------------------------------------------------
// strerror 与 errno 名（逐字；与 F11694 联签的那一串在本卡只出现一次）
// ---------------------------------------------------------------------------

/// strerror 原文表：只收本卡认得的 13 个 errno。
pub fn strerror(code: i32) -> &'static str {
    match code {
        ERR_NONE => "Success",
        ERR_ENOENT => "No such file or directory",
        ERR_EIO => "Input/output error",
        ERR_ENXIO => "No such device or address",
        ERR_EBADF => "Bad file descriptor",
        ERR_EACCES => "Permission denied",
        ERR_EEXIST => "File exists",
        ERR_EINVAL => "Invalid argument",
        ERR_ENOSPC => "No space left on device",
        ERR_EROFS => "Read-only file system",
        ERR_EOVERFLOW => "Value too large for defined data type",
        ERR_EDQUOT => "Quota exceeded",
        ERR_ENOMEDIUM => "No medium found",
        _ => "Unknown error",
    }
}

/// errno 名字表（大写宏名式样，用于判据行与 ktest）。
pub fn errno_name(code: i32) -> &'static str {
    match code {
        ERR_NONE => "SUCCESS",
        ERR_ENOENT => "ENOENT",
        ERR_EIO => "EIO",
        ERR_ENXIO => "ENXIO",
        ERR_EBADF => "EBADF",
        ERR_EACCES => "EACCES",
        ERR_EEXIST => "EEXIST",
        ERR_EINVAL => "EINVAL",
        ERR_ENOSPC => "ENOSPC",
        ERR_EROFS => "EROFS",
        ERR_EOVERFLOW => "EOVERFLOW",
        ERR_EDQUOT => "EDQUOT",
        ERR_ENOMEDIUM => "ENOMEDIUM",
        _ => "UNKNOWN",
    }
}

/// 本卡认得的 errno 全集（顺序与 `strerror` 分支一致）。
pub const KNOWN_ERRNOS: [i32; ERRNO_SET_SIZE] = [
    ERR_NONE,
    ERR_ENOENT,
    ERR_EIO,
    ERR_ENXIO,
    ERR_EBADF,
    ERR_EACCES,
    ERR_EEXIST,
    ERR_EINVAL,
    ERR_ENOSPC,
    ERR_EROFS,
    ERR_EOVERFLOW,
    ERR_EDQUOT,
    ERR_ENOMEDIUM,
];

/// 空间类 errno 两档（ENOSPC 与 EDQUOT）。
pub const SPACE_ERRNOS: [i32; SPACE_ERRNO_COUNT] = [ERR_ENOSPC, ERR_EDQUOT];

/// 读侧可出现的 errno 五档。
pub const READ_ERRNOS: [i32; READ_ERRNO_COUNT] = [
    ERR_EIO,
    ERR_ENXIO,
    ERR_ENOMEDIUM,
    ERR_EINVAL,
    ERR_EOVERFLOW,
];

/// 写侧/同步侧可出现的 errno 四档（空间类 + 两类邻居）。
pub const WRITE_ERRNOS: [i32; 4] = [ERR_ENOSPC, ERR_EDQUOT, ERR_EIO, ERR_EBADF];

/// errno 是否在本卡认得的集合里。
pub fn errno_known(code: i32) -> bool {
    let mut i = 0usize;
    while i < KNOWN_ERRNOS.len() {
        if let Some(c) = KNOWN_ERRNOS.get(i) {
            if *c == code {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 是否空间类（本卡红线只跟这一族有关）。
pub fn errno_space_family(code: i32) -> bool {
    let mut i = 0usize;
    while i < SPACE_ERRNOS.len() {
        if let Some(c) = SPACE_ERRNOS.get(i) {
            if *c == code {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 是否读侧合法的 errno（读侧不许出现 28，这一条是本卡的结构闸）。
pub fn errno_read_family(code: i32) -> bool {
    if code == ERR_NONE {
        return true;
    }
    let mut i = 0usize;
    while i < READ_ERRNOS.len() {
        if let Some(c) = READ_ERRNOS.get(i) {
            if *c == code {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// 文案：逐字复刻上游真实报错措辞
// ---------------------------------------------------------------------------

/// GNU coreutils 8.x 写侧失败原文：`dd: writing '<dst>': <strerror>`。
pub fn msg_gnu8_write(dst: &str, code: i32) -> String {
    let mut s = String::from("dd: writing '");
    s.push_str(dst);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU coreutils 9.x 写侧失败原文：`dd: error writing '<dst>': <strerror>`。
pub fn msg_gnu9_write(dst: &str, code: i32) -> String {
    let mut s = String::from("dd: error writing '");
    s.push_str(dst);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// busybox 写侧失败原文：`dd: write error to '<dst>': <strerror>`。
pub fn msg_bb_write(dst: &str, code: i32) -> String {
    let mut s = String::from("dd: write error to '");
    s.push_str(dst);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU 读侧失败原文：`dd: error reading '<src>': <strerror>`。
pub fn msg_gnu_read(src: &str, code: i32) -> String {
    let mut s = String::from("dd: error reading '");
    s.push_str(src);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// busybox 读侧失败原文：`dd: read error from '<src>': <strerror>`。
pub fn msg_bb_read(src: &str, code: i32) -> String {
    let mut s = String::from("dd: read error from '");
    s.push_str(src);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU 9.x 的 `close()` 迟到失败原文：
/// `dd: closing output file '<dst>': <strerror>`。
pub fn msg_gnu_close_output(dst: &str, code: i32) -> String {
    let mut s = String::from("dd: closing output file '");
    s.push_str(dst);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// `conv=fsync`/`fdatasync` 收尾同步失败原文：
/// `dd: fsync failed for '<dst>': <strerror>`。
pub fn msg_gnu_fsync_failed(dst: &str, code: i32) -> String {
    let mut s = String::from("dd: fsync failed for '");
    s.push_str(dst);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// 打开失败原文：`dd: failed to open '<dst>': <strerror>`。
pub fn msg_gnu_open_failed(name: &str, code: i32) -> String {
    let mut s = String::from("dd: failed to open '");
    s.push_str(name);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// 未知 operand 原文：`dd: unrecognized operand '<op>'`。
pub fn msg_unrecognized_operand(op: &str) -> String {
    let mut s = String::from("dd: unrecognized operand '");
    s.push_str(op);
    s.push('\'');
    s
}

/// `bs=0` 原文：`dd: invalid number: '0'`。
pub fn msg_invalid_number(text: &str) -> String {
    let mut s = String::from("dd: invalid number: '");
    s.push_str(text);
    s.push('\'');
    s
}

/// 汇总第一行原文：`N+M records in`。
pub fn msg_records_in(full: u32, partial: bool) -> String {
    let mut s = nm_text(full, partial);
    s.push_str(" records in");
    s
}

/// 汇总第二行原文：`N+M records out`。
pub fn msg_records_out(full: u32, partial: bool) -> String {
    let mut s = nm_text(full, partial);
    s.push_str(" records out");
    s
}

/// 汇总第三行前半原文：`<n> bytes copied`（速率与人类可读段由外层补）。
pub fn msg_copied(bytes: u32) -> String {
    let mut s = dec_text(bytes);
    s.push_str(" bytes copied");
    s
}

/// GNU coreutils 手册 `dd invocation` 一节逐字抄来的汇总三行，用于钉版式。
pub const LIT_SUMMARY_MANUAL: [&str; 3] = [
    "7287+1 records in",
    "116608+0 records out",
    "59703296 bytes (60 MB, 57 MiB) copied, 0.0427974 s, 1.4 GB/s",
];

/// GNU coreutils 手册里 `conv=noerror` 那一行的逐字原文（判据的第一手依据）。
pub const LIT_CONV_NOERROR: &str = "'noerror' Continue after read errors.";

/// GNU coreutils 手册里 `conv=fsync` 那一行的逐字原文（写侧错误不在可忽略集
/// 合里的直接反证）。
pub const LIT_CONV_FSYNC: &str =
    "Synchronize output data and metadata just before finishing, even if there were write errors.";

/// 满盘现场的四行原文（本卡判据的样板行，逐字）。
pub const LIT_REDLINE_FOUR_LINES: [&str; 4] = [
    "dd: error reading '/dev/sr0': Input/output error",
    "dd: error writing '/mnt/disk/img': No space left on device",
    "1+0 records in",
    "0+0 records out",
];

// ---------------------------------------------------------------------------
// 文案家族
// ---------------------------------------------------------------------------

/// 上游家族：三式的写侧/读侧文案前缀互不相同，混认即复原不了现场。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flavor {
    /// GNU coreutils 8.x：`dd: writing 'X': <strerror>`。
    Gnu8,
    /// GNU coreutils 9.x：`dd: error writing 'X': <strerror>`。
    Gnu9,
    /// busybox：`dd: write error to 'X': <strerror>`，且不打印字节行。
    Busybox,
}

impl Flavor {
    /// 家族全集。
    pub fn all() -> [Flavor; 3] {
        [Flavor::Gnu8, Flavor::Gnu9, Flavor::Busybox]
    }

    /// 家族名。
    pub fn name(&self) -> &'static str {
        match self {
            Flavor::Gnu8 => "gnu8-dd",
            Flavor::Gnu9 => "gnu9-dd",
            Flavor::Busybox => "busybox-dd",
        }
    }

    /// 是否 GNU 两家之一。
    pub fn gnu(&self) -> bool {
        matches!(self, Flavor::Gnu8 | Flavor::Gnu9)
    }

    /// 写侧文案的动词段（逐字，不含前缀与路径）。
    pub fn write_verb(&self) -> &'static str {
        match self {
            Flavor::Gnu8 => "writing",
            Flavor::Gnu9 => "error writing",
            Flavor::Busybox => "write error to",
        }
    }

    /// 读侧文案的动词段（逐字）。
    pub fn read_verb(&self) -> &'static str {
        match self {
            Flavor::Gnu8 | Flavor::Gnu9 => "error reading",
            Flavor::Busybox => "read error from",
        }
    }

    /// 是否打印 `N bytes ... copied` 那一行。busybox 不打印，本卡要靠块几何
    /// 复原字节，而不是跟着日志说"未知"。
    pub fn prints_bytes_line(&self) -> bool {
        matches!(self, Flavor::Gnu8 | Flavor::Gnu9)
    }

    /// 这一家的 `noerror` 会不会漏到写侧。三家都不会，所以恒为 false；
    /// 出现 true 说明有人在某一家上给 noerror 开了后门。
    pub fn noerror_leaks_to_write(&self) -> bool {
        false
    }

    /// 吸收位（`conv=noerror`）在本家族是否真的只作用于读侧。
    pub fn absorb_read_only(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// status= 档
// ---------------------------------------------------------------------------

/// `status=` 档：只影响 stderr 上的汇总，**不影响错误文案、不影响退出码**。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// 未给 `status=`（默认三行汇总都在）。
    Plain,
    /// `status=none`：三行汇总全压。
    None,
    /// `status=noxfer`：只压最后一行的速率与字节。
    Noxfer,
    /// `status=progress`：逐块刷新最后一行。
    Progress,
    /// `status=numeric`：只出数字式汇总。
    Numeric,
    /// 未知 `status=` 值：归 F11489 的解析错，本卡只当 argv 闸的用法档引进。
    Unknown,
}

impl Status {
    /// 全集（顺序固定）。
    pub fn all() -> [Status; 6] {
        [
            Status::Plain,
            Status::None,
            Status::Noxfer,
            Status::Progress,
            Status::Numeric,
            Status::Unknown,
        ]
    }

    /// 档名（`status=` 后面那一串，逐字）。
    pub fn token(&self) -> &'static str {
        match self {
            Status::Plain => "plain",
            Status::None => "none",
            Status::Noxfer => "noxfer",
            Status::Progress => "progress",
            Status::Numeric => "numeric",
            Status::Unknown => "bogus",
        }
    }

    /// 是否压掉全部汇总行。
    pub fn hides_summary(&self) -> bool {
        matches!(self, Status::None)
    }

    /// 是否压掉速率与字节行。
    pub fn hides_rate(&self) -> bool {
        matches!(self, Status::None | Status::Noxfer)
    }

    /// 是不是本卡认得的合法档（`Unknown` 不是）。
    pub fn legal(&self) -> bool {
        !matches!(self, Status::Unknown)
    }

    /// 这一档会不会把"红行"伪装成"没发生过"——只有 `none` 有这种能力，
    /// 因为汇总没了以后唯一证据只剩退出码。
    pub fn can_disguise(&self) -> bool {
        matches!(self, Status::None)
    }
}

// ---------------------------------------------------------------------------
// 错误落点与吸收档
// ---------------------------------------------------------------------------

/// 一次运行里的错误落点（档位矩阵的列）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fall {
    /// 无错：整场跑到底。
    NoError,
    /// 读侧红：`read()` 返 -1（EIO/ENXIO/ENOMEDIUM）。
    ReadError,
    /// 写侧红：第一次 `write()` 返 -1（ENOSPC）。
    WriteError,
    /// 收尾同步红：`close()`/`fsync()` 才红（延迟分配的 28）。
    LateSync,
}

impl Fall {
    /// 列全集。
    pub fn all() -> [Fall; FALL_COL_COUNT] {
        [Fall::NoError, Fall::ReadError, Fall::WriteError, Fall::LateSync]
    }

    /// 列名。
    pub fn name(&self) -> &'static str {
        match self {
            Fall::NoError => "no-error",
            Fall::ReadError => "read-error",
            Fall::WriteError => "write-enospc",
            Fall::LateSync => "late-sync-enospc",
        }
    }

    /// 是否空间类落点（写侧与同步侧两列）——红线锚只在这两列才许亮。
    pub fn space_side(&self) -> bool {
        matches!(self, Fall::WriteError | Fall::LateSync)
    }

    /// 是否 `noerror` 管得着的落点——只有读侧一列。
    pub fn absorbable(&self) -> bool {
        matches!(self, Fall::ReadError)
    }

    /// 落点秩（0..3）。
    pub fn index(&self) -> usize {
        match self {
            Fall::NoError => 0usize,
            Fall::ReadError => 1usize,
            Fall::WriteError => 2usize,
            Fall::LateSync => 3usize,
        }
    }

    /// 反查落点。
    pub fn from_index(i: usize) -> Option<Fall> {
        match i {
            0 => Some(Fall::NoError),
            1 => Some(Fall::ReadError),
            2 => Some(Fall::WriteError),
            3 => Some(Fall::LateSync),
            _ => None,
        }
    }
}

/// 吸收档：一次读错到底被怎么处理了。本卡的第一分类维度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Absorb {
    /// `noerror` 不在场：读错即中止，没有吸收账。
    NotOffered,
    /// 被吸收且**未补齐**：此后所有字节前移一个坏块，镜像错位。
    AbsorbedRaw,
    /// 被吸收且补齐：坏块补零到 `ibs`，对齐保住。
    Padded,
    /// 读侧出现 `ENOSPC`：结构性谎报，谈不上吸收。
    Rejected,
}

impl Absorb {
    /// 全集。
    pub fn all() -> [Absorb; 4] {
        [
            Absorb::NotOffered,
            Absorb::AbsorbedRaw,
            Absorb::Padded,
            Absorb::Rejected,
        ]
    }

    /// 档名。
    pub fn name(&self) -> &'static str {
        match self {
            Absorb::NotOffered => "no-absorb",
            Absorb::AbsorbedRaw => "absorbed-raw",
            Absorb::Padded => "absorbed-padded",
            Absorb::Rejected => "read-space-lie",
        }
    }

    /// 是否发生过吸收（skipped 只有在这两档才非零）。
    pub fn happened(&self) -> bool {
        matches!(self, Absorb::AbsorbedRaw | Absorb::Padded)
    }

    /// 输出是否错位（只有 raw 档错位）。
    pub fn misaligned(&self) -> bool {
        matches!(self, Absorb::AbsorbedRaw)
    }
}

// ---------------------------------------------------------------------------
// 阶段（八道闸）
// ---------------------------------------------------------------------------

/// 本卡纵线的八道闸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 秩 1：`conv=`/`status=`/`bs=` 这些 operand 的解析。
    Argv,
    /// 秩 2：`if=`/`of=` 的打开与创建（含 `conv=excl`）。
    Open,
    /// 秩 3：读侧那一次 `read()`。
    Read,
    /// 秩 4：`noerror` 吸收闸。
    Absorb,
    /// 秩 5：`sync` 补齐闸。
    Pad,
    /// 秩 6：写侧那一次 `write()`。
    Write,
    /// 秩 7：`close()`/`fsync()` 迟到闸。
    Sync,
    /// 秩 8：退出码与汇总结算闸，也是唯一允许的封印秩。
    Exit,
}

impl Stage {
    /// 按秩列出八道闸。
    pub fn all() -> [Stage; STAGE_COUNT as usize] {
        [
            Stage::Argv,
            Stage::Open,
            Stage::Read,
            Stage::Absorb,
            Stage::Pad,
            Stage::Write,
            Stage::Sync,
            Stage::Exit,
        ]
    }

    /// 秩（1..8）。
    pub fn rank(&self) -> u32 {
        match self {
            Stage::Argv => 1u32,
            Stage::Open => 2u32,
            Stage::Read => 3u32,
            Stage::Absorb => 4u32,
            Stage::Pad => 5u32,
            Stage::Write => 6u32,
            Stage::Sync => 7u32,
            Stage::Exit => 8u32,
        }
    }

    /// 反查秩；秩 0 或 >8 一律 `None`（`None` 让调用方落 `DN_ESTAGE`）。
    pub fn from_rank(r: u32) -> Option<Stage> {
        match r {
            1 => Some(Stage::Argv),
            2 => Some(Stage::Open),
            3 => Some(Stage::Read),
            4 => Some(Stage::Absorb),
            5 => Some(Stage::Pad),
            6 => Some(Stage::Write),
            7 => Some(Stage::Sync),
            8 => Some(Stage::Exit),
            _ => None,
        }
    }

    /// 闸名（判据行与 ktest 里都用这一串）。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Argv => "argv",
            Stage::Open => "open",
            Stage::Read => "read",
            Stage::Absorb => "absorb",
            Stage::Pad => "pad",
            Stage::Write => "write",
            Stage::Sync => "fsync",
            Stage::Exit => "exit",
        }
    }

    /// 这一闸是不是"失败落点"。
    pub fn failure(&self) -> bool {
        matches!(
            self,
            Stage::Open | Stage::Read | Stage::Absorb | Stage::Pad | Stage::Write | Stage::Sync
        )
    }

    /// 这一闸之前，一次 `read()`/`write()` 都没发生（只有 argv 闸满足）。
    pub fn before_io(&self) -> bool {
        matches!(self, Stage::Argv)
    }

    /// 这一闸是不是"写侧三闸"之一（write/fsync；红线只在它们身上成立）。
    pub fn write_side(&self) -> bool {
        matches!(self, Stage::Write | Stage::Sync)
    }

    /// 这一闸是不是"读侧吸收族"（absorb/pad）——`noerror` 的全部效力范围。
    pub fn absorb_side(&self) -> bool {
        matches!(self, Stage::Absorb | Stage::Pad)
    }

    /// 这一闸产不产 skipped（只有吸收族产；写侧失败一个字节都不进 skipped）。
    pub fn makes_skipped(&self) -> bool {
        matches!(self, Stage::Absorb | Stage::Pad)
    }

    /// 落点 → 应该落在哪道闸。本卡用它顶住"28 说错落点"的假绿。
    pub fn for_fall(f: Fall) -> Stage {
        match f {
            Fall::NoError => Stage::Exit,
            Fall::ReadError => Stage::Absorb,
            Fall::WriteError => Stage::Write,
            Fall::LateSync => Stage::Sync,
        }
    }

    /// 成功账允许落在哪道闸：只有走完全线的 Exit。
    pub fn success_stage() -> Stage {
        Stage::Exit
    }

    /// 操作码 → 闸门（`OP_*` 归并档给 `None`）。
    pub fn from_op(op: u8) -> Option<Stage> {
        match op {
            OP_ARGV => Some(Stage::Argv),
            OP_OPEN => Some(Stage::Open),
            OP_READ => Some(Stage::Read),
            OP_ABSORB => Some(Stage::Absorb),
            OP_PAD => Some(Stage::Pad),
            OP_WRITE => Some(Stage::Write),
            OP_FSYNC => Some(Stage::Sync),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 注解（吸收档 × 文案家族 × status 档的组合档）
// ---------------------------------------------------------------------------

/// note 维度：一条判据行的"形态"。它把吸收结果、落点、文案家族合一，
/// 好让 `judge` 能在一格里抓住"错位写成补齐"或"三式混认"。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// 纯复制跑到底，一次错都没有。
    PlainComplete,
    /// `noerror` 在场但没东西可吸收，同样跑到底。
    NoerrorIdle,
    /// 吸收且补齐：坏块补零到 `ibs`，对齐保住。
    AbsorbedPadded,
    /// 吸收但未补齐：镜像自该点起错位。
    AbsorbedRaw,
    /// 吸收过坏块、随后写侧中止：两个事实都得在账上。
    AbsorbedThenFatal,
    /// 读侧红且 `noerror` 缺席：当场中止，没有吸收账。
    ReadFatalNoAbsorb,
    /// 红线档：`noerror` 在场，写侧 `ENOSPC` 仍当场中止。
    WriteFatalNoerror,
    /// 无 `noerror`，写侧 `ENOSPC` 中止（对照档，红线锚不亮）。
    WriteFatalPlain,
    /// 迟到档：`close()`/`fsync()` 才红的 `ENOSPC`。
    LateSyncFatal,
    /// `open()` 就红：件都建不起来，落盘字节必为 0。
    OpenFailed,
    /// operand 不合法（`dd: unrecognized operand 'X'`）。
    UnknownOperand,
    /// `conv=` 列表里有未知名。
    UnknownConv,
    /// 读侧出现 `ENOSPC`：结构性谎报。
    ReadSpaceClaim,
    /// 别的卡管的面（`bs=0`、`lcase`+`ucase` 等解析错，归 F11489）。
    OutOfScope,
}

impl Note {
    /// 全集（顺序固定，与 `NOTE_NAMES` 一一对应）。
    pub fn all() -> [Note; 14] {
        [
            Note::PlainComplete,
            Note::NoerrorIdle,
            Note::AbsorbedPadded,
            Note::AbsorbedRaw,
            Note::AbsorbedThenFatal,
            Note::ReadFatalNoAbsorb,
            Note::WriteFatalNoerror,
            Note::WriteFatalPlain,
            Note::LateSyncFatal,
            Note::OpenFailed,
            Note::UnknownOperand,
            Note::UnknownConv,
            Note::ReadSpaceClaim,
            Note::OutOfScope,
        ]
    }

    /// 反查序号（0..13）。
    pub fn index(&self) -> usize {
        match self {
            Note::PlainComplete => 0usize,
            Note::NoerrorIdle => 1usize,
            Note::AbsorbedPadded => 2usize,
            Note::AbsorbedRaw => 3usize,
            Note::AbsorbedThenFatal => 4usize,
            Note::ReadFatalNoAbsorb => 5usize,
            Note::WriteFatalNoerror => 6usize,
            Note::WriteFatalPlain => 7usize,
            Note::LateSyncFatal => 8usize,
            Note::OpenFailed => 9usize,
            Note::UnknownOperand => 10usize,
            Note::UnknownConv => 11usize,
            Note::ReadSpaceClaim => 12usize,
            Note::OutOfScope => 13usize,
        }
    }

    /// 档名（判据行里的那一串）。
    pub fn name(&self) -> &'static str {
        match self {
            Note::PlainComplete => "plain-complete",
            Note::NoerrorIdle => "noerror-idle",
            Note::AbsorbedPadded => "absorbed-padded",
            Note::AbsorbedRaw => "absorbed-raw-misaligned",
            Note::AbsorbedThenFatal => "absorbed-then-write-fatal",
            Note::ReadFatalNoAbsorb => "read-fatal-no-absorb",
            Note::WriteFatalNoerror => "write-fatal-despite-noerror",
            Note::WriteFatalPlain => "write-fatal-plain",
            Note::LateSyncFatal => "late-sync-fatal",
            Note::OpenFailed => "open-failed",
            Note::UnknownOperand => "unknown-operand",
            Note::UnknownConv => "unknown-conv",
            Note::ReadSpaceClaim => "read-space-claim",
            Note::OutOfScope => "out-of-scope",
        }
    }

    /// 是否用法档（文件系统一次都没碰）。
    pub fn usage(&self) -> bool {
        matches!(self, Note::UnknownOperand | Note::UnknownConv | Note::OutOfScope)
    }

    /// 是否"未落盘"档（用法 + 打开失败：一次 write 都没有）。
    pub fn untouched(&self) -> bool {
        matches!(
            self,
            Note::UnknownOperand | Note::UnknownConv | Note::OutOfScope | Note::OpenFailed
        )
    }

    /// 是否成功侧。
    pub fn success(&self) -> bool {
        matches!(self, Note::PlainComplete | Note::NoerrorIdle)
    }

    /// 是否吸收侧的档（skipped 只在这些档才合法地非零）。
    pub fn absorbed(&self) -> bool {
        matches!(
            self,
            Note::AbsorbedPadded | Note::AbsorbedRaw | Note::AbsorbedThenFatal
        )
    }

    /// 是否中止侧的档（本卡的中止家族）。
    pub fn abort_family(&self) -> bool {
        matches!(
            self,
            Note::ReadFatalNoAbsorb
                | Note::WriteFatalNoerror
                | Note::WriteFatalPlain
                | Note::LateSyncFatal
                | Note::AbsorbedThenFatal
        )
    }

    /// 是否写侧（含迟到）落点的档——红线锚只在这些档才许亮。
    pub fn write_side(&self) -> bool {
        matches!(
            self,
            Note::WriteFatalNoerror | Note::AbsorbedThenFatal | Note::LateSyncFatal
        )
    }

    /// 是否"补齐与对齐"必须由 `sync` 位支撑的档。
    pub fn needs_sync_bit(&self) -> bool {
        matches!(self, Note::AbsorbedPadded)
    }

    /// 是否"错位"档（`sync` 缺席且吸收过）。
    pub fn misaligned(&self) -> bool {
        matches!(self, Note::AbsorbedRaw)
    }

    /// 是否结构性谎报档。
    pub fn lie(&self) -> bool {
        matches!(self, Note::ReadSpaceClaim)
    }

    /// 是否本卡不裁定的档。
    pub fn out_of_scope(&self) -> bool {
        matches!(self, Note::OutOfScope)
    }
}

/// note 名字表（与 `Note::all()` 同序，交叉校验用）。
pub const NOTE_NAMES: [&str; 14] = [
    "plain-complete",
    "noerror-idle",
    "absorbed-padded",
    "absorbed-raw-misaligned",
    "absorbed-then-write-fatal",
    "read-fatal-no-absorb",
    "write-fatal-despite-noerror",
    "write-fatal-plain",
    "late-sync-fatal",
    "open-failed",
    "unknown-operand",
    "unknown-conv",
    "read-space-claim",
    "out-of-scope",
];

// ---------------------------------------------------------------------------
// 档位矩阵的一格（conv 档位行 × 错误落点列）
// ---------------------------------------------------------------------------

/// 一格档位：把「这一组 conv 位遇上这一个落点」的全部效力钉成七个可数的量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    /// 退出码（0 或 1，用法错不在这张表里）。
    pub exit: u32,
    /// 是否发生了吸收（只有 noerror 行 × 读侧列交叉点为真）。
    pub absorbed: bool,
    /// 是否当场中止运行。
    pub fatal: bool,
    /// 输出是否错位（吸收了却没补齐）。
    pub misaligned: bool,
    /// 吸收档。
    pub absorb: Absorb,
    /// 落点闸。
    pub stage: Stage,
    /// 注解档。
    pub note: Note,
}

/// conv 档位行标（顺序即矩阵行序）。
pub const CONV_ROWS: [u8; CONV_ROW_COUNT] = [
    0u8,
    CONV_NOERROR,
    CONV_SYNC,
    CONV_NOERROR | CONV_SYNC,
];

/// 档位矩阵行名。
pub const CONV_ROW_NAMES: [&str; CONV_ROW_COUNT] = [
    "conv=none",
    "conv=noerror",
    "conv=sync",
    "conv=noerror,sync",
];

/// 档位矩阵列标（与 `Fall::all()` 同序，读数用）。
pub const FALL_COLS: [Fall; FALL_COL_COUNT] = [
    Fall::NoError,
    Fall::ReadError,
    Fall::WriteError,
    Fall::LateSync,
];

/// **本卡的档位表**：4 行 × 4 列，16 格，一格都不许悬空。
///
/// 读这张表的唯一要点：右两列（写侧 28、迟到同步 28）在**所有四行**上都是
/// `fatal=true`、`absorbed=false`、`exit=1`——这就是「noerror 不吞 ENOSPC」
/// 在表格里的形状。若哪一格被人改成 `absorbed=true`，`matrix_ok()` 立刻变红。
pub const MATRIX: [[Cell; FALL_COL_COUNT]; CONV_ROW_COUNT] = [
    // 行 0：`conv=` 空集。任何错误都当场中止。
    [
        Cell {
            exit: EXIT_OK,
            absorbed: false,
            fatal: false,
            misaligned: false,
            absorb: Absorb::NotOffered,
            stage: Stage::Exit,
            note: Note::PlainComplete,
        },
        Cell {
            exit: EXIT_IO,
            absorbed: false,
            fatal: true,
            misaligned: false,
            absorb: Absorb::NotOffered,
            stage: Stage::Read,
            note: Note::ReadFatalNoAbsorb,
        },
        Cell {
            exit: EXIT_IO,
            absorbed: false,
            fatal: true,
            misaligned: false,
            absorb: Absorb::NotOffered,
            stage: Stage::Write,
            note: Note::WriteFatalPlain,
        },
        Cell {
            exit: EXIT_IO,
            absorbed: false,
            fatal: true,
            misaligned: false,
            absorb: Absorb::NotOffered,
            stage: Stage::Sync,
            note: Note::LateSyncFatal,
        },
    ],
    // 行 1：`conv=noerror`。读侧被吸收、但**没补齐**，输出从该点错位。
    [
        Cell {
            exit: EXIT_OK,
            absorbed: false,
            fatal: false,
            misaligned: false,
            absorb: Absorb::NotOffered,
            stage: Stage::Exit,
            note: Note::NoerrorIdle,
        },
        Cell {
            exit: EXIT_IO,
            absorbed: true,
            fatal: false,
            misaligned: true,
            absorb: Absorb::AbsorbedRaw,
            stage: Stage::Absorb,
            note: Note::AbsorbedRaw,
        },
        Cell {
            // 红线格：noerror 在场，写侧 28 照旧中止。
            exit: EXIT_IO,
            absorbed: false,
            fatal: true,
            misaligned: false,
            absorb: Absorb::NotOffered,
            stage: Stage::Write,
            note: Note::WriteFatalNoerror,
        },
        Cell {
            exit: EXIT_IO,
            absorbed: false,
            fatal: true,
            misaligned: false,
            absorb: Absorb::NotOffered,
            stage: Stage::Sync,
            note: Note::LateSyncFatal,
        },
    ],
    // 行 2：`conv=sync`。补齐位**不带**吸收力：读错仍当场中止。
    [
        Cell {
            exit: EXIT_OK,
            absorbed: false,
            fatal: false,
            misaligned: false,
            absorb: Absorb::NotOffered,
            stage: Stage::Exit,
            note: Note::PlainComplete,
        },
        Cell {
            exit: EXIT_IO,
            absorbed: false,
            fatal: true,
            misaligned: false,
            absorb: Absorb::NotOffered,
            stage: Stage::Read,
            note: Note::ReadFatalNoAbsorb,
        },
        Cell {
            exit: EXIT_IO,
            absorbed: false,
            fatal: true,
            misaligned: false,
            absorb: Absorb::NotOffered,
            stage: Stage::Write,
            note: Note::WriteFatalPlain,
        },
        Cell {
            exit: EXIT_IO,
            absorbed: false,
            fatal: true,
            misaligned: false,
            absorb: Absorb::NotOffered,
            stage: Stage::Sync,
            note: Note::LateSyncFatal,
        },
    ],
    // 行 3：`conv=noerror,sync`。唯一保住对齐的一行；写侧两列仍然致命。
    [
        Cell {
            exit: EXIT_OK,
            absorbed: false,
            fatal: false,
            misaligned: false,
            absorb: Absorb::NotOffered,
            stage: Stage::Exit,
            note: Note::NoerrorIdle,
        },
        Cell {
            exit: EXIT_IO,
            absorbed: true,
            fatal: false,
            misaligned: false,
            absorb: Absorb::Padded,
            stage: Stage::Pad,
            note: Note::AbsorbedPadded,
        },
        Cell {
            // 红线格之二：补齐位也救不了写侧 28。
            exit: EXIT_IO,
            absorbed: false,
            fatal: true,
            misaligned: false,
            absorb: Absorb::NotOffered,
            stage: Stage::Write,
            note: Note::WriteFatalNoerror,
        },
        Cell {
            exit: EXIT_IO,
            absorbed: false,
            fatal: true,
            misaligned: false,
            absorb: Absorb::NotOffered,
            stage: Stage::Sync,
            note: Note::LateSyncFatal,
        },
    ],
];

// ---------------------------------------------------------------------------
// 四档
// ---------------------------------------------------------------------------

/// 四档：按 `Outcome` 把 14 格分进四个筐。`total` 与 `good` 两维独立统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 一档：完全绿（跑到底且一个 errno 都没沾）。
    Clean,
    /// 二档：诚实红（中止或吸收后仍红，文案与位集自洽）。
    HonestFail,
    /// 三档：未落盘（用法错、打开失败、不裁定——一次 write 都没有）。
    NotTouched,
    /// 四档：谎报（结构上不自洽，判死）。
    Refused,
}

impl Tier {
    /// 按 outcome 归档。顺序：先拒谎报（结构不一致），再分未落盘/绿/诚实红。
    pub fn of(o: &Outcome) -> Tier {
        if !o.consistent {
            return Tier::Refused;
        }
        if o.note.usage() || o.note.untouched() {
            return Tier::NotTouched;
        }
        if o.ok() {
            return Tier::Clean;
        }
        Tier::HonestFail
    }

    /// 档位序号（0..3）。
    pub fn index(&self) -> usize {
        match self {
            Tier::Clean => 0usize,
            Tier::HonestFail => 1usize,
            Tier::NotTouched => 2usize,
            Tier::Refused => 3usize,
        }
    }

    /// 反查档位。
    pub fn from_index(i: usize) -> Option<Tier> {
        match i {
            0 => Some(Tier::Clean),
            1 => Some(Tier::HonestFail),
            2 => Some(Tier::NotTouched),
            3 => Some(Tier::Refused),
            _ => None,
        }
    }

    /// 档位名。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Clean => "clean",
            Tier::HonestFail => "honest-fail",
            Tier::NotTouched => "not-touched",
            Tier::Refused => "refused",
        }
    }

    /// 是否成功侧。
    pub fn success(&self) -> bool {
        matches!(self, Tier::Clean)
    }

    /// 是否允许落本卡红线锚（只有诚实红一档）。
    pub fn anchored(&self) -> bool {
        matches!(self, Tier::HonestFail)
    }

    /// 谎报档是不是"必须判死"的档。
    pub fn is_refused(&self) -> bool {
        matches!(self, Tier::Refused)
    }
}

// ---------------------------------------------------------------------------
// 判据缺陷维（judge 的返回）
// ---------------------------------------------------------------------------

/// 判序六维 + 结构性附加维。`judge` 的返回类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 六维全对。
    Ok,
    /// 成败互报（申报与实际的绿/红不一致）。
    Estate,
    /// 阶段错位（28 落在哪道闸说错了）。
    Stage,
    /// errno 错位（邻居码与 28 互换）。
    Code,
    /// skipped 错位（吸收量数错，或把写侧字节计进 skipped）。
    Value,
    /// 注解错位（补齐档与错位档混认、文案家族混认）。
    Note,
    /// 锚位错位（红线锚该亮不亮、不该亮却亮）。
    Default,
    /// 几何矛盾（块大小/坏块数/吸收量算不出同一段字节）。
    Geom,
    /// 状态违例（封印后再动、冻结后再登记）。
    State,
    /// 计数失衡（报表/矩阵/账本）。
    Count,
    /// 槽位非法。
    Slot,
    /// 指纹不符。
    Seal,
    /// 无观测（槽号越界、期望表缺格）。
    Nob,
    /// 矩阵不平（档位矩阵出现空洞格或行列对不上）。
    Matrix,
}

impl Fault {
    /// 全集（顺序固定，与 13 个错误码的映射在 `code()` 里）。
    pub fn all() -> [Fault; 14] {
        [
            Fault::Ok,
            Fault::Estate,
            Fault::Stage,
            Fault::Code,
            Fault::Value,
            Fault::Note,
            Fault::Default,
            Fault::Geom,
            Fault::State,
            Fault::Count,
            Fault::Slot,
            Fault::Seal,
            Fault::Nob,
            Fault::Matrix,
        ]
    }

    /// 缺陷维对应的错误码。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0u32,
            Fault::Estate => DN_ESTATE,
            Fault::Stage => DN_ESTAGE,
            Fault::Code => DN_ECODE,
            Fault::Value => DN_EVALUE,
            Fault::Note => DN_ENOTE,
            Fault::Default => DN_EDEFAULT,
            Fault::Geom => DN_EGEOM,
            Fault::State => DN_ESTATE,
            Fault::Count => DN_ECOUNT,
            Fault::Slot => DN_ESLOT,
            Fault::Seal => DN_ESEAL,
            Fault::Nob => DN_ENOBS,
            Fault::Matrix => DN_EMATRIX,
        }
    }

    /// 是否"绿"。
    pub fn alive(&self) -> bool {
        matches!(self, Fault::Ok)
    }

    /// 是否本卡裁定过（`Ok` 或六缺陷之一为真；Geom/State 等属结构性附加维）。
    pub fn judged(&self) -> bool {
        matches!(
            self,
            Fault::Ok
                | Fault::Estate
                | Fault::Stage
                | Fault::Code
                | Fault::Value
                | Fault::Note
                | Fault::Default
        )
    }
}

/// 缺陷维短名（账本与报表里的那一格）。
pub fn fault_short(f: Fault) -> &'static str {
    match f {
        Fault::Ok => "ok",
        Fault::Estate => "estate",
        Fault::Stage => "stage",
        Fault::Code => "code",
        Fault::Value => "value",
        Fault::Note => "note",
        Fault::Default => "default",
        Fault::Geom => "geom",
        Fault::State => "state",
        Fault::Count => "count",
        Fault::Slot => "slot",
        Fault::Seal => "seal",
        Fault::Nob => "nob",
        Fault::Matrix => "matrix",
    }
}

/// 判序：固定为 Estate → Stage → Code → Value → Note → Default。
pub const JUDGE_ORDER: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

/// 判序六维对应的错误码（顺序与 `JUDGE_ORDER` 一致，六个互不相同）。
pub const REJECT_CODES: [u32; REJECT] = [
    DN_ESTATE,
    DN_ESTAGE,
    DN_ECODE,
    DN_EVALUE,
    DN_ENOTE,
    DN_EDEFAULT,
];

/// 判序六维（与 `REJECT_CODES` 同序）。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

/// 账本统计格的对账表：7 个统计数 + 第 8 格红档率 929。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 2, 5, 1, 3, 929];

// ---------------------------------------------------------------------------
// 形状：一次 dd 运行的全部输入维度
// ---------------------------------------------------------------------------

/// 本卡输入的全部维度：命令行、块几何、三次 I/O 的返回、汇总档与申报的码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// conv 位集（本卡只认 `CONV_KNOWN` 里的八位）。
    pub conv: u8,
    /// `status=` 档。
    pub status: Status,
    /// 文案家族。
    pub flavor: Flavor,
    /// `if=` 的名字（读侧文案里逐字出现）。
    pub src: &'static str,
    /// `of=` 的名字（写侧文案里逐字出现）。
    pub dst: &'static str,
    /// argv 闸里那个不合法的 operand（用法文案用；非用法行填空串）。
    pub bad_operand: &'static str,
    /// 读块大小。
    pub ibs: u32,
    /// 写块大小。
    pub obs: u32,
    /// 打开那次返回的 errno（0 表示绿）。
    pub open_errno: i32,
    /// 读侧返回的 errno（0 表示绿）。
    pub read_errno: i32,
    /// 写侧返回的 errno（0 表示绿）。
    pub write_errno: i32,
    /// 收尾同步返回的 errno（0 表示绿）。
    pub sync_errno: i32,
    /// 坏块个数（被吸收的读错块数）。
    pub bad_blocks: u32,
    /// 读满的整块数。
    pub in_full: u32,
    /// 读侧是否留了半块（`N+1 records in` 的那个 `+1`）。
    pub in_partial: bool,
    /// 申报的 skipped 字节（本卡会按几何复算，不复算就对不上）。
    pub skipped: u32,
    /// 落盘字节。
    pub written: u32,
    /// operand 本身合不合法。
    pub argv_legal: bool,
    /// conv 列表里的名字是否全都认得（解析对错归 F11489，本卡只引进结论）。
    pub conv_names_legal: bool,
    /// 申报的退出码（exit 闸用来对账）。
    pub exit_declared: u32,
}

impl Shape {
    /// 空形状：一次全绿的 `bs=512` 纯复制，供 `const` 组装基型用。
    pub const fn blank() -> Shape {
        Shape {
            conv: 0u8,
            status: Status::Plain,
            flavor: Flavor::Gnu9,
            src: "in.img",
            dst: "out.img",
            bad_operand: "",
            ibs: 512u32,
            obs: 512u32,
            open_errno: ERR_NONE,
            read_errno: ERR_NONE,
            write_errno: ERR_NONE,
            sync_errno: ERR_NONE,
            bad_blocks: 0u32,
            in_full: 2u32,
            in_partial: false,
            skipped: 0u32,
            written: 1024u32,
            argv_legal: true,
            conv_names_legal: true,
            exit_declared: EXIT_OK,
        }
    }

    /// 取某个 conv 位。
    pub const fn has_conv(&self, bit: u8) -> bool {
        self.conv & bit != 0
    }

    /// `noerror` 是否在场（唯一能把读侧错误变成续传的位）。
    pub const fn offers_absorb(&self) -> bool {
        self.has_conv(CONV_NOERROR)
    }

    /// `sync` 是否在场（唯一能把被吸收的坏块补回原长度的位）。
    pub const fn offers_pad(&self) -> bool {
        self.has_conv(CONV_SYNC)
    }

    /// `fsync`/`fdatasync` 是否在场（第 7 闸的迟到档靠它）。
    pub const fn offers_late(&self) -> bool {
        self.conv & CONVE_LATE != 0
    }

    /// `excl` 是否在场。
    pub const fn offers_excl(&self) -> bool {
        self.has_conv(CONV_EXCL)
    }

    /// 未知 conv 位（本卡认得的八位之外）。
    pub const fn unknown_conv_bits(&self) -> u8 {
        self.conv & !CONV_KNOWN
    }

    /// 用法是否坏：命令行不合法、conv 名不合法、status 档不合法、块大小为零。
    pub fn usage_bad(&self) -> bool {
        if !self.argv_legal {
            return true;
        }
        if !self.conv_names_legal {
            return true;
        }
        if !self.status.legal() {
            return true;
        }
        if self.ibs < BLOCK_MIN || self.obs < BLOCK_MIN {
            return true;
        }
        false
    }

    /// 四个 errno 是否都在本卡认得的集合里。
    pub fn errnos_legal(&self) -> bool {
        errno_known(self.open_errno)
            && errno_known(self.read_errno)
            && errno_known(self.write_errno)
            && errno_known(self.sync_errno)
    }

    /// 块几何是否落在合法域内。
    pub fn geom_in_range(&self) -> bool {
        if self.ibs < BLOCK_MIN || self.ibs > BLOCK_MAX {
            return false;
        }
        if self.obs < BLOCK_MIN || self.obs > BLOCK_MAX {
            return false;
        }
        if self.bad_blocks > BAD_BLOCKS_MAX {
            return false;
        }
        if self.in_full > RECORD_MAX {
            return false;
        }
        true
    }

    /// 按几何复算 skipped：**只有被吸收的读侧坏块才产 skipped**。
    ///
    /// 这一条是本卡对"把写侧失败的字节也计进 skipped"的直接拒答：写侧红的时候
    /// 吸收量仍然只等于 `bad_blocks * ibs`，一个字节都不许多。
    pub fn skipped_expected(&self) -> u32 {
        if self.read_errno == ERR_NONE || !self.offers_absorb() {
            return 0u32;
        }
        self.bad_blocks.saturating_mul(self.ibs)
    }

    /// 申报的 skipped 与几何复算是否一致。
    pub fn skipped_matches_geom(&self) -> bool {
        self.skipped == self.skipped_expected()
    }

    /// 落盘字节是否算得出来（不超过读到的总字节）。
    pub fn written_plausible(&self) -> bool {
        let read_total = self
            .in_full
            .saturating_add(1)
            .saturating_mul(self.ibs);
        if self.written > read_total {
            return false;
        }
        if self.write_errno != ERR_NONE || self.sync_errno != ERR_NONE {
            return true;
        }
        if self.open_errno != ERR_NONE {
            return self.written == 0;
        }
        true
    }

    /// 主导落点：写侧优先于迟到、迟到优先于读侧——本卡的红线在写侧，
    /// 所以落点判定必须让写侧压过读侧，否则一条"吸收过又写死"的行会被读成绿。
    pub fn fall(&self) -> Fall {
        if self.write_errno != ERR_NONE {
            return Fall::WriteError;
        }
        if self.sync_errno != ERR_NONE {
            return Fall::LateSync;
        }
        if self.read_errno != ERR_NONE {
            return Fall::ReadError;
        }
        Fall::NoError
    }

    /// 形状指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_byte(h, self.conv);
        h = fold_byte(h, self.status as u8);
        h = fold_byte(h, self.flavor as u8);
        h = fold_str(h, self.src);
        h = fold_str(h, self.dst);
        h = fold_str(h, self.bad_operand);
        h = fold_word(h, self.ibs as u64);
        h = fold_word(h, self.obs as u64);
        h = fold_i32(h, self.open_errno);
        h = fold_i32(h, self.read_errno);
        h = fold_i32(h, self.write_errno);
        h = fold_i32(h, self.sync_errno);
        h = fold_word(h, self.bad_blocks as u64);
        h = fold_word(h, self.in_full as u64);
        h = fold_bool(h, self.in_partial);
        h = fold_word(h, self.skipped as u64);
        h = fold_word(h, self.written as u64);
        h = fold_bool(h, self.argv_legal);
        h = fold_bool(h, self.conv_names_legal);
        fold_word(h, self.exit_declared as u64)
    }
}

// ---------------------------------------------------------------------------
// 结论 / 申报 / 语料格
// ---------------------------------------------------------------------------

/// 实际结论（闸门纵线跑完之后落下的六格账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 落点闸。
    pub stage: Stage,
    /// errno（0 表示这次运行没沾 errno）。
    pub code: i32,
    /// skipped 字节（只有吸收侧才非零）。
    pub value: u32,
    /// 形态档。
    pub note: Note,
    /// 红线锚：`noerror` 在场 + 空间类 errno 落在写/同步闸 + 判成硬中止。
    pub default: bool,
    /// 结构自洽：读侧冒 28、补齐档缺 `sync` 位等都把它压成 `false`。
    pub consistent: bool,
}

impl Outcome {
    /// 全零初值。
    pub const fn blank() -> Outcome {
        Outcome {
            stage: Stage::Exit,
            code: ERR_NONE,
            value: 0u32,
            note: Note::PlainComplete,
            default: false,
            consistent: true,
        }
    }

    /// 组装一格结论。
    pub const fn make(
        stage: Stage,
        code: i32,
        value: u32,
        note: Note,
        default: bool,
        consistent: bool,
    ) -> Outcome {
        Outcome {
            stage,
            code,
            value,
            note,
            default,
            consistent,
        }
    }

    /// 是否绿。
    pub fn ok(&self) -> bool {
        self.code == ERR_NONE && self.consistent
    }

    /// 是否红。
    pub fn failed(&self) -> bool {
        self.code != ERR_NONE
    }

    /// 是否落了红线锚。
    pub fn anchored(&self) -> bool {
        self.default
    }

    /// 是否谎报（结构上不自洽）。
    pub fn refused(&self) -> bool {
        !self.consistent
    }

    /// 派生吸收档（不额外占一格，从 note 反推）。
    pub fn absorb(&self) -> Absorb {
        match self.note {
            Note::AbsorbedPadded => Absorb::Padded,
            Note::AbsorbedRaw => Absorb::AbsorbedRaw,
            Note::AbsorbedThenFatal => Absorb::AbsorbedRaw,
            Note::ReadSpaceClaim => Absorb::Rejected,
            _ => Absorb::NotOffered,
        }
    }

    /// 应然的退出码：用法 2、绿 0、其余一律 1。
    ///
    /// 关键一条：**被吸收过读错的运行仍给 1**。"跑到底"不等于"跑成功"，
    /// 把这一格写成 0 是本卡判死的第二档假绿。
    pub fn exit(&self) -> u32 {
        if self.note.usage() || self.note.out_of_scope() {
            return EXIT_USAGE;
        }
        if self.ok() {
            return EXIT_OK;
        }
        EXIT_IO
    }

    /// 该不该落红线锚（复算一遍，供 `default` 自检）：结构自洽、errno 是空间类、
    /// 落点在写侧两闸之一、note 属写侧中止家族。`noerror` 是否在场由 note 家族
    /// 承载——`WriteFatalPlain`（对照档）不在写侧家族里，所以不亮。
    pub fn should_anchor(&self) -> bool {
        if !self.consistent {
            return false;
        }
        if !errno_space_family(self.code) {
            return false;
        }
        if !self.stage.write_side() {
            return false;
        }
        self.note.write_side()
    }

    /// 结论指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.rank() as u64);
        h = fold_i32(h, self.code);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        h = fold_bool(h, self.default);
        fold_bool(h, self.consistent)
    }
}

/// 申报（外部读到的那一段账）。与 `Outcome` 同构，少了 `consistent`
/// ——申报方没有资格自证自洽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    /// 申报落在哪道闸。
    pub stage: Stage,
    /// 申报的 errno。
    pub code: i32,
    /// 申报的 skipped 字节。
    pub value: u32,
    /// 申报的形态档。
    pub note: Note,
    /// 申报的红线锚。
    pub default: bool,
}

impl Claim {
    /// 全零初值。
    pub const fn blank() -> Claim {
        Claim {
            stage: Stage::Exit,
            code: ERR_NONE,
            value: 0u32,
            note: Note::PlainComplete,
            default: false,
        }
    }

    /// 组装一份申报。
    pub const fn make(stage: Stage, code: i32, value: u32, note: Note, default: bool) -> Claim {
        Claim {
            stage,
            code,
            value,
            note,
            default,
        }
    }

    /// 申报读成绿还是红。
    pub fn success(&self) -> bool {
        self.code == ERR_NONE
    }

    /// 申报指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.rank() as u64);
        h = fold_i32(h, self.code);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        fold_bool(h, self.default)
    }
}

/// 一格语料：形状 + 申报 + 短名。
#[derive(Clone, Copy, Debug)]
pub struct Case {
    /// 输入形状。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
    /// 短名（标签必须是本卡的）。
    pub tag: &'static str,
}

impl Case {
    /// 空语料格。
    pub const fn blank() -> Case {
        Case {
            shape: Shape::blank(),
            claim: Claim::blank(),
            tag: "dd-noerror-blank",
        }
    }

    /// 实际结论。
    pub fn actual(&self) -> Outcome {
        define(&self.shape)
    }

    /// 短名。
    pub fn tag(&self) -> &'static str {
        self.tag
    }

    /// 判定（走全局 `judge`）。
    pub fn fault(&self) -> Fault {
        judge(self)
    }

    /// 是否绿。
    pub fn ok(&self) -> bool {
        matches!(self.fault(), Fault::Ok)
    }

    /// 语料格指纹。
    pub fn seal(&self) -> u64 {
        let mut h = self.shape.seal();
        h = fold_word(h, self.claim.seal());
        fold_str(h, self.tag)
    }
}

// ---------------------------------------------------------------------------
// define：把形状过八道闸，推出实际结论
// ---------------------------------------------------------------------------

/// 用法档的 note：区分"operand 整个不认"与"conv 列表里有未知名"。
fn usage_note(s: &Shape) -> Note {
    if !s.conv_names_legal {
        return Note::UnknownConv;
    }
    if !s.argv_legal {
        return Note::UnknownOperand;
    }
    Note::OutOfScope
}

/// 吸收后的 note：补齐与错位两档按 `sync` 位分家。
fn absorbed_note(s: &Shape) -> Note {
    if s.offers_pad() {
        Note::AbsorbedPadded
    } else {
        Note::AbsorbedRaw
    }
}

/// 吸收后落哪道闸：补齐档在 pad 闸，错位档在 absorb 闸。
fn absorbed_stage(s: &Shape) -> Stage {
    if s.offers_pad() {
        Stage::Pad
    } else {
        Stage::Absorb
    }
}

/// **本卡的纵线**：八道闸按秩走，前一闸不产出事实，后一闸就没有输入。
///
/// 走法（每一段都对应 `Stage` 的一闸，返回即落闸）：
///
/// 1. `argv` 闸：不合法的 operand、未知 conv 名、未知 status 档、`bs=0`
///    都在这里出局，一次 I/O 都没发生。这两类**永远不给锚**。
/// 2. `open` 闸：`if=`/`of=` 打不开，落盘字节必为 0。
/// 3. `read` 闸：先过结构闸——读侧冒出空间类 errno 即谎报（`consistent=false`）；
///    合法读错且 `noerror` 缺席则当场中止。
/// 4. `absorb` / 5. `pad` 闸：`noerror` 在场才吸收；`sync` 在场才补齐；
///    吸收量按几何复算（`bad_blocks * ibs`）。
/// 6. `write` 闸：**本卡心脏**——写侧 errno 一律中止，`noerror`/`sync` 无效，
///    红线锚在"`noerror` 在场 + 空间类 errno"时亮。
/// 7. `fsync` 闸：延迟分配的 28 在这里落地，语义与写侧同级。
/// 8. `exit` 闸：跑到底且没吸收过东西才给绿；吸收过的运行落在吸收闸上、
///    退出码仍是 1。
pub fn define(s: &Shape) -> Outcome {
    // 闸 1：argv。
    if s.usage_bad() {
        return Outcome {
            stage: Stage::Argv,
            code: ERR_NONE,
            value: 0u32,
            note: usage_note(s),
            default: false,
            consistent: true,
        };
    }
    // 闸 2：open。
    if s.open_errno != ERR_NONE {
        return Outcome {
            stage: Stage::Open,
            code: s.open_errno,
            value: 0u32,
            note: Note::OpenFailed,
            default: false,
            consistent: true,
        };
    }
    // 闸 3：read 的结构面——读侧不许出现空间类 errno。
    if !errno_read_family(s.read_errno) {
        return Outcome {
            stage: Stage::Read,
            code: s.read_errno,
            value: s.skipped,
            note: Note::ReadSpaceClaim,
            default: false,
            consistent: false,
        };
    }
    let absorbed = s.read_errno != ERR_NONE && s.offers_absorb();
    // 闸 3：read 的终止面——没有 noerror 的读错直接中止。
    if s.read_errno != ERR_NONE && !absorbed {
        return Outcome {
            stage: Stage::Read,
            code: s.read_errno,
            value: 0u32,
            note: Note::ReadFatalNoAbsorb,
            default: false,
            consistent: true,
        };
    }
    let skip = s.skipped_expected();
    // 闸 6：write——空间类也好、EIO 也好，一律中止，与 conv 位无关。
    if s.write_errno != ERR_NONE {
        let note = if absorbed {
            Note::AbsorbedThenFatal
        } else if s.offers_absorb() {
            Note::WriteFatalNoerror
        } else {
            Note::WriteFatalPlain
        };
        let anchor = s.offers_absorb() && errno_space_family(s.write_errno);
        return Outcome {
            stage: Stage::Write,
            code: s.write_errno,
            value: skip,
            note,
            default: anchor,
            consistent: true,
        };
    }
    // 闸 7：fsync/close 迟到——语义与写侧同级。
    if s.sync_errno != ERR_NONE {
        let anchor = s.offers_absorb() && errno_space_family(s.sync_errno);
        return Outcome {
            stage: Stage::Sync,
            code: s.sync_errno,
            value: skip,
            note: Note::LateSyncFatal,
            default: anchor,
            consistent: true,
        };
    }
    // 闸 4/5：吸收族收尾（写侧与同步侧都绿，运行跑到底但仍是红）。
    if absorbed {
        return Outcome {
            stage: absorbed_stage(s),
            code: s.read_errno,
            value: skip,
            note: absorbed_note(s),
            default: false,
            consistent: true,
        };
    }
    // 闸 8：全绿。
    let note = if s.offers_absorb() || s.offers_pad() {
        Note::NoerrorIdle
    } else {
        Note::PlainComplete
    };
    Outcome {
        stage: Stage::Exit,
        code: ERR_NONE,
        value: 0u32,
        note,
        default: false,
        consistent: true,
    }
}

/// 按落点与家族挑文案（逐字，含单引号式样）。
pub fn message(s: &Shape, o: &Outcome) -> String {
    match o.stage {
        Stage::Argv => match o.note {
            Note::UnknownOperand => msg_unrecognized_operand(s.bad_operand),
            Note::UnknownConv => msg_unrecognized_operand(s.bad_operand),
            _ => msg_invalid_number(s.bad_operand),
        },
        Stage::Open => msg_gnu_open_failed(s.dst, o.code),
        Stage::Read => match s.flavor {
            Flavor::Busybox => msg_bb_read(s.src, o.code),
            _ => msg_gnu_read(s.src, o.code),
        },
        Stage::Absorb | Stage::Pad => match s.flavor {
            Flavor::Busybox => msg_bb_read(s.src, o.code),
            _ => msg_gnu_read(s.src, o.code),
        },
        Stage::Write => match s.flavor {
            Flavor::Gnu8 => msg_gnu8_write(s.dst, o.code),
            Flavor::Busybox => msg_bb_write(s.dst, o.code),
            Flavor::Gnu9 => msg_gnu9_write(s.dst, o.code),
        },
        Stage::Sync => {
            if s.offers_late() {
                msg_gnu_fsync_failed(s.dst, o.code)
            } else {
                msg_gnu_close_output(s.dst, o.code)
            }
        }
        Stage::Exit => String::from(""),
    }
}

/// 判据行的完整释义（一行一条，字段顺序固定，供 ktest 逐字比对）。
pub fn detail(c: &Case) -> String {
    let a = c.actual();
    let mut s = String::from("dd-noerror conv=");
    s.push_str(dec_text(c.shape.conv as u32).as_str());
    s.push_str(" flavor=");
    s.push_str(c.shape.flavor.name());
    s.push_str(" status=");
    s.push_str(c.shape.status.token());
    s.push_str(" stage=");
    s.push_str(a.stage.name());
    s.push_str(" errno=");
    s.push_str(errno_pair_text(a.code).as_str());
    s.push_str(" skip=");
    s.push_str(dec_text(a.value).as_str());
    s.push_str(" note=");
    s.push_str(a.note.name());
    s.push_str(" anchor=");
    s.push_str(bool_text(a.default));
    s.push_str(" exit=");
    s.push_str(dec_text(a.exit()).as_str());
    s.push_str(" msg=\"");
    s.push_str(message(&c.shape, &a).as_str());
    s.push('"');
    s.push('\n');
    s
}

// ---------------------------------------------------------------------------
// 档位矩阵的读数
// ---------------------------------------------------------------------------

/// conv 位集 → 矩阵行标；只认四行精确组合，其余给 `None`（越界即 `DN_EMATRIX`）。
pub fn matrix_row_of(conv: u8) -> Option<usize> {
    let neutral = conv & CONVE_NEUTRAL;
    let strict = conv & !CONVE_NEUTRAL;
    let _ = neutral;
    match strict {
        0u8 => Some(0usize),
        CONV_NOERROR => Some(1usize),
        CONV_SYNC => Some(2usize),
        CONV_NOERROR | CONV_SYNC => Some(3usize),
        _ => None,
    }
}

/// 取一格；行或列越界给 `None`。
pub fn matrix_cell(row: usize, col: usize) -> Option<Cell> {
    let Some(r) = MATRIX.get(row) else {
        return None;
    };
    r.get(col).copied()
}

/// 按（位集, 落点）取一格；位集越界给 `None`。
pub fn matrix_cell_of(conv: u8, fall: Fall) -> Option<Cell> {
    let row = matrix_row_of(conv)?;
    matrix_cell(row, fall.index())
}

/// 矩阵里"noerror 漏到写侧"的格数——必须恒为 0，这就是红线。
pub fn matrix_write_leaks() -> usize {
    let mut n = 0usize;
    let mut r = 0usize;
    while r < CONV_ROW_COUNT {
        let mut c = 0usize;
        while c < FALL_COL_COUNT {
            if let Some(cell) = matrix_cell(r, c) {
                if let Some(col) = FALL_COLS.get(c) {
                    if cell.absorbed && col.space_side() {
                        n = n.saturating_add(1);
                    }
                    if cell.absorb.happened() && col.space_side() {
                        n = n.saturating_add(1);
                    }
                }
            }
            c += 1;
        }
        r += 1;
    }
    n
}

/// 矩阵里悬空格数：退出码不在 {0,1} 且落点/注解自相矛盾的格。
pub fn matrix_dangling() -> usize {
    let mut n = 0usize;
    let mut r = 0usize;
    while r < CONV_ROW_COUNT {
        let mut c = 0usize;
        while c < FALL_COL_COUNT {
            let Some(cell) = matrix_cell(r, c) else {
                n = n.saturating_add(1);
                c += 1;
                continue;
            };
            if cell.exit != EXIT_OK && cell.exit != EXIT_IO {
                n = n.saturating_add(1);
            }
            if (cell.absorb.happened() != cell.absorbed) || cell.misaligned != cell.absorb.misaligned() {
                n = n.saturating_add(1);
            }
            if cell.fatal && cell.absorb.happened() {
                n = n.saturating_add(1);
            }
            if cell.note.usage() {
                n = n.saturating_add(1);
            }
            c += 1;
        }
        r += 1;
    }
    n
}

/// 矩阵里"致命且落在写侧两闸"的格数（4 行 × 2 列 = 8）。
pub fn matrix_write_fatal_cells() -> usize {
    let mut n = 0usize;
    let mut r = 0usize;
    while r < CONV_ROW_COUNT {
        let mut c = 0usize;
        while c < FALL_COL_COUNT {
            if let Some(cell) = matrix_cell(r, c) {
                if cell.fatal && cell.stage.write_side() {
                    n = n.saturating_add(1);
                }
            }
            c += 1;
        }
        r += 1;
    }
    n
}

/// 矩阵里"吸收发生"的格数（只有 noerror 两行 × 读侧一列 = 2）。
pub fn matrix_absorbed_cells() -> usize {
    let mut n = 0usize;
    let mut r = 0usize;
    while r < CONV_ROW_COUNT {
        let mut c = 0usize;
        while c < FALL_COL_COUNT {
            if let Some(cell) = matrix_cell(r, c) {
                if cell.absorbed {
                    n = n.saturating_add(1);
                }
            }
            c += 1;
        }
        r += 1;
    }
    n
}

/// 形状基型：一次全绿的纯复制（`conv=` 空集，`bs=512`，2 块落盘）。
const BASE: Shape = Shape {
    conv: 0,
    status: Status::Plain,
    flavor: Flavor::Gnu9,
    src: "in.img",
    dst: "out.img",
    bad_operand: "",
    ibs: 512,
    obs: 512,
    open_errno: ERR_NONE,
    read_errno: ERR_NONE,
    write_errno: ERR_NONE,
    sync_errno: ERR_NONE,
    bad_blocks: 0,
    in_full: 2,
    in_partial: false,
    skipped: 0,
    written: 1024,
    argv_legal: true,
    conv_names_legal: true,
    exit_declared: EXIT_OK,
};

/// 申报基型：与 `BASE` 推出的实际结论逐格相同。
const BASE_CLAIM: Claim = Claim {
    stage: Stage::Exit,
    code: ERR_NONE,
    value: 0,
    note: Note::PlainComplete,
    default: false,
};

/// 十四格语料形状。前八格申报正确，后六格一槽只坏一维。
pub const SHAPES: [Shape; CORPUS] = [
    // 0 纯复制跑到底：一个 errno 都没沾，三行汇总齐全。
    Shape {
        src: "pack.img",
        dst: "mnt/pack.img",
        ibs: 4096,
        obs: 4096,
        in_full: 3,
        written: 12288,
        ..BASE
    },
    // 1 `conv=noerror` 空转：位集在场但没东西可吸收，照样码 0。
    Shape {
        conv: CONV_NOERROR,
        src: "/dev/sr0",
        dst: "/mnt/disk/img",
        ibs: 2048,
        obs: 2048,
        in_full: 8,
        written: 16384,
        ..BASE
    },
    // 2 吸收未补齐：一个坏块被跳过，此后全部字节前移 4096——错位档。
    Shape {
        conv: CONV_NOERROR,
        status: Status::Noxfer,
        src: "bad.img",
        dst: "out.img",
        ibs: 4096,
        obs: 4096,
        read_errno: ERR_EIO,
        bad_blocks: 1,
        in_full: 3,
        skipped: 4096,
        written: 12288,
        exit_declared: EXIT_IO,
        ..BASE
    },
    // 3 吸收且补齐：`noerror,sync` 把两个坏块补零，对齐保住。
    Shape {
        conv: CONV_NOERROR | CONV_SYNC,
        src: "/dev/sr1",
        dst: "/mnt/disk/fix.img",
        ibs: 512,
        obs: 512,
        read_errno: ERR_EIO,
        bad_blocks: 2,
        in_full: 6,
        skipped: 1024,
        written: 3072,
        exit_declared: EXIT_IO,
        ..BASE
    },
    // 4 **红线格**：`noerror` 在场，写侧 `ENOSPC` 当场中止，锚亮。
    Shape {
        conv: CONV_NOERROR,
        src: "/dev/sr0",
        dst: "/mnt/disk/img",
        ibs: 4096,
        obs: 4096,
        write_errno: ERR_ENOSPC,
        in_full: 1,
        written: 4096,
        exit_declared: EXIT_IO,
        ..BASE
    },
    // 5 混合现场：先吸收一个坏块、随后写侧 28——两个事实都必须在账上。
    Shape {
        conv: CONV_NOERROR | CONV_SYNC,
        status: Status::Progress,
        src: "cd.img",
        dst: "mnt/img",
        ibs: 512,
        obs: 512,
        read_errno: ERR_ENOMEDIUM,
        write_errno: ERR_ENOSPC,
        bad_blocks: 1,
        in_full: 4,
        skipped: 512,
        written: 2048,
        exit_declared: EXIT_IO,
        ..BASE
    },
    // 6 用法错：`conv=noerrorr` 拼错，operand 一路不认识，一次 I/O 都没发生。
    Shape {
        conv: CONV_NOERROR,
        conv_names_legal: false,
        bad_operand: "conv=noerrorr",
        src: "",
        dst: "",
        in_full: 0,
        written: 0,
        exit_declared: EXIT_USAGE,
        ..BASE
    },
    // 7 迟到档：写侧全绿、`conv=fsync` 收尾才红 28——noerror 同样救不了。
    Shape {
        conv: CONV_NOERROR | CONV_FSYNC,
        src: "work.img",
        dst: "/mnt/disk/work.bak",
        ibs: 4096,
        obs: 4096,
        sync_errno: ERR_ENOSPC,
        in_full: 2,
        written: 8192,
        exit_declared: EXIT_IO,
        ..BASE
    },
    // 8 Estate 死槽：现场是槽 0 那条纯复制，申报却给了一个 EIO。
    Shape {
        src: "pack.img",
        dst: "mnt/pack.img",
        ibs: 4096,
        obs: 4096,
        in_full: 3,
        written: 12288,
        ..BASE
    },
    // 9 Stage 死槽：红线现场，申报把落点写成"被 absorb 闸吸收了"。
    Shape {
        conv: CONV_NOERROR,
        src: "/dev/sr0",
        dst: "/mnt/disk/img",
        ibs: 4096,
        obs: 4096,
        write_errno: ERR_ENOSPC,
        in_full: 1,
        written: 4096,
        exit_declared: EXIT_IO,
        ..BASE
    },
    // 10 Code 死槽：红线现场，申报把 28 写成 122（EDQUOT 冒充 ENOSPC）。
    Shape {
        conv: CONV_NOERROR,
        src: "/dev/sr0",
        dst: "/mnt/disk/img",
        ibs: 4096,
        obs: 4096,
        write_errno: ERR_ENOSPC,
        in_full: 1,
        written: 4096,
        exit_declared: EXIT_IO,
        ..BASE
    },
    // 11 Value 死槽：混合现场，申报把吸收量抹成 0。
    Shape {
        conv: CONV_NOERROR | CONV_SYNC,
        status: Status::Progress,
        src: "cd.img",
        dst: "mnt/img",
        ibs: 512,
        obs: 512,
        read_errno: ERR_ENOMEDIUM,
        write_errno: ERR_ENOSPC,
        bad_blocks: 1,
        in_full: 4,
        skipped: 512,
        written: 2048,
        exit_declared: EXIT_IO,
        ..BASE
    },
    // 12 Note 死槽：补齐现场，申报成"未补齐的错位档"。
    Shape {
        conv: CONV_NOERROR | CONV_SYNC,
        src: "/dev/sr1",
        dst: "/mnt/disk/fix.img",
        ibs: 512,
        obs: 512,
        read_errno: ERR_EIO,
        bad_blocks: 2,
        in_full: 6,
        skipped: 1024,
        written: 3072,
        exit_declared: EXIT_IO,
        ..BASE
    },
    // 13 Default 死槽：红线现场，申报把红线锚摁灭。
    Shape {
        conv: CONV_NOERROR,
        src: "/dev/sr0",
        dst: "/mnt/disk/img",
        ibs: 4096,
        obs: 4096,
        write_errno: ERR_ENOSPC,
        in_full: 1,
        written: 4096,
        exit_declared: EXIT_IO,
        ..BASE
    },
];

/// 十四格申报。前八格与实际逐格相同，后六格各偏一维。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim {
        stage: Stage::Exit,
        code: ERR_NONE,
        value: 0,
        note: Note::PlainComplete,
        default: false,
    },
    Claim {
        stage: Stage::Exit,
        code: ERR_NONE,
        value: 0,
        note: Note::NoerrorIdle,
        default: false,
    },
    Claim {
        stage: Stage::Absorb,
        code: ERR_EIO,
        value: 4096,
        note: Note::AbsorbedRaw,
        default: false,
    },
    Claim {
        stage: Stage::Pad,
        code: ERR_EIO,
        value: 1024,
        note: Note::AbsorbedPadded,
        default: false,
    },
    Claim {
        stage: Stage::Write,
        code: ERR_ENOSPC,
        value: 0,
        note: Note::WriteFatalNoerror,
        default: true,
    },
    Claim {
        stage: Stage::Write,
        code: ERR_ENOSPC,
        value: 512,
        note: Note::AbsorbedThenFatal,
        default: true,
    },
    Claim {
        stage: Stage::Argv,
        code: ERR_NONE,
        value: 0,
        note: Note::UnknownConv,
        default: false,
    },
    Claim {
        stage: Stage::Sync,
        code: ERR_ENOSPC,
        value: 0,
        note: Note::LateSyncFatal,
        default: true,
    },
    // 8 Estate：把绿行报成红（成败互报是第一维，判序当场抓住）。
    Claim {
        stage: Stage::Exit,
        code: ERR_EIO,
        value: 0,
        note: Note::PlainComplete,
        default: false,
    },
    // 9 Stage：落点从 write 闸挪到 absorb 闸——"noerror 吞了 28"的谎形状。
    Claim {
        stage: Stage::Absorb,
        code: ERR_ENOSPC,
        value: 0,
        note: Note::WriteFatalNoerror,
        default: true,
    },
    // 10 Code：28 与 122 互换。
    Claim {
        stage: Stage::Write,
        code: ERR_EDQUOT,
        value: 0,
        note: Note::WriteFatalNoerror,
        default: true,
    },
    // 11 Value：吸收量抹成 0。
    Claim {
        stage: Stage::Write,
        code: ERR_ENOSPC,
        value: 0,
        note: Note::AbsorbedThenFatal,
        default: true,
    },
    // 12 Note：补齐档写成错位档。
    Claim {
        stage: Stage::Pad,
        code: ERR_EIO,
        value: 1024,
        note: Note::AbsorbedRaw,
        default: false,
    },
    // 13 Default：红线锚摁灭。
    Claim {
        stage: Stage::Write,
        code: ERR_ENOSPC,
        value: 0,
        note: Note::WriteFatalNoerror,
        default: false,
    },
];

/// 每格的期望缺陷维序号：0 表示应判绿，1..6 对应 `REJECT_FAULTS`。
pub const WANT_NDX: [u8; CORPUS] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6];

/// 每格的短标签（后六格一格只坏一维，标签点名那一维）。
pub const WANT_TAGS: [&str; CORPUS] = [
    "live-plain-complete",
    "live-noerror-idle",
    "live-absorbed-raw",
    "live-absorbed-padded",
    "live-redline-write",
    "live-absorbed-then-fatal",
    "live-unknown-conv",
    "live-late-sync-fatal",
    "estate-swap",
    "stage-swap",
    "code-swap",
    "value-swap",
    "note-swap",
    "default-swap",
];

/// 申报表第二式：落点闸（与 `CLAIMS` 逐格交叉校验）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Exit,
    Stage::Exit,
    Stage::Absorb,
    Stage::Pad,
    Stage::Write,
    Stage::Write,
    Stage::Argv,
    Stage::Sync,
    Stage::Exit,
    Stage::Absorb,
    Stage::Write,
    Stage::Write,
    Stage::Pad,
    Stage::Write,
];

/// 申报表第二式：errno。
pub const WANT_CODE: [i32; CORPUS] = [
    ERR_NONE,
    ERR_NONE,
    ERR_EIO,
    ERR_EIO,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_NONE,
    ERR_ENOSPC,
    ERR_EIO,
    ERR_ENOSPC,
    ERR_EDQUOT,
    ERR_ENOSPC,
    ERR_EIO,
    ERR_ENOSPC,
];

/// 申报表第二式：skipped 字节。
pub const WANT_VALUE: [u32; CORPUS] = [0, 0, 4096, 1024, 0, 512, 0, 0, 0, 0, 0, 0, 1024, 0];

/// 申报表第二式：形态档。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::PlainComplete,
    Note::NoerrorIdle,
    Note::AbsorbedRaw,
    Note::AbsorbedPadded,
    Note::WriteFatalNoerror,
    Note::AbsorbedThenFatal,
    Note::UnknownConv,
    Note::LateSyncFatal,
    Note::PlainComplete,
    Note::WriteFatalNoerror,
    Note::WriteFatalNoerror,
    Note::AbsorbedThenFatal,
    Note::AbsorbedRaw,
    Note::WriteFatalNoerror,
];

/// 申报表第二式：红线锚。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, true, true, false, true, false, true, true, true, false, false,
];

/// 实际表第一式：纵线跑完之后落在哪道闸（`define` 的手写复算，互相对撞）。
pub const ACT_STAGE: [Stage; CORPUS] = [
    Stage::Exit,
    Stage::Exit,
    Stage::Absorb,
    Stage::Pad,
    Stage::Write,
    Stage::Write,
    Stage::Argv,
    Stage::Sync,
    Stage::Exit,
    Stage::Write,
    Stage::Write,
    Stage::Write,
    Stage::Pad,
    Stage::Write,
];

/// 实际表第二式：errno。
pub const ACT_CODE: [i32; CORPUS] = [
    ERR_NONE,
    ERR_NONE,
    ERR_EIO,
    ERR_EIO,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_NONE,
    ERR_ENOSPC,
    ERR_NONE,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_EIO,
    ERR_ENOSPC,
];

/// 实际表第三式：skipped 字节。
pub const ACT_VALUE: [u32; CORPUS] = [0, 0, 4096, 1024, 0, 512, 0, 0, 0, 0, 0, 512, 1024, 0];

/// 实际表第四式：形态档。
pub const ACT_NOTE: [Note; CORPUS] = [
    Note::PlainComplete,
    Note::NoerrorIdle,
    Note::AbsorbedRaw,
    Note::AbsorbedPadded,
    Note::WriteFatalNoerror,
    Note::AbsorbedThenFatal,
    Note::UnknownConv,
    Note::LateSyncFatal,
    Note::PlainComplete,
    Note::WriteFatalNoerror,
    Note::WriteFatalNoerror,
    Note::AbsorbedThenFatal,
    Note::AbsorbedPadded,
    Note::WriteFatalNoerror,
];

/// 实际表第五式：红线锚。
pub const ACT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, true, true, false, true, false, true, true, true, false, true,
];

/// 实际表第六式：结构自洽（本卡语料里没有谎报格——谎报档由场景族承担）。
pub const ACT_CONSISTENT: [bool; CORPUS] = [
    true, true, true, true, true, true, true, true, true, true, true, true, true, true,
];

/// 活槽下标表。
const ACCEPT_SLOTS: [usize; ACCEPT] = [0usize, 1, 2, 3, 4, 5, 6, 7];

/// 死槽下标表。
const REJECT_SLOTS: [usize; REJECT] = [8usize, 9, 10, 11, 12, 13];

/// 组装十四格语料。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case::blank(); CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        if let (Some(sh), Some(cl), Some(tg)) = (SHAPES.get(i), CLAIMS.get(i), WANT_TAGS.get(i)) {
            if let Some(slot) = out.get_mut(i) {
                *slot = Case {
                    shape: *sh,
                    claim: *cl,
                    tag: *tg,
                };
            }
        }
        i += 1;
    }
    out
}

/// 取第 i 格；越界给 `None`（调用方落 `DN_ENOBS`）。
pub fn case_at(i: usize) -> Option<Case> {
    cases().get(i).copied()
}

/// 覆盖率检查：格数必须正好是 `CORPUS`。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        return Ok(());
    }
    Err(DN_ECOUNT)
}

/// 第 i 格期望的缺陷维；越界给 `None`。
pub fn expected(i: usize) -> Option<Fault> {
    let ndx = WANT_NDX.get(i)?;
    if *ndx == 0 {
        return Some(Fault::Ok);
    }
    let k = (*ndx as usize) - 1;
    REJECT_FAULTS.get(k).copied()
}

/// **判序**：状态 → 阶段 → errno → skipped → 注解 → 锚位。次序固定，不许换。
pub fn judge(c: &Case) -> Fault {
    let a = define(&c.shape);
    let q = &c.claim;
    // Estate：成败互报优先，先于阶段/错误码。
    if a.ok() != q.success() {
        return Fault::Estate;
    }
    if a.stage != q.stage {
        return Fault::Stage;
    }
    if a.code != q.code {
        return Fault::Code;
    }
    if a.value != q.value {
        return Fault::Value;
    }
    if a.note != q.note {
        return Fault::Note;
    }
    if a.default != q.default {
        return Fault::Default;
    }
    Fault::Ok
}

// ---------------------------------------------------------------------------
// 场景族：语料之外的形状，逐格钉住谎报档、家族差与 status 差
// ---------------------------------------------------------------------------

/// 场景数。
pub const SCENE_COUNT: usize = 9;

/// 九个场景：读侧 28 谎报 × 打开失败 × `bs=0` 不裁定 × busybox/gnu8 两式红线 ×
/// `status=none` 假绿 × 无 noerror 的读错 × 写侧 EDQUOT × 吸收后迟到。
pub fn scenes() -> [Shape; SCENE_COUNT] {
    [
        // 0 读侧冒 28：结构性谎报，`consistent=false`，落第四档。
        Shape {
            conv: CONV_NOERROR,
            src: "/dev/sr0",
            dst: "mnt/img",
            ibs: 4096,
            obs: 4096,
            read_errno: ERR_ENOSPC,
            bad_blocks: 1,
            in_full: 2,
            skipped: 4096,
            written: 8192,
            exit_declared: EXIT_IO,
            ..BASE
        },
        // 1 `open` 就红：目录都不存在，落盘字节必为 0。
        Shape {
            src: "in.img",
            dst: "mnt/no/dir.img",
            open_errno: ERR_ENOENT,
            in_full: 0,
            written: 0,
            exit_declared: EXIT_IO,
            ..BASE
        },
        // 2 `bs=0`：本卡不裁定，点名交给 F11489 的解析面。
        Shape {
            bad_operand: "bs=0",
            ibs: 0,
            obs: 0,
            in_full: 0,
            written: 0,
            exit_declared: EXIT_USAGE,
            ..BASE
        },
        // 3 busybox 式红线：文案换家族，红线不变。
        Shape {
            conv: CONV_NOERROR,
            flavor: Flavor::Busybox,
            status: Status::None,
            src: "/dev/sr0",
            dst: "/mnt/disk/bb.img",
            ibs: 1024,
            obs: 1024,
            write_errno: ERR_ENOSPC,
            in_full: 2,
            written: 2048,
            exit_declared: EXIT_IO,
            ..BASE
        },
        // 4 gnu8 式红线：8.x 的动词段是 `writing`。
        Shape {
            conv: CONV_NOERROR,
            flavor: Flavor::Gnu8,
            src: "/dev/sr0",
            dst: "/mnt/disk/old.img",
            ibs: 1024,
            obs: 1024,
            write_errno: ERR_ENOSPC,
            in_full: 2,
            written: 2048,
            exit_declared: EXIT_IO,
            ..BASE
        },
        // 5 `status=none` 假绿现场：汇总三行全没，红线照样亮。
        Shape {
            conv: CONV_NOERROR,
            status: Status::None,
            src: "/dev/sr0",
            dst: "/mnt/disk/q.img",
            ibs: 512,
            obs: 512,
            write_errno: ERR_ENOSPC,
            in_full: 4,
            written: 2048,
            exit_declared: EXIT_IO,
            ..BASE
        },
        // 6 无 `noerror` 的读错：吸收闸不产出，读侧直接中止。
        Shape {
            src: "bad.img",
            dst: "out.img",
            ibs: 4096,
            obs: 4096,
            read_errno: ERR_EIO,
            bad_blocks: 1,
            skipped: 0,
            in_full: 1,
            written: 0,
            exit_declared: EXIT_IO,
            ..BASE
        },
        // 7 写侧 EDQUOT：同闸同码档，成因移交 F11695，锚仍亮（空间类）。
        Shape {
            conv: CONV_NOERROR,
            src: "/dev/zero",
            dst: "mnt/quota.img",
            ibs: 512,
            obs: 512,
            write_errno: ERR_EDQUOT,
            in_full: 2,
            written: 1024,
            exit_declared: EXIT_IO,
            ..BASE
        },
        // 8 吸收过、再在 `conv=fsync` 上迟到：两笔账同时在场。
        Shape {
            conv: CONV_NOERROR | CONV_SYNC | CONV_FSYNC,
            src: "/dev/sr1",
            dst: "mnt/slow.img",
            ibs: 512,
            obs: 512,
            read_errno: ERR_EIO,
            sync_errno: ERR_ENOSPC,
            bad_blocks: 2,
            in_full: 6,
            skipped: 1024,
            written: 3072,
            exit_declared: EXIT_IO,
            ..BASE
        },
    ]
}

/// 场景标签。
pub const SCENE_TAGS: [&str; SCENE_COUNT] = [
    "scene-read-space-lie",
    "scene-open-failed",
    "scene-bs-zero-out",
    "scene-bb-redline",
    "scene-gnu8-redline",
    "scene-none-disguise",
    "scene-plain-read-fatal",
    "scene-write-edquot",
    "scene-absorb-then-late",
];

/// 场景落点闸。
pub const SCENE_WANT_STAGE: [Stage; SCENE_COUNT] = [
    Stage::Read,
    Stage::Open,
    Stage::Argv,
    Stage::Write,
    Stage::Write,
    Stage::Write,
    Stage::Read,
    Stage::Write,
    Stage::Sync,
];

/// 场景 errno。
pub const SCENE_WANT_CODE: [i32; SCENE_COUNT] = [
    ERR_ENOSPC,
    ERR_ENOENT,
    ERR_NONE,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_EIO,
    ERR_EDQUOT,
    ERR_ENOSPC,
];

/// 场景 skipped。
pub const SCENE_WANT_VALUE: [u32; SCENE_COUNT] = [4096, 0, 0, 0, 0, 0, 0, 0, 1024];

/// 场景形态档。
pub const SCENE_WANT_NOTE: [Note; SCENE_COUNT] = [
    Note::ReadSpaceClaim,
    Note::OpenFailed,
    Note::OutOfScope,
    Note::WriteFatalNoerror,
    Note::WriteFatalNoerror,
    Note::WriteFatalNoerror,
    Note::ReadFatalNoAbsorb,
    Note::WriteFatalNoerror,
    Note::LateSyncFatal,
];

/// 场景红线锚。
pub const SCENE_WANT_DEFAULT: [bool; SCENE_COUNT] = [
    false, false, false, true, true, true, false, true, true,
];

/// 场景结构自洽。
pub const SCENE_WANT_CONSISTENT: [bool; SCENE_COUNT] = [
    false, true, true, true, true, true, true, true, true,
];

/// 取第 i 个场景。
pub fn scene_at(i: usize) -> Option<Shape> {
    scenes().get(i).copied()
}

/// 取第 i 个场景的锚位。
pub fn scene_anchored(i: usize) -> Option<bool> {
    SCENE_WANT_DEFAULT.get(i).copied()
}

/// 第 i 个场景的实际结论（申报与场景同构：场景只用来钉实际侧）。
pub fn scene_case(i: usize) -> Option<Case> {
    let Some(sh) = scene_at(i) else {
        return None;
    };
    let Some(st) = SCENE_WANT_STAGE.get(i) else {
        return None;
    };
    let Some(cd) = SCENE_WANT_CODE.get(i) else {
        return None;
    };
    let Some(vl) = SCENE_WANT_VALUE.get(i) else {
        return None;
    };
    let Some(nt) = SCENE_WANT_NOTE.get(i) else {
        return None;
    };
    let Some(df) = SCENE_WANT_DEFAULT.get(i) else {
        return None;
    };
    let Some(tg) = SCENE_TAGS.get(i) else {
        return None;
    };
    Some(Case {
        shape: sh,
        claim: Claim {
            stage: *st,
            code: *cd,
            value: *vl,
            note: *nt,
            default: *df,
        },
        tag: tg,
    })
}

/// 九个场景逐格对判：申报即实际，故每格都必须判绿；任一格对不上即 false。
pub fn scenes_agree() -> bool {
    let all = scenes();
    let mut i = 0usize;
    while i < SCENE_COUNT {
        let Some(sh) = all.get(i) else {
            return false;
        };
        let a = define(sh);
        let Some(st) = SCENE_WANT_STAGE.get(i) else {
            return false;
        };
        let Some(cd) = SCENE_WANT_CODE.get(i) else {
            return false;
        };
        let Some(vl) = SCENE_WANT_VALUE.get(i) else {
            return false;
        };
        let Some(nt) = SCENE_WANT_NOTE.get(i) else {
            return false;
        };
        let Some(df) = SCENE_WANT_DEFAULT.get(i) else {
            return false;
        };
        let Some(cs) = SCENE_WANT_CONSISTENT.get(i) else {
            return false;
        };
        if a.stage != *st || a.code != *cd || a.value != *vl || a.note != *nt || a.default != *df
        {
            return false;
        }
        if a.consistent != *cs {
            return false;
        }
        let Some(c) = scene_case(i) else {
            return false;
        };
        if !matches!(judge(&c), Fault::Ok) {
            return false;
        }
        i += 1;
    }
    true
}

/// 过一遍形状但只在接受域内给结论；越界给 `None`。
pub fn define_or_none(s: &Shape) -> Option<Outcome> {
    if s.errnos_legal() && s.geom_in_range() {
        return Some(define(s));
    }
    None
}

/// 段位互证：五处交叉署名（F11489 联签、F11683、F11681、F11698）必须与
/// `SEG_BASE` 严格加减得上，errno 与文案也必须与署名串相同。
pub fn anchors_agree() -> bool {
    if SEG_BASE != 0x6d410 {
        return false;
    }
    if ANCHOR_F11489_SEG.saturating_add(ANCHOR_F11489_GAP) != SEG_BASE {
        return false;
    }
    if ANCHOR_F11683_SEG.saturating_add(ANCHOR_F11683_GAP) != SEG_BASE {
        return false;
    }
    if ANCHOR_F11681_SEG.saturating_add(ANCHOR_F11681_GAP) != SEG_BASE {
        return false;
    }
    if ANCHOR_F11698_SEG.saturating_sub(ANCHOR_F11698_GAP) != SEG_BASE {
        return false;
    }
    if ANCHOR_ENOSPC != ERR_ENOSPC {
        return false;
    }
    if ANCHOR_ERRNO_TEXT != strerror(ERR_ENOSPC) {
        return false;
    }
    if ANCHOR_EXIT_IO != EXIT_IO {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 矩阵与报表
// ---------------------------------------------------------------------------

/// 一档的行列：`total`（该档实际格数）与 `good`（该档判对格数）两维独立。
#[derive(Clone, Copy, Debug)]
pub struct Row {
    /// 该档总格数。
    pub total: usize,
    /// 该档判对格数。
    pub good: usize,
}

impl Row {
    /// 零行。
    pub const fn blank() -> Row {
        Row { total: 0, good: 0 }
    }

    /// 该档是否全对。
    pub fn perfect(&self) -> bool {
        self.total == self.good
    }

    /// 该档红格数。
    pub fn red(&self) -> usize {
        self.total.saturating_sub(self.good)
    }

    /// 渲染一行（矩阵里的一格）。
    pub fn render(&self, t: Tier) -> String {
        let mut s = String::from(t.name());
        s.push('=');
        s.push_str(dec_text(self.total as u32).as_str());
        s.push('/');
        s.push_str(dec_text(self.good as u32).as_str());
        s
    }
}

/// 四档矩阵。
#[derive(Clone, Copy, Debug)]
pub struct Matrix {
    /// 一档（全绿）。
    pub clean: Row,
    /// 二档（诚实红）。
    pub honest: Row,
    /// 三档（未落盘）。
    pub untouched: Row,
    /// 四档（谎报）。
    pub refused: Row,
}

impl Matrix {
    /// 零矩阵。
    pub const fn blank() -> Matrix {
        Matrix {
            clean: Row::blank(),
            honest: Row::blank(),
            untouched: Row::blank(),
            refused: Row::blank(),
        }
    }

    /// 过一遍语料，按实际结论归档，并按判序统计 good。
    pub fn build() -> Matrix {
        let mut m = Matrix::blank();
        let all = cases();
        let mut i = 0usize;
        while i < all.len() {
            let Some(c) = all.get(i) else {
                i += 1;
                continue;
            };
            let t = Tier::of(&c.actual());
            let hit = matches!(c.fault(), Fault::Ok);
            let row = match t {
                Tier::Clean => &mut m.clean,
                Tier::HonestFail => &mut m.honest,
                Tier::NotTouched => &mut m.untouched,
                Tier::Refused => &mut m.refused,
            };
            row.total = row.total.saturating_add(1);
            if hit {
                row.good = row.good.saturating_add(1);
            }
            i += 1;
        }
        m
    }

    /// 取某档行。
    pub fn row(&self, t: Tier) -> Option<Row> {
        match t {
            Tier::Clean => Some(self.clean),
            Tier::HonestFail => Some(self.honest),
            Tier::NotTouched => Some(self.untouched),
            Tier::Refused => Some(self.refused),
        }
    }

    /// 四档格数合计。
    pub fn slots(&self) -> usize {
        self.clean
            .total
            .saturating_add(self.honest.total)
            .saturating_add(self.untouched.total)
            .saturating_add(self.refused.total)
    }

    /// 四档判对合计。
    pub fn goods(&self) -> usize {
        self.clean
            .good
            .saturating_add(self.honest.good)
            .saturating_add(self.untouched.good)
            .saturating_add(self.refused.good)
    }

    /// 全对的行数（空行不算全对，防"没有该档所以满分"混进来）。
    pub fn perfect_rows(&self) -> usize {
        let all = Tier::all();
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            let Some(t) = all.get(i) else {
                i += 1;
                continue;
            };
            if let Some(r) = self.row(*t) {
                if r.total > 0 && r.perfect() {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 四档是否覆盖满（合计等于语料格数）。
    pub fn covered(&self) -> bool {
        self.slots() == CORPUS
    }

    /// 渲染四档（每档一行）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let all = Tier::all();
        let mut i = 0usize;
        while i < TIER_COUNT {
            let Some(t) = all.get(i) else {
                i += 1;
                continue;
            };
            if let Some(r) = self.row(*t) {
                s.push_str(r.render(*t).as_str());
                s.push('\n');
            }
            i += 1;
        }
        s
    }
}

impl Tier {
    /// 四档全集（矩阵与报表遍历用，顺序与 `index()` 一致）。
    pub fn all() -> [Tier; TIER_COUNT] {
        [
            Tier::Clean,
            Tier::HonestFail,
            Tier::NotTouched,
            Tier::Refused,
        ]
    }
}

/// 汇总报表。`matched` 是本卡的总判据：14 格里每一格的实际缺陷都必须
/// 本身没走偏（活格判绿），以及六具尸位各自坏在对的那一维（死格判对应的
/// 缺陷）。任何一维静默退化，`matched` 就会掉，`rate()` 立刻离开 1000。
#[derive(Clone, Copy, Debug)]
pub struct Report {
    /// 总槽数。
    pub total: u32,
    /// 申报正确槽数。
    pub accept: u32,
    /// 申报错误槽数。
    pub reject: u32,
    /// 一档（全绿）格数。
    pub clean: u32,
    /// 二档（诚实红）格数。
    pub honest: u32,
    /// 三档（未落盘）格数。
    pub untouched: u32,
    /// 四档（谎报）格数。
    pub refused: u32,
    /// 诚实红里落了红线锚的格数。
    pub anchored: u32,
    /// 对判命中数。
    pub matched: u32,
}

impl Report {
    /// 千分率（四舍五入）。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.matched * 1000 + self.total / 2) / self.total
    }

    /// 是否全绿：格数正好、且每一格的实际缺陷都等于期望缺陷。
    pub fn green(&self) -> bool {
        self.total == CORPUS as u32 && self.matched == self.total
    }

    /// 红档数（对判没命中的格数）。
    pub fn reds(&self) -> u32 {
        self.total.saturating_sub(self.matched)
    }

    /// 闸门：不全绿就报计数错。
    pub fn gate(&self) -> Result<(), u32> {
        if self.green() {
            Ok(())
        } else {
            Err(DN_ECOUNT)
        }
    }

    /// 四档格数合计（应当等于 `ACCEPT`：只有判对的格才计档）。
    pub fn tier_cells(&self) -> u32 {
        self.clean
            .saturating_add(self.honest)
            .saturating_add(self.untouched)
            .saturating_add(self.refused)
    }

    /// 码非零的档合计（诚实红 + 谎报）。
    pub fn red_cells(&self) -> u32 {
        self.honest.saturating_add(self.refused)
    }

    /// 渲染整份汇总（逐字稳定，与 `RENDER_OK` 对账）。
    pub fn render(&self) -> String {
        let mut s = String::from("dn-dd-noerror corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" clean=");
        s.push_str(u32_text(self.clean).as_str());
        s.push_str(" honest=");
        s.push_str(u32_text(self.honest).as_str());
        s.push_str(" untouched=");
        s.push_str(u32_text(self.untouched).as_str());
        s.push_str(" refused=");
        s.push_str(u32_text(self.refused).as_str());
        s.push_str(" anchor=");
        s.push_str(u32_text(self.anchored).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 全绿渲染的逐字钉版。
const RENDER_OK: &str = "dn-dd-noerror corpus=14 accept=8 reject=6 clean=2 honest=5 \
untouched=1 refused=0 anchor=3 match=14 rate=1000ppm\n";

/// 空报表（供"越界槽位"分支先返回一个形状再落错码时用）。
pub fn empty_report() -> Report {
    Report {
        total: 0,
        accept: 0,
        reject: 0,
        clean: 0,
        honest: 0,
        untouched: 0,
        refused: 0,
        anchored: 0,
        matched: 0,
    }
}

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 可注入一槽缺陷的全跑。
///
/// 注入的意义是把"期望表"本身也拉进被测面：强行宣称某槽犯了另一种缺陷，报表
/// 必须掉 `matched`、`rate()` 随之离开 1000。若注入后仍报全绿，说明 `matched`
/// 根本没看期望表——本卡的账本就是空的。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(DN_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut clean = 0u32;
    let mut honest = 0u32;
    let mut untouched = 0u32;
    let mut refused = 0u32;
    let mut anchored = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let Some(c) = cs.get(i) else {
            return Err(DN_ENOBS);
        };
        let a = c.actual();
        let mut f = judge(c);
        if let Some((j, forced)) = over {
            if j == i {
                f = forced;
            }
        }
        let Some(want) = expected(i) else {
            return Err(DN_ENOBS);
        };
        total = total.saturating_add(1);
        if f == want {
            matched = matched.saturating_add(1);
        }
        if matches!(f, Fault::Ok) {
            accept = accept.saturating_add(1);
            match Tier::of(&a) {
                Tier::Clean => clean = clean.saturating_add(1),
                Tier::HonestFail => honest = honest.saturating_add(1),
                Tier::NotTouched => untouched = untouched.saturating_add(1),
                Tier::Refused => refused = refused.saturating_add(1),
            }
            if a.default {
                anchored = anchored.saturating_add(1);
            }
        } else {
            reject = reject.saturating_add(1);
        }
        i += 1;
    }
    Ok(Report {
        total,
        accept,
        reject,
        clean,
        honest,
        untouched,
        refused,
        anchored,
        matched,
    })
}

/// 报表与矩阵是否互证（同一份语料，两条独立统计路径必须给出同样的档位计数）。
pub fn report_agrees_with_matrix() -> bool {
    let Ok(r) = run() else {
        return false;
    };
    let m = Matrix::build();
    r.clean as usize == m.clean.total
        && r.honest as usize == m.honest.total
        && r.untouched as usize == m.untouched.total
        && r.refused as usize == m.refused.total
        && m.slots() == CORPUS
        && r.total == CORPUS as u32
}

/// 红档率标定：注入一槽缺陷后的千分率必须正好等于 `RED_RATE`（929）。
pub fn red_rate_calibrated() -> bool {
    let Ok(r) = run_with(Some((0, Fault::Code))) else {
        return false;
    };
    r.rate() == RED_RATE && r.matched == CORPUS as u32 - 1
}

/// 六槽注入各自压低千分率：每一维都要能把全绿打断一次。
pub fn inject_six_lowers_rate() -> bool {
    let mut i = 0usize;
    while i < REJECT {
        let Some(f) = REJECT_FAULTS.get(i) else {
            return false;
        };
        let Ok(r) = run_with(Some((8, *f))) else {
            return false;
        };
        if r.matched == CORPUS as u32 {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 阶段闸门：观测纵线必须单调
// ---------------------------------------------------------------------------

/// 八闸单调闸门。
///
/// 这条纵线是 `argv → open → read → absorb → pad → write → fsync → exit`。
/// 它不能倒过来走：dd 只有在真读过、真写过之后才有资格谈 `fsync`，更不可能在
/// 还没解析完 operand 时就报出落盘 errno。回退的申报（已经判死在 `fsync` 又改口
/// 说停在 `read`）在本卡一律按阶段错处理——这类倒推在真实故障里最常见：文案抄得
/// 一字不差，闸位却和盘面上的证据对不上。
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    /// 上一段秩（`-1` 表示一段没走）。
    last: i32,
    /// 已走段数。
    steps: u32,
    /// 回退次数（显性记录，不许静默吞）。
    falls: u32,
    /// 是否已关闸。
    closed: bool,
}

impl Gate {
    /// 新闸门：未走、未关、零回退。
    pub const fn new() -> Gate {
        Gate {
            last: -1,
            steps: 0u32,
            falls: 0u32,
            closed: false,
        }
    }

    /// 走一段。关闸后再走给状态违例，回退给阶段错，同秩重复允许。
    pub fn step(&mut self, s: Stage) -> Result<(), u32> {
        if self.closed {
            self.falls = self.falls.saturating_add(1);
            return Err(DN_ESTATE);
        }
        let o = s.rank() as i32;
        if o < self.last {
            self.falls = self.falls.saturating_add(1);
            return Err(DN_ESTAGE);
        }
        self.last = o;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }

    /// 按本卡纵线走满八段并关闸。
    pub fn walk_all(&mut self) -> Result<u32, u32> {
        let all = Stage::all();
        let mut i = 0usize;
        while i < all.len() {
            let Some(s) = all.get(i) else {
                return Err(DN_ECOUNT);
            };
            self.step(*s)?;
            i += 1;
        }
        self.close()
    }

    /// 按结论走：只走到落点闸再关闸。
    ///
    /// dd 在写侧中止时根本不会执行 `fsync` 那道收尾（`conv=fsync` 没在场），
    /// 所以这里要按 `conv` 位集裁剪尾闸，否则会把「没走到的段」算成「走错的段」。
    pub fn walk_for(&mut self, s: &Shape, o: &Outcome) -> Result<u32, u32> {
        let all = Stage::all();
        let stop = o.stage.rank();
        let mut i = 0usize;
        let mut walked = 0u32;
        while i < all.len() {
            let Some(st) = all.get(i) else {
                return Err(DN_ECOUNT);
            };
            let r = st.rank();
            if r > stop {
                break;
            }
            let skip = match st {
                Stage::Absorb => !s.offers_absorb(),
                Stage::Pad => !s.offers_pad(),
                Stage::Sync => !s.offers_late(),
                _ => false,
            };
            if !skip {
                self.step(*st)?;
                walked = walked.saturating_add(1);
            }
            i += 1;
        }
        Ok(walked)
    }

    /// 关闸：段数不足八道视为账不平；双关视为状态违例。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            self.falls = self.falls.saturating_add(1);
            return Err(DN_ESTATE);
        }
        self.closed = true;
        if self.steps != STAGE_COUNT {
            return Err(DN_ECOUNT);
        }
        Ok(self.steps)
    }

    /// 注入一档缺陷后的关闸结果（六维各自钉死一条违约路径）。
    pub fn defect_close(&mut self, d: GateDefect) -> Result<u32, u32> {
        match d {
            GateDefect::None => self.walk_all(),
            GateDefect::Estate => self.close(),
            GateDefect::Stage => {
                self.last = Stage::Exit.rank() as i32;
                self.falls = self.falls.saturating_add(1);
                self.close()
            }
            GateDefect::Code => {
                self.step(Stage::Read)?;
                self.step(Stage::Argv).map(|_| self.steps)
            }
            GateDefect::Value => {
                self.steps = self.steps.saturating_sub(1);
                self.close()
            }
            GateDefect::Note => {
                let _ = self.walk_all();
                self.step(Stage::Exit).map(|_| self.steps)
            }
            GateDefect::Default => {
                self.closed = true;
                self.close()
            }
        }
    }

    /// 上一段秩。
    pub const fn last_rank(&self) -> i32 {
        self.last
    }

    /// 已走段数。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// 回退计数。
    pub const fn falls(&self) -> u32 {
        self.falls
    }

    /// 是否已关闸。
    pub const fn closed(&self) -> bool {
        self.closed
    }

    /// 闸门指纹（自身状态也进指纹，防「关闸后再改状态」逃过重放）。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, (self.last + 1) as u64);
        h = fold_word(h, self.steps as u64);
        h = fold_word(h, self.falls as u64);
        fold_bool(h, self.closed)
    }
}

/// 闸门注入的六维缺陷。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateDefect {
    /// 无缺陷：走满八段并关闸。
    None,
    /// 一段没走就关闸（成败互报那一档的闸门投影）。
    Estate,
    /// 尾段之后回退（阶段错位那一档）。
    Stage,
    /// 落点之后继续走（errno 错位那一档）。
    Code,
    /// 段数被抹掉一格（吸收量错那一档）。
    Value,
    /// 关闸后再走（形态档认岔那一档）。
    Note,
    /// 内部置成已关再关闸（锚位那一档的双重登记）。
    Default,
}

impl GateDefect {
    /// 全集（顺序与 `JUDGE_ORDER` 同构，便于逐维对账）。
    pub fn all() -> [GateDefect; REJECT] {
        [
            GateDefect::Estate,
            GateDefect::Stage,
            GateDefect::Code,
            GateDefect::Value,
            GateDefect::Note,
            GateDefect::Default,
        ]
    }

    /// 这一档违约必须返回的错误码。
    pub fn want(&self) -> Result<u32, u32> {
        match self {
            GateDefect::None => Ok(STAGE_COUNT),
            GateDefect::Estate => Err(DN_ECOUNT),
            GateDefect::Stage => Err(DN_ECOUNT),
            GateDefect::Code => Err(DN_ESTAGE),
            GateDefect::Value => Err(DN_ESTATE),
            GateDefect::Note => Err(DN_ESTATE),
            GateDefect::Default => Err(DN_ESTATE),
        }
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 账本格：一次观测的最小记账单位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    /// 槽位（登记时必须严格等于当前长度）。
    pub slot: u8,
    /// 操作码（`OP_*`，或闸门段的秩）。
    pub op: u8,
    /// 是否成功格。
    pub ok: bool,
    /// 错误码（成功格恒为 0）。
    pub code: u32,
    /// 观测值（死格恒为 0）。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0u8,
            op: 0u8,
            ok: false,
            code: 0u32,
            value: 0u32,
        }
    }

    /// 组装一格。
    pub const fn make(slot: u8, op: u8, ok: bool, code: u32, value: u32) -> Sample {
        Sample {
            slot,
            op,
            ok,
            code,
            value,
        }
    }

    /// 这一格的操作是否落在合法操作码集合里。
    pub fn op_legal(&self) -> bool {
        if matches!(self.op, OP_ARGV..=OP_REPLAY) {
            return true;
        }
        Stage::from_op(self.op).is_some()
    }

    /// 这一格落在哪道闸（`OP_*` 的归并档）。
    pub fn stage(&self) -> Stage {
        if let Some(s) = Stage::from_op(self.op) {
            return s;
        }
        match self.op {
            OP_CORPUS => Stage::Exit,
            OP_ABSORB | OP_PAD => Stage::Absorb,
            OP_RATE => Stage::Exit,
            OP_DEFECT => Stage::Exit,
            OP_REPLAY => Stage::Exit,
            _ => Stage::Argv,
        }
    }

    /// 一格指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.slot as u64);
        h = fold_word(h, self.op as u64);
        h = fold_bool(h, self.ok);
        h = fold_word(h, self.code as u64);
        fold_word(h, self.value as u64)
    }
}

/// 账本：14 格 = 7 统计格 + 1 红档率格 + 6 缺陷码格；冻结后拒登记。
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
            len: 0usize,
            root: 0u64,
            frozen: false,
            rejects: 0u32,
        }
    }

    /// 登记一格：槽位不等于当前长度、越界或已冻结都拒绝并计数。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DN_ESLOT);
        }
        if !s.op_legal() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DN_ESTATE);
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

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一格（越界给 `None`）。
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
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 死格数。
    pub fn err_count(&self) -> usize {
        self.len.saturating_sub(self.ok_count())
    }

    /// 逐格对账：槽位连续、操作码合法、成格值等于 `VALS`、
    /// 死格码等于 `REJECT_CODES` 且值归零。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(DN_ECOUNT);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let Some(s) = self.rows.get(i) else {
                return Err(DN_ECOUNT);
            };
            if (s.slot as usize) != i {
                return Err(DN_ESLOT);
            }
            if !s.op_legal() {
                return Err(DN_ESTATE);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DN_ECODE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DN_EVALUE);
                        }
                    }
                    None => return Err(DN_ECOUNT),
                }
                vi = vi.saturating_add(1);
            } else {
                if s.value != 0 {
                    if s.code == DN_ESTATE {
                        return Err(DN_ESTATE);
                    }
                    return Err(DN_EVALUE);
                }
                if !in_codes(s.code) {
                    return Err(DN_ECODE);
                }
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DN_ENOTE);
                        }
                    }
                    None => return Err(DN_ECOUNT),
                }
                ci = ci.saturating_add(1);
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(DN_ECOUNT);
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

    /// 冻结：先对账，账不平不给冻；已冻结再冻视为状态违例。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DN_ESTATE);
        }
        if self.len != SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DN_ECOUNT);
        }
        self.audit()?;
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：重建账本并比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(DN_ESEAL)
        }
    }

    /// 篡改留痕：冻结后禁止改动；改动成功后指纹必变、对账必红。
    pub fn tamper(&mut self, i: usize, value: u32) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DN_ESTATE);
        }
        if i >= self.len {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DN_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(i) {
            dst.value = value;
        }
        Ok(())
    }

    /// 冻结指纹。
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// 是否已冻结。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub const fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 按注入档造一本坏账（`None` 给正常账）。
    pub fn defect(d: LedgerDefect) -> Ledger {
        let mut l = build_ledger();
        match d {
            LedgerDefect::None => {}
            LedgerDefect::Count => {
                if let Some(c) = l.rows.get_mut(2) {
                    c.ok = false;
                }
            }
            LedgerDefect::OkValue => {
                if let Some(c) = l.rows.get_mut(4) {
                    c.value = c.value.saturating_add(1);
                }
            }
            LedgerDefect::BadCode => {
                if let Some(c) = l.rows.get_mut(10) {
                    c.code = 0u32;
                }
            }
            LedgerDefect::BadValue => {
                if let Some(c) = l.rows.get_mut(11) {
                    c.value = 1u32;
                }
            }
            LedgerDefect::DeadValue => {
                if let Some(c) = l.rows.get_mut(8) {
                    c.value = 1u32;
                    c.code = DN_ESTATE;
                }
            }
            LedgerDefect::SlotGap => {
                if let Some(c) = l.rows.get_mut(12) {
                    c.slot = 11u8;
                }
            }
        }
        l
    }
}

/// 账本注入的六类坏格（每类只打一条对账规则）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerDefect {
    /// 正常账。
    None,
    /// 统计格被改成死格，账数不平。
    Count,
    /// 成格观测值被抬高一格。
    OkValue,
    /// 死格错误码被清零。
    BadCode,
    /// 死格观测值被填 nonzero。
    BadValue,
    /// 死格既带值又带 Estate 码。
    DeadValue,
    /// 槽位跳号（两格抢同一槽）。
    SlotGap,
}

impl LedgerDefect {
    /// 全集（`None` 之外六档，与判序六维同长）。
    pub fn all() -> [LedgerDefect; 6] {
        [
            LedgerDefect::Count,
            LedgerDefect::OkValue,
            LedgerDefect::BadCode,
            LedgerDefect::BadValue,
            LedgerDefect::DeadValue,
            LedgerDefect::SlotGap,
        ]
    }

    /// 这一类坏账必须被 `audit` 抓到哪个码。
    pub fn want(&self) -> Result<(), u32> {
        match self {
            LedgerDefect::None => Ok(()),
            LedgerDefect::Count => Err(DN_ECOUNT),
            LedgerDefect::OkValue => Err(DN_EVALUE),
            LedgerDefect::BadCode => Err(DN_ECODE),
            LedgerDefect::BadValue => Err(DN_EVALUE),
            LedgerDefect::DeadValue => Err(DN_ESTATE),
            LedgerDefect::SlotGap => Err(DN_ESLOT),
        }
    }
}

/// 操作码 → 闸门（`OP_*` 归并档给 `None`）。
pub fn stage_of_op(op: u8) -> Option<Stage> {
    Stage::from_op(op)
}

/// 错误码是否属于本卡的 13 码链（账本对账用）。
fn in_codes(c: u32) -> bool {
    ALL_CODES.contains(&c)
}

/// 计数结果 → 样本。
fn count_cell(slot: u8, op: u8, res: Result<u32, u32>) -> Sample {
    match res {
        Ok(v) => Sample {
            slot,
            op,
            ok: true,
            code: 0u32,
            value: v,
        },
        Err(c) => Sample {
            slot,
            op,
            ok: false,
            code: c,
            value: 0u32,
        },
    }
}

/// 建账本：7 格统计 + 1 格红档率（929）+ 6 格缺陷码。
///
/// 统计格直接取 `run()` 的报表，红档格取注入一槽缺陷后的千分率，缺陷格取
/// 六具尸位各自的判序结果——三条来源毫无共用，任何一条静默退化都会让
/// `audit()` 或 `freeze()` 当场红。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let _ = l.register(count_cell(0u8, OP_CORPUS, r.map(|x| x.total)));
    let _ = l.register(count_cell(1u8, OP_CORPUS, r.map(|x| x.accept)));
    let _ = l.register(count_cell(2u8, OP_CORPUS, r.map(|x| x.reject)));
    let _ = l.register(count_cell(3u8, OP_READ, r.map(|x| x.clean)));
    let _ = l.register(count_cell(4u8, OP_WRITE, r.map(|x| x.honest)));
    let _ = l.register(count_cell(5u8, OP_ARGV, r.map(|x| x.untouched)));
    let _ = l.register(count_cell(6u8, OP_FSYNC, r.map(|x| x.anchored)));

    let red = run_with(Some((0usize, Fault::Code)));
    let _ = l.register(count_cell(7u8, OP_RATE, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        let slot = (ACCEPT + i) as u8;
        if let Some(c) = cs.get(ACCEPT + i) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot,
                op: OP_DEFECT,
                ok: false,
                code: f.code(),
                value: 0u32,
            });
        }
        i += 1;
    }
    l
}

/// 篡改可见性：冻结核掉的账改不动，未冻的账一改指纹就变、对账就红。
pub fn tamper_visible() -> bool {
    let mut l = build_ledger();
    let Ok(root) = l.freeze() else {
        return false;
    };
    if l.tamper(1usize, VALS[1].saturating_add(1)) != Err(DN_ESTATE) {
        return false;
    }
    if l.rejects() != 1 || l.replay() != Ok(root) {
        return false;
    }
    let mut m = build_ledger();
    let before = m.seal();
    if m.tamper(1usize, VALS[1].saturating_add(1)) != Ok(()) {
        return false;
    }
    if m.seal() == before {
        return false;
    }
    m.audit() == Err(DN_EVALUE)
}

/// 报表形状 → 账本读数（把报表的四条总判据各自映射到一个错误码维）。
pub fn report_of(r: &Report) -> Result<u32, u32> {
    if r.total != CORPUS as u32 || r.accept != ACCEPT as u32 || r.reject != REJECT as u32 {
        return Err(DN_ECOUNT);
    }
    if r.matched != CORPUS as u32 || r.rate() != RATE_FULL {
        return Err(DN_ESTATE);
    }
    if r.tier_cells() != ACCEPT as u32 {
        return Err(DN_EVALUE);
    }
    if r.anchored != VALS[6] {
        return Err(DN_ECODE);
    }
    Ok(r.matched)
}

// ---------------------------------------------------------------------------
// 判据基元（私有谓词；inject_four 的四路各押四条）
// ---------------------------------------------------------------------------

/// 表路之一：段基址、十三码链、邻卡锚、操作码闭集。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d410 {
        return false;
    }
    if DN_ESTAGE != SEG_BASE + 0x01
        || DN_ECODE != SEG_BASE + 0x02
        || DN_EVALUE != SEG_BASE + 0x03
        || DN_ENOTE != SEG_BASE + 0x04
        || DN_EDEFAULT != SEG_BASE + 0x05
        || DN_EGEOM != SEG_BASE + 0x06
        || DN_ESTATE != SEG_BASE + 0x07
        || DN_ECOUNT != SEG_BASE + 0x08
        || DN_ESLOT != SEG_BASE + 0x09
        || DN_ESEAL != SEG_BASE + 0x0a
        || DN_ENOBS != SEG_BASE + 0x0b
        || DN_EMATRIX != SEG_BASE + 0x0c
        || DN_EBAL != SEG_BASE + 0x0d
    {
        return false;
    }
    if ALL_CODES.len() != 13 || CODE_NAMES.len() != 13 {
        return false;
    }
    if ANCHOR_F11698_SEG != 0x6d420 || ANCHOR_F11698_GAP != 0x10 {
        return false;
    }
    if ANCHOR_ENOSPC != ERR_ENOSPC || ANCHOR_EXIT_IO != EXIT_IO {
        return false;
    }
    // 操作码闭集：11 个互不重复。
    if OP_COUNT != 11 || OP_BITS.len() != OP_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < OP_BITS.len() {
        let Some(x) = OP_BITS.get(i) else {
            return false;
        };
        let mut j = i + 1;
        while j < OP_BITS.len() {
            if let Some(y) = OP_BITS.get(j) {
                if x == y {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 表路之二：枚举全序（八闸/四列/四吸收/六状态/三族/十四注解/十四缺陷/四档位）。
fn enums_ok() -> bool {
    if STAGE_COUNT != 8 || STAGE_LAST_RANK != 8 {
        return false;
    }
    // 八闸：秩双射。
    let stages = Stage::all();
    if stages.len() != STAGE_COUNT as usize {
        return false;
    }
    let mut i = 0usize;
    while i < stages.len() {
        let Some(s) = stages.get(i) else {
            return false;
        };
        if s.rank() != (i + 1) as u32 {
            return false;
        }
        if Stage::from_rank(i as u32 + 1) != Some(*s) {
            return false;
        }
        if s.name().is_empty() {
            return false;
        }
        i += 1;
    }
    // 落点四列：index/from_index 互逆，读侧列唯一。
    let falls = Fall::all();
    if falls.len() != FALL_COL_COUNT {
        return false;
    }
    i = 0usize;
    while i < falls.len() {
        let Some(f) = falls.get(i) else {
            return false;
        };
        if f.index() != i || Fall::from_index(i) != Some(*f) {
            return false;
        }
        i += 1;
    }
    if Absorb::all().len() != 4
        || Status::all().len() != 6
        || Flavor::all().len() != 3
        || Note::all().len() != 14
        || NOTE_NAMES.len() != 14
        || Fault::all().len() != 14
        || Tier::all().len() != TIER_COUNT
    {
        return false;
    }
    // 注解名字表与枚举逐项对齐。
    let notes = Note::all();
    i = 0usize;
    while i < notes.len() {
        let Some(n) = notes.get(i) else {
            return false;
        };
        match NOTE_NAMES.get(i) {
            Some(t) => {
                if *t != n.name() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 表路之三：conv 位集闭集、效力归并位、矩阵行标。
fn conv_ok() -> bool {
    if CONV_BIT_COUNT != 8 || CONV_BITS.len() != CONV_BIT_COUNT {
        return false;
    }
    // 八位的析取必须铺满 CONV_KNOWN。
    let mut known = 0u8;
    let mut i = 0usize;
    while i < CONV_BITS.len() {
        if let Some(b) = CONV_BITS.get(i) {
            known |= *b;
        }
        i += 1;
    }
    if known != CONV_KNOWN {
        return false;
    }
    // 效力归并位与原始位一致；中性位与三类效力位互斥。
    if CONVE_ABSORB != CONV_NOERROR || CONVE_PAD != CONV_SYNC {
        return false;
    }
    if CONVE_LATE != (CONV_FSYNC | CONV_FDATASYNC) {
        return false;
    }
    if CONVE_NEUTRAL & (CONVE_ABSORB | CONVE_PAD | CONVE_LATE) != 0 {
        return false;
    }
    // 四行标：精确组合可入表，非精确组合拒。
    if matrix_row_of(0) != Some(0)
        || matrix_row_of(CONV_NOERROR) != Some(1)
        || matrix_row_of(CONV_SYNC) != Some(2)
        || matrix_row_of(CONV_NOERROR | CONV_SYNC) != Some(3)
        || matrix_row_of(CONV_FSYNC).is_some()
    {
        return false;
    }
    if CONV_ROWS.len() != CONV_ROW_COUNT || CONV_ROW_NAMES.len() != CONV_ROW_COUNT {
        return false;
    }
    i = 0usize;
    while i < CONV_ROWS.len() {
        if matrix_row_of(CONV_ROWS[i]) != Some(i) {
            return false;
        }
        i += 1;
    }
    true
}

/// 判路前哨：形状基型与合法域（errno 家族、块几何、skip 复算）。
fn shape_ok() -> bool {
    let b = Shape::blank();
    if !b.errnos_legal() || !b.geom_in_range() || !b.skipped_matches_geom() {
        return false;
    }
    if b.usage_bad() || b.fall() != Fall::NoError || b.unknown_conv_bits() != 0 {
        return false;
    }
    if !b.written_plausible() || b.exit_declared != EXIT_OK {
        return false;
    }
    // 用法闸：argv 不合法即坏。
    let mut bad = b;
    bad.argv_legal = false;
    if !bad.usage_bad() {
        return false;
    }
    // 落点判定：写侧压过迟到、迟到压过读侧。
    let mut wo = b;
    wo.write_errno = ERR_ENOSPC;
    if !wo.errnos_legal() || wo.fall() != Fall::WriteError {
        return false;
    }
    let mut so = b;
    so.sync_errno = ERR_ENOSPC;
    if !so.errnos_legal() || so.fall() != Fall::LateSync {
        return false;
    }
    let mut ro = b;
    ro.read_errno = ERR_EIO;
    if !ro.errnos_legal() || ro.fall() != Fall::ReadError {
        return false;
    }
    // 语料形状逐格合法。
    if SHAPES.len() != CORPUS {
        return false;
    }
    let mut i = 0usize;
    while i < SHAPES.len() {
        let Some(s) = SHAPES.get(i) else {
            return false;
        };
        if !s.errnos_legal() || !s.geom_in_range() {
            return false;
        }
        i += 1;
    }
    true
}

/// 判路之一：define 可复算、语料逐格对表、判序命中期望缺陷维。
fn define_ok() -> bool {
    let cs = cases();
    if cs.len() != CORPUS {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let Some(c) = cs.get(i) else {
            return false;
        };
        // 可复算：同形两判同指纹。
        let a1 = define(&c.shape);
        let a2 = define(&c.shape);
        if a1.seal() != a2.seal() {
            return false;
        }
        // 对表：阶段/errno/值/注解逐维与期望表一致。
        let Some(ws) = WANT_STAGE.get(i) else {
            return false;
        };
        let Some(wc) = WANT_CODE.get(i) else {
            return false;
        };
        let Some(wv) = WANT_VALUE.get(i) else {
            return false;
        };
        let Some(wn) = WANT_NOTE.get(i) else {
            return false;
        };
        if a1.stage != *ws || a1.code != *wc || a1.value != *wv || a1.note != *wn {
            return false;
        }
        // 判序：期望缺陷维逐一命中。
        let Some(wf) = expected(i) else {
            return false;
        };
        if judge(c) != wf {
            return false;
        }
        i += 1;
    }
    // 场景族与锚位两路（表路判路的交叉验证）。
    if !scenes_agree() || !anchors_agree() {
        return false;
    }
    true
}

/// 判路之二：吸收语义——noerror 只管读侧，补齐靠 sync 位，写侧一律致命。
fn absorb_ok() -> bool {
    let b = Shape::blank();
    // 无 noerror 的读错：当场中止，无吸收账。
    let mut ro = b;
    ro.read_errno = ERR_EIO;
    let a = define(&ro);
    if a.stage != Stage::Read || a.note != Note::ReadFatalNoAbsorb || a.ok() {
        return false;
    }
    // noerror 在场、补齐位缺席：吸收发生但错位。
    let mut raw = ro;
    raw.conv = CONV_NOERROR;
    let a2 = define(&raw);
    if a2.absorb() != Absorb::AbsorbedRaw || !a2.absorb().happened() || !a2.absorb().misaligned() {
        return false;
    }
    // 补齐位也在场：补齐档，对齐保住。
    let mut pad = raw;
    pad.conv |= CONV_SYNC;
    let a3 = define(&pad);
    if a3.absorb() != Absorb::Padded || !a3.absorb().happened() || a3.absorb().misaligned() {
        return false;
    }
    // 结构闸：读侧不许冒空间类 errno（谎报档）。
    let mut lie = raw;
    lie.read_errno = ERR_ENOSPC;
    let a4 = define(&lie);
    if a4.note != Note::ReadSpaceClaim || a4.consistent {
        return false;
    }
    // 矩阵口径：吸收只许发生在 noerror 两行 × 读侧一列（2 格）。
    if matrix_absorbed_cells() != 2 {
        return false;
    }
    true
}

/// 判路之三：判序固定——照抄必绿，逐维改坏逐维点名。
fn judge_ok() -> bool {
    let cs = cases();
    if cs.len() != CORPUS {
        return false;
    }
    let mut i = 0usize;
    while i < cs.len() {
        let Some(c) = cs.get(i) else {
            return false;
        };
        // 照抄事实的申报必绿。
        let a = c.actual();
        let k = Claim::make(a.stage, a.code, a.value, a.note, a.default);
        let honest = Case {
            shape: c.shape,
            claim: k,
            tag: c.tag,
        };
        if judge(&honest) != Fault::Ok {
            return false;
        }
        i += 1;
    }
    // 逐维改坏：判序必须按固定次序点名（先成败、再阶段、errno、值、注解、锚位）。
    let Some(c0) = cs.get(0) else {
        return false;
    };
    let a0 = c0.actual();
    if !a0.ok() {
        return false;
    }
    // 阶段维。
    let k = Claim::make(
        if a0.stage == Stage::Argv {
            Stage::Open
        } else {
            Stage::Argv
        },
        a0.code,
        a0.value,
        a0.note,
        a0.default,
    );
    if judge(&Case {
        shape: c0.shape,
        claim: k,
        tag: c0.tag,
    }) != Fault::Stage
    {
        return false;
    }
    // errno 维。
    let k = Claim::make(a0.stage, a0.code.wrapping_add(1), a0.value, a0.note, a0.default);
    if judge(&Case {
        shape: c0.shape,
        claim: k,
        tag: c0.tag,
    }) != Fault::Code
    {
        return false;
    }
    // 值维。
    let k = Claim::make(a0.stage, a0.code, a0.value.wrapping_add(1), a0.note, a0.default);
    if judge(&Case {
        shape: c0.shape,
        claim: k,
        tag: c0.tag,
    }) != Fault::Value
    {
        return false;
    }
    // 注解维。
    let k = Claim::make(
        a0.stage,
        a0.code,
        a0.value,
        if a0.note == Note::PlainComplete {
            Note::NoerrorIdle
        } else {
            Note::PlainComplete
        },
        a0.default,
    );
    if judge(&Case {
        shape: c0.shape,
        claim: k,
        tag: c0.tag,
    }) != Fault::Note
    {
        return false;
    }
    // 锚位维。
    let k = Claim::make(a0.stage, a0.code, a0.value, a0.note, !a0.default);
    if judge(&Case {
        shape: c0.shape,
        claim: k,
        tag: c0.tag,
    }) != Fault::Default
    {
        return false;
    }
    true
}

/// 判路之四：档位矩阵——16 格无悬空、noerror 不吞写侧空间错。
fn matrix_ok() -> bool {
    let mut r = 0usize;
    while r < CONV_ROW_COUNT {
        let mut c = 0usize;
        while c < FALL_COL_COUNT {
            if matrix_cell(r, c).is_none() {
                return false;
            }
            c += 1;
        }
        r += 1;
    }
    // 红线：noerror 漏到写侧的格数恒为 0。
    if matrix_write_leaks() != 0 || matrix_dangling() != 0 {
        return false;
    }
    // 致命格 8（4 行 × 写侧两列）、吸收格 2（noerror 两行 × 读侧一列）。
    if matrix_write_fatal_cells() != 8 || matrix_absorbed_cells() != 2 {
        return false;
    }
    // 列标与列序一致。
    let mut i = 0usize;
    while i < FALL_COLS.len() {
        let Some(f) = FALL_COLS.get(i) else {
            return false;
        };
        if f.index() != i || Fall::from_index(i) != Some(*f) {
            return false;
        }
        i += 1;
    }
    if !report_agrees_with_matrix() {
        return false;
    }
    true
}

/// 渲染路之一：语料与三份陈述（形状/申报/期望表）两两对齐。
fn corpus_ok() -> bool {
    if CLAIMS.len() != CORPUS
        || WANT_TAGS.len() != CORPUS
        || WANT_NDX.len() != CORPUS
        || WANT_STAGE.len() != CORPUS
        || WANT_CODE.len() != CORPUS
        || WANT_VALUE.len() != CORPUS
        || WANT_NOTE.len() != CORPUS
        || SHAPES.len() != CORPUS
    {
        return false;
    }
    // 短名互不重复。
    let mut i = 0usize;
    while i < CORPUS {
        let Some(t) = WANT_TAGS.get(i) else {
            return false;
        };
        if t.is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < CORPUS {
            if let Some(u) = WANT_TAGS.get(j) {
                if t == u {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    // 期望缺陷维闭集：0（绿）或 1..=REJECT。
    i = 0usize;
    while i < CORPUS {
        let n = WANT_NDX[i] as usize;
        if n != 0 && n > REJECT {
            return false;
        }
        i += 1;
    }
    if REJECT_FAULTS.len() != REJECT {
        return false;
    }
    true
}

/// 渲染路之二：槽位与红档标定。
fn slots_ok() -> bool {
    if VALS.len() != ACCEPT || STAT_CELLS != 7 {
        return false;
    }
    // 红档率末格钉死 929。
    match VALS.get(ACCEPT - 1) {
        Some(v) => {
            if *v != RED_RATE {
                return false;
            }
        }
        None => return false,
    }
    // 场景族：逐格可取、锚位有判。
    if SCENE_COUNT != 9 {
        return false;
    }
    let mut i = 0usize;
    while i < SCENE_COUNT {
        if scene_at(i).is_none() || scene_anchored(i).is_none() || scene_case(i).is_none() {
            return false;
        }
        i += 1;
    }
    if !scenes_agree() {
        return false;
    }
    true
}

/// 渲染路之三：文案逐字——三族各自的写侧动词与 strerror 尾。
fn message_ok() -> bool {
    let mut s = Shape::blank();
    s.write_errno = ERR_ENOSPC;
    // GNU 8.x：`dd: writing 'X': <strerror>`。
    s.flavor = Flavor::Gnu8;
    let o8 = define(&s);
    let m8 = message(&s, &o8);
    if !m8.contains("dd: writing '") || !m8.contains(ANCHOR_ERRNO_TEXT) {
        return false;
    }
    // GNU 9.x：`dd: error writing 'X': <strerror>`。
    s.flavor = Flavor::Gnu9;
    let m9 = message(&s, &define(&s));
    if !m9.contains("dd: error writing '") || m9 == m8 {
        return false;
    }
    // busybox：`dd: write error to 'X': <strerror>`，且不打印字节行。
    s.flavor = Flavor::Busybox;
    let mb = message(&s, &define(&s));
    if !mb.contains("dd: write error to '") || mb == m9 {
        return false;
    }
    // 全绿无话。
    let green = define(&Shape::blank());
    if green.note != Note::NoerrorIdle && green.note != Note::PlainComplete {
        return false;
    }
    if !message(&Shape::blank(), &green).is_empty() {
        return false;
    }
    // 用法错走 operand 原文。
    let mut bad = Shape::blank();
    bad.argv_legal = false;
    bad.bad_operand = "zz";
    let ob = define(&bad);
    if !message(&bad, &ob).contains("zz") {
        return false;
    }
    true
}

/// 渲染路之四：释义行字段齐全，报表形状与红线读数对得上。
fn detail_ok() -> bool {
    let cs = cases();
    let Some(c) = cs.get(0) else {
        return false;
    };
    let d = detail(c);
    if !d.contains(c.tag())
        || !d.contains("stage=")
        || !d.contains("errno=")
        || !d.contains("exit=")
        || !d.contains("msg=\"")
    {
        return false;
    }
    // 报表：总数对、渲染非空、四条总判据与矩阵对账。
    let rep = match run() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if rep.total != CORPUS as u32 || rep.render().is_empty() {
        return false;
    }
    if !report_agrees_with_matrix() || !red_rate_calibrated() {
        return false;
    }
    true
}

/// 账本路之一：闸门机——走满八段才许关，回退/重关/欠段各落各码。
fn gate_ok() -> bool {
    // 无缺陷：走满八段关闸得段数。
    let mut g = Gate::new();
    if g.defect_close(GateDefect::None) != Ok(STAGE_COUNT) {
        return false;
    }
    // 回退：高秩之后走低秩给阶段错。
    let mut g2 = Gate::new();
    if g2.step(Stage::Write).is_err() || g2.step(Stage::Open).is_ok() {
        return false;
    }
    // 双关：关闸后再关给状态违例。
    let mut g3 = Gate::new();
    if g3.walk_all().is_err() || g3.close().is_ok() {
        return false;
    }
    // 六具尸位：各自违约路径必须显性落红。
    let defects = GateDefect::all();
    let mut i = 0usize;
    while i < defects.len() {
        let Some(d) = defects.get(i) else {
            return false;
        };
        let mut x = Gate::new();
        if x.defect_close(*d).is_ok() {
            return false;
        }
        i += 1;
    }
    true
}

/// 账本路之二：登记/对账/冻结重放/篡改显性。
fn ledger_ok() -> bool {
    let mut l = build_ledger();
    if l.audit() != Ok(()) || l.len() != SAMPLES {
        return false;
    }
    // 冻结重放。
    let root = match l.freeze() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if !l.is_frozen() || l.replay() != Ok(root) {
        return false;
    }
    // 冻结后篡改必拒。
    if l.tamper(1usize, VALS[1].saturating_add(1)) != Err(DN_ESTATE) {
        return false;
    }
    // 未冻篡改必显性。
    let mut m = build_ledger();
    let before = m.seal();
    if m.tamper(1usize, VALS[1].saturating_add(1)).is_err() {
        return false;
    }
    if m.seal() == before {
        return false;
    }
    if !tamper_visible() {
        return false;
    }
    // 报表读数：全绿千分比与总账一致。
    match run() {
        Ok(rep) => {
            if report_of(&rep) != Ok(rep.matched) {
                return false;
            }
            if rep.rate() != RATE_FULL || rep.matched != CORPUS as u32 {
                return false;
            }
        }
        Err(_) => return false,
    }
    true
}

// ---------------------------------------------------------------------------
// 四路注入（各自独立自检）
// ---------------------------------------------------------------------------

/// 四路注入：表路 / 判路 / 渲染路 / 账本路。
///
/// 任何一路静默退化都会在这里变成 `false`：
/// 1. 表路——段基址与错误码链、枚举全序、conv 位集与档位矩阵合法域；
/// 2. 判路——八闸纵线、吸收语义、六维判序、矩阵覆盖；
/// 3. 渲染路——语料与场景族、槽位与红档标定、文案逐字、释义逐字；
/// 4. 账本路——阶段闸门、逐格对账、冻结重放、四注入自身。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && enums_ok() && conv_ok() && shape_ok();
    out[1] = define_ok() && absorb_ok() && judge_ok() && matrix_ok();
    out[2] = corpus_ok() && slots_ok() && message_ok() && detail_ok();
    out[3] = gate_ok() && ledger_ok() && {
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

// CONTINUATION-7
