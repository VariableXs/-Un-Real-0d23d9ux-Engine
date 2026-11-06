//! UNX-F11685 sort 临时文件 ENOSPC 降级判据
//!
//! `sort` 与 cp/dd/tee 的根本不同在于它**先要一块自己的地盘**：它把输入读进
//! 内存缓冲（`-S` 给的那份，缺省按可用内存折算），缓冲装满就把这一段排好序
//! **甩成一个临时件**落到 `$TMPDIR`（或 `-T` 指定的目录，再退到 `/tmp`），
//! 最后把所有临时件连同内存里那份尾段一起做 k 路归并写到输出。于是设备满在
//! `sort` 身上不是一个点，而是**四个不同的落点**，每个落点的文案、后果、可否
//! 降级都不一样：
//!
//! ```text
//! $ sort -S 100 big.txt > /mnt/full/out.txt
//! sort: write failed: '/tmp/sortZ3fQaL': No space left on device
//! $ echo $?
//! 2
//! ```
//!
//! 四个落点：
//!
//! * **临时件创建失败**（目录项/inode/日志面尽，不是数据块尽）：
//!   `sort: cannot create temporary file in '/mnt/loop': No space left on
//!   device` —— 句式里给的是**目录**，不是文件名，因为一个名字都没开出来。
//! * **临时件写出失败**（数据块尽）：`sort: write failed: '/tmp/sortZ3fQaL':
//!   No space left on device` —— 句式里给的是**临时件全名**，此时目录项是有的、
//!   件已经在那儿了，只是写不进去。两句混认，运维就分不清该清理目录还是该扩容。
//! * **归并回读失败**：上一段临时件被别的回收逻辑删掉了，`sort: cannot read:
//!   '/tmp/sortZ3fQaL': No such file or directory` —— 这是**红**，但和空间无关，
//!   归本卡拒收；把它记成 ENOSPC 会把一次 inode 回收事故诊断成容量事故。
//! * **最终写出失败**（stdout 或 `-o` 目的地满）：`sort: write failed:
//!   'standard output': No space left on device` / `sort: write failed:
//!   'sorted.txt': No space left on device`。这一档独有一个事实：**已经吐出去的
//!   行收不回来**。下游 `sort | head`、`sort > f` 的消费者拿到的是一个被截断的
//!   有序流，而前三个落点的 `emitted` 恒为 0。
//!
//! 本卡的诚实主干是这一句：**没碰过临时盘的 sort 不许被报成 ENOSPC 失败**。
//! `-m`（流式归并已排序输入）、`-c`/`-C`（只校验顺序、不排序）三种形态**一个
//! 临时件都不建**；小输入全部落在内存缓冲里时也不建。这些形态撞上满 `$TMPDIR`
//! 照样成功——`TMPDIR=/mnt/full sort tiny.txt` 退 0，这是**正判据**，不是漏报。
//! 反过来，凡是把这种现场写成 `sort: write failed: '<tmp>': ...` 的行，本卡在
//! 缓冲闸就把它拦下来判成谎报（`MemoryLie`），连错误码都不给真实档，因为这条
//! 记录本身没有可采信的现场。
//!
//! 另一根红线是**输入完整性**：`sort` 从不改输入文件（`-o` 可以给成输入名，
//! 那是**目的地**，不是对输入的破坏）。所以"临时盘满了"与"输入被截了"在因果上
//! 不可能同时成立；出现后者说明被测实现做错了事，本卡把它结算在清理闸并**永远
//! 拒绝落锚**——文案、退出码、行数全对也不给过。
//!
//! 工具家族也必须分表：GNU coreutils 的 sort 遇到麻烦退 **2**（含 `-c` 的乱序），
//! 只有 `-C`（静默校验）退 **1**；busybox 的 applet 一律退 **1**，且它的文案
//! 不带被引号包住的路径（`sort: can't create temp file: No space left on device`）。
//! 把 busybox 的 1 记成 GNU 的档位，或者反过来要求 busybox 打带引号的临时件名，
//! 都是本卡 Note 维的死槽。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 九道闸，秩从 **1** 起算；`Gate` 只接受"前进到下一秩"，倒退落 `ST_ESTAGE`、
//! 跳档落 `ST_EBAL`、封印后再动落 `ST_ESTATE`。前一闸不产出事实，后一闸就没有
//! 输入。
//!
//! 1. `args`（秩 1）——命令行本身。未知选项、`-S`/`-T` 取值不成形状在这里退 2，
//!    一个字节都没碰过，临时盘余量、已吐行数这些概念都不存在。
//! 2. `tmpdir`（秩 2）——**目录能不能开出一个名字**。`TMPDIR` 未设退 `/tmp`、
//!    设成空串同样退 `/tmp`（空值不算"用户指了个坏目录"）、`-T` 覆盖环境量。
//!    这一闸的红是目录项/inode/日志面层次的失败，它必须排在写出闸之前：件名都
//!    没有，后面所有"第几个临时件写挂"的算术都失去载体。反过来它必须排在缓冲闸
//!    之后——只有确定这次**真要落盘**，才有权说这个目录不可用。
//! 3. `input`（秩 3）——输入的打开与读。`sort: cannot read: 'gone.txt': No such
//!    file or directory` 在这里出局；`-c`/`-C` 的乱序判定也在这里发生（它们
//!    边读边比，压根不进缓冲）。它在临时盘闸之后：读不到输入和盘满无关，但
//!    "读都读不到"绝不能被下游误记成"写不进"。
//! 4. `buffer`（秩 4）——**本卡的诚实闸**。按 `-S` 折算的容量决定这次要不要落
//!    临时件、落几件。凡"要落盘数为 0"的形态（`-m`/`-c`/`-C`/输入装得下）在这里
//!    就把一切指向临时盘的 ENOSPC 申报判死；这一闸必须排在真正的写出闸之前，
//!    因为它是后面所有落点算术的前提，而不是对结果的补充说明。
//! 5. `spill`（秩 5）——往已经开出来的临时件里写字节。落点在第 `room+1` 件，
//!    `emitted` 恒为 0（归并还没开始，一行都没出去）。这是本卡验收文案那一枪：
//!    `sort: write failed: '/tmp/sortZ3fQaL': No space left on device`，退 2。
//! 6. `merge`（秩 6）——k 路归并的回读与合并缓冲。`sort: memory exhausted` 与
//!    `sort: cannot read: '<tmp>': No such file or directory` 在这一闸出局，两者
//!    都**不是**空间事故；归并还没读完就不许声称最终输出红过，所以它排在输出闸前。
//! 7. `output`（秩 7）——最终写：stdout 或 `-o` 目的地。这一闸独有的是
//!    `emitted > 0` 的可能：**已经吐出去的残缺有序流**。它必须排在归并之后：
//!    没有归并就没有输出；也必须排在清理之前：残留与输入完好性是对"这一整次"
//!    的结算，不是对某一次写的结算。
//! 8. `cleanup`（秩 8）——临时件回收与输入完好性红线。`sort` 退出前要把自己的
//!    chunk 文件全删掉（残留 = 降级不合格，本卡不落锚）；输入文件必须逐字节
//!    原封不动（受损 = 红线，本卡判死）。它排在所有失败闸之后：只有现场已经
//!    定性，才谈得上"善后到不到位"。
//! 9. `settle`（秩 9）——成功收尾：码 0、`emitted` 等于输入行数（`-c`/`-C` 为 0）、
//!    临时件回收干净。它是唯一允许封印的秩。
//!
//! ## 关键差异（本卡深度所在）
//!
//! * **与 F11681（ENOSPC 总锚，段基址 `0x6d130`，位距 `0x1e0`）的边界**：总锚判
//!   的是那一次 `write()` 的 (返回, errno) 配对，它不知道这个 `write()` 是往
//!   **临时件**里写还是往**目的地**里写，更不知道这是第几个 chunk 文件。本卡把
//!   锚的现场折算成"落点 × 已吐行数 × 退出码 × 文案句式"四元组，并且只在
//!   `errno_seen == 28` 时允许落降级锚。两卡不互相 import，本卡用自声明的
//!   `ANCHOR_F11681_SEG`/`_GAP` 常量署名，由 `anchors_agree()` 复算对齐。
//! * **与 F11682（cp 中止）/F11683（dd 块账）/F11684（tee 半程）的边界**：那三张
//!   卡的目的地就是**用户要的那个文件**，本卡多出一整层**实现自己的私有目的地**
//!   （临时目录），它对用户不可见、却先于用户目的地耗尽。于是本卡有它们都没有的
//!   两件事：① 同一个 28 分"目录开不出名字"与"名字开得出但写不进"两种句式；
//!   ② 一个"根本没碰盘"的诚实闸。cp 卡判"留了多少字节的半件"，dd 卡判"records
//!   的 `N+M` 与几何是否对得上"，tee 卡判"两条支路的字节分叉宽度"；本卡判的是
//!   **落点归属 + 已吐出去的有序残缺行 + 输入未被污染**，三套数字各管一件事，
//!   互相顶替就是错位。
//! * **与 F11686（sed -i 轮换，本卡直接后置，位距 `0x10`）的边界**：sed 的临时件
//!   开在**目标目录**里，且它的目的是把临时件**换名顶替原件**，所以 sed 卡的红线
//!   是"原件不许被截"；sort 从不删改输入，它的临时件在 `$TMPDIR` 这个**与目的地
//!   可能不同挂载点**的目录里，换名动作根本不存在。同一个满盘，sed 卡问"`notes.txt`
//!   还在不在、多大"，本卡问"第几个 chunk 写挂了几行、已经吐出去几行"。
//! * **与 F11688（touch/mkdir，段基址 `0x6d350`）的边界**：F11688 管**目录项与
//!   inode 分配**那一层（一次 write 都没有），本卡管**字节流动**。本卡的
//!   `tmpdir` 闸认得"目录开不出名字"这一档，并明确把它与"开得出但写不进"分开，
//!   但**不裁定**建目录本身的对错与残留父目录层数——那是 F11688 的账。
//! * **与 F11690（rename ENOSPC）的边界**：本卡的四个落点里**没有 rename**：
//!   `sort` 不需要换名，临时件是删除而不是搬动。凡申报里出现"归并阶段 rename 报
//!   28"的行，本卡按落点越界拒收。
//! * **与 F11695（ENOSPC 与 EDQUOT 分档）的边界**：`$TMPDIR` 落在配额目录里时，
//!   同一个句式后面挂的是 `Quota exceeded`。本卡**不裁配额**，只判死它冒充 ENOSPC
//!   落锚（`ForeignErr`）。
//! * **本卡的排他职责**：① 四落点分表（创建/写出/回读/输出）与各自主语文件名；
//!   ② 诚实闸——`-m`/`-c`/`-C`/全内存四种形态永不落临时盘；③ `emitted` 作为
//!   "残缺有序流已外发"的唯一判据量（前三落点恒 0，输出落点 > 0）；
//!   ④ 输入完好红线与临时件回收；⑤ GNU 退 2 / `-C` 退 1 / busybox 退 1 的家族码表。
//! * **谁判死谁放行**：现场落点与几何自洽、`errno` 为 28、输入逐字节完好、临时件
//!   回收为零、码与家族配对、已吐行数与落点配对 → 放行并落降级锚；把没落盘的
//!   形态报成盘满、把配额/ENOENT/EIO 报成 ENOSPC、`-o` 与 stdout 的残缺行数说谎、
//!   输入被截却报正常、临时件残留却报降级合格 → 一律判死。
//!
//! 判据六维：结束阶段（gate）、退出码（code）、已吐出行数（value）、文案家族与
//! 形态（note）、降级锚位（default：是否"临时面 ENOSPC 且善后合格"）、成败申报
//! （estate）。判序固定：状态 → 阶段 → 退出码 → 行数 → 注解 → 锚位。
//!
//! 语料 14 槽：8 槽申报正确（全内存 1、`-m` 归并 1、落盘成功 1、临时件写出满 1、
//! 目录开不出名 1、stdout 残缺 1、busybox 落盘满 1、`-C` 乱序 1），6 槽里 5 槽
//! 复刻同一"第三个 chunk 写挂"现场、各挂一维错报（Stage/Code/Value/Note/Default），
//! 第 1 槽（Estate）特意拿一条**全内存小输入**申报成临时盘 ENOSPC 失败——本卡的
//! 成败互报维要抓的就是这个越界。账本 14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码，
//! 冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11685）。
pub const SEG_BASE: u32 = 0x6d310;

/// 交叉署名：F11681「ENOSPC 总锚」的段基址。本卡不 import 那个模块的任何符号，
/// 只把段位与位距原样声明一遍，由 `anchors_agree()` 复算对齐。
pub const ANCHOR_F11681_SEG: u32 = 0x6d130;
/// F11681 与本卡的位距（0x6d310 - 0x6d130）。
pub const ANCHOR_F11681_GAP: u32 = 0x1e0;
/// 交叉署名：F11682「cp ENOSPC 中止」的段基址。
pub const ANCHOR_F11682_SEG: u32 = 0x6d140;
/// F11682 与本卡的位距。
pub const ANCHOR_F11682_GAP: u32 = 0x1d0;
/// 交叉署名：F11684「tee 半程失败」的段基址（本卡的上一张工具面卡）。
pub const ANCHOR_F11684_SEG: u32 = 0x6d160;
/// F11684 与本卡的位距。
pub const ANCHOR_F11684_GAP: u32 = 0x1b0;
/// 交叉署名：F11686「sed -i 轮换」的段基址（本卡的直接后置）。
pub const NEXT_F11686_SEG: u32 = 0x6d320;
/// F11686 与本卡的位距。
pub const NEXT_F11686_GAP: u32 = 0x10;

/// 与 F11681 在 errno 面的署名：ENOSPC 在 Linux 就是 28。
pub const ANCHOR_ENOSPC: u32 = 28;
/// 与 F11694（文案逐字卡）在文案面的署名：三条卡共用同一串 strerror 原文。
pub const ANCHOR_ERRNO_TEXT: &str = "No space left on device";
/// 与 F11698（工具退出码矩阵）的署名：sort 的"有麻烦"档就是 2。
pub const ANCHOR_SORT_TROUBLE: u32 = 2;

/// 语料槽数。
pub const CORPUS: usize = 14usize;

/// 账本格数。
pub const SAMPLES: usize = 14usize;

/// 申报正确的槽数。
pub const ACCEPT: usize = 8usize;

/// 申报错误的槽数。
pub const REJECT: usize = 6usize;

/// 档位数。
pub const TIER_COUNT: usize = 4usize;

/// 闸门道数（秩 1..9）。
pub const STAGE_COUNT: u32 = 9u32;

/// 最后一道闸的秩（封印只接受站在这里的账）。
pub const STAGE_LAST_RANK: u32 = 9u32;

/// 落点档位数（无 / 建名 / 写件 / 回读 / 输出 / 内存）。
pub const LOCUS_COUNT: usize = 6usize;

/// 临时目录形态档位数。
pub const TMPDIR_KINDS: usize = 6usize;

/// 账本里统计格的个数（红档格与缺陷格之外）。
pub const STAT_CELLS: usize = 7usize;

/// 本卡的记录几何：模型里一律按 16 字节一条记录折算空间。
pub const LINE_BYTES: u32 = 16;

/// 字节数的合法上界（128 MiB；超过它本卡认为形状非法，不参与判据）。
pub const BYTES_MAX: u32 = 0x0800_0000;

/// 行数的合法上界（1 Mi 条；本卡的算术全部走行计数，越界即视为坏形状）。
pub const LINES_MAX: u32 = 0x0010_0000;

/// 红档率标定值：14 格里恰好中 13 格时的千分率 `(13*1000 + 14/2) / 14`。
pub const RED_RATE: u32 = 929;

/// 全绿千分率。
pub const RATE_FULL: u32 = 1000;

// ---------------------------------------------------------------------------
// errno / 退出码（POSIX、Linux、coreutils、busybox 真值）
// ---------------------------------------------------------------------------

/// 设备无空间（strerror: `No space left on device`）——本卡唯一的中止源。
pub const ERR_ENOSPC: u32 = 28;
/// 路径上没有这个东西（strerror: `No such file or directory`）——归并重读上一段。
pub const ERR_ENOENT: u32 = 2;
/// 不是目录（strerror: `Not a directory`）——`TMPDIR` 指到了普通文件上。
pub const ERR_ENOTDIR: u32 = 20;
/// 权限不足（strerror: `Permission denied`）——`TMPDIR` 不可写。
pub const ERR_EACCES: u32 = 13;
/// 设备错（strerror: `Input/output error`）——邻居，不许冒充 ENOSPC。
pub const ERR_EIO: u32 = 5;
/// 配额超限（strerror: `Quota exceeded`）——邻居，归 F11695。
pub const ERR_EDQUOT: u32 = 122;
/// 内存不够（strerror: `Cannot allocate memory`）——`sort: memory exhausted`。
pub const ERR_ENOMEM: u32 = 12;
/// 成功时 errno 应停放的零值。
pub const ERR_NONE: u32 = 0;

/// 合法 errno 观测集合的大小（0/28/2/20/13/5/122/12 八个）。
pub const ERRNO_SET_SIZE: usize = 8usize;

/// 全部落定，正常收尾。
pub const EXIT_OK: u32 = 0;
/// busybox 的"有麻烦"档，以及 GNU `-C` 的乱序档。
pub const EXIT_QUIET_FAIL: u32 = 1;
/// GNU sort 的"有麻烦"档：临时面、目的地、用法全在这一档。
pub const EXIT_TROUBLE: u32 = 2;
/// 本卡不裁定的占位码。它**不是** sort 的任何真实退出码，故意取一个撞不上的
/// 值，谎报行就不可能被误读成正常行。
pub const EXIT_UNJUDGED: u32 = 99;

/// 退出码档位数量（0/1/2 三档真实码 + 1 档不裁定）。
pub const EXIT_RANKS: usize = 4usize;

// ---------------------------------------------------------------------------
// 选项位（本卡认得的形状；不认识的位即用法越界）
// ---------------------------------------------------------------------------

/// `-m`：只归并**已排序**的输入。流式，一个临时件都不建。
pub const OPT_MERGE: u8 = 0x01;
/// `-c`：检查是否已排序，不排序、不输出。边读边比，不建临时件。
pub const OPT_CHECK: u8 = 0x02;
/// `-C`：同 `-c` 但静默，乱序退 1（GNU 家族里唯一退 1 的失败档）。
pub const OPT_CHECK_QUIET: u8 = 0x04;
/// `-s`：稳定排序（不改变落盘算术，只改变文案之外的可比性）。
pub const OPT_STABLE: u8 = 0x08;
/// `-o FILE`：目的地是文件而不是标准输出（改变第 7 闸的主语名）。
pub const OPT_OUTPUT: u8 = 0x10;
/// `-T DIR`：显式指定临时目录，优先级高于 `$TMPDIR`。
pub const OPT_TDIR: u8 = 0x20;
/// `-S SIZE`：显式指定缓冲，决定第几个 chunk 落盘（本卡的算术来源）。
pub const OPT_BUFSIZE: u8 = 0x40;
/// `-r`：降序（与空间无关，只参与选项位合法性遍历）。
pub const OPT_REVERSE: u8 = 0x80;

/// 本卡认得的选项位全集。
pub const OPT_KNOWN: u8 =
    OPT_MERGE | OPT_CHECK | OPT_CHECK_QUIET | OPT_STABLE | OPT_OUTPUT | OPT_TDIR | OPT_BUFSIZE
        | OPT_REVERSE;

/// "永不落临时盘"的三位（诚实闸的集合论定义）。
pub const OPT_NO_SPILL: u8 = OPT_MERGE | OPT_CHECK | OPT_CHECK_QUIET;

/// 校验类两位（不产出排序流，`emitted` 恒 0）。
pub const OPT_CHECKING: u8 = OPT_CHECK | OPT_CHECK_QUIET;

/// 选项位个数。
pub const OPT_BIT_COUNT: usize = 8usize;

/// 选项位表（遍历与唯一性检查用）。
pub const OPT_BITS: [u8; OPT_BIT_COUNT] = [
    OPT_MERGE,
    OPT_CHECK,
    OPT_CHECK_QUIET,
    OPT_STABLE,
    OPT_OUTPUT,
    OPT_TDIR,
    OPT_BUFSIZE,
    OPT_REVERSE,
];

// ---------------------------------------------------------------------------
// 错误码（13 个，SEG_BASE + 0x01..0x0d）
// ---------------------------------------------------------------------------

/// 01 落点闸申报不符：第 5 闸写挂却说成第 2 闸/第 7 闸。
pub const ST_ESTAGE: u32 = SEG_BASE + 0x01;
/// 02 状态违例：成败互报、闸门封印后再动、账本冻结后再登记。
pub const ST_ESTATE: u32 = SEG_BASE + 0x02;
/// 03 退出码申报不符：GNU 的 2 记成 busybox 的 1，或红了却报 0。
pub const ST_ECODE: u32 = SEG_BASE + 0x03;
/// 04 已吐出行数申报不符：把输入行数当已外行数，或把输出落点的残缺说成 0。
pub const ST_EEMIT: u32 = SEG_BASE + 0x04;
/// 05 注解申报不符：创建/写出/回读/输出四式与 GNU/busybox 两家族混认。
pub const ST_ENOTE: u32 = SEG_BASE + 0x05;
/// 06 降级锚位申报不符：该落锚不落、不该落却落。
pub const ST_EANCH: u32 = SEG_BASE + 0x06;
/// 07 诚实违例：根本没落盘的形态被报成临时盘 ENOSPC（本卡排他维）。
pub const ST_EINMEM: u32 = SEG_BASE + 0x07;
/// 08 输入完整性违例：`sort` 毁了输入件（红线，永不落锚）。
pub const ST_EINPUT: u32 = SEG_BASE + 0x08;
/// 09 计数不符：报表不满、矩阵不平、档位覆盖有洞。
pub const ST_ECOUNT: u32 = SEG_BASE + 0x09;
/// 10 槽位非法：注入槽越界，或账本登记时 slot != len。
pub const ST_ESLOT: u32 = SEG_BASE + 0x0a;
/// 11 指纹不符：冻结后重放对不上，或篡改留痕。
pub const ST_ESEAL: u32 = SEG_BASE + 0x0b;
/// 12 无观测：槽号超出语料、期望表缺格。
pub const ST_ENOBS: u32 = SEG_BASE + 0x0c;
/// 13 账本不平：格数、成败比或对账表任一失衡。
pub const ST_EBAL: u32 = SEG_BASE + 0x0d;

/// 13 个错误码的有序全集（检查器与账本共用）。
pub const ALL_CODES: [u32; 13] = [
    ST_ESTAGE,
    ST_ESTATE,
    ST_ECODE,
    ST_EEMIT,
    ST_ENOTE,
    ST_EANCH,
    ST_EINMEM,
    ST_EINPUT,
    ST_ECOUNT,
    ST_ESLOT,
    ST_ESEAL,
    ST_ENOBS,
    ST_EBAL,
];

/// 错误码个数。
pub const CODE_COUNT: usize = 13usize;

// ---------------------------------------------------------------------------
// 操作码（账本格的操作归类）
// ---------------------------------------------------------------------------

/// 命令行/选项闸。
pub const OP_ARGS: u8 = 0x01;
/// 临时目录闸。
pub const OP_TMPDIR: u8 = 0x02;
/// 缓冲/落盘规划闸。
pub const OP_BUFFER: u8 = 0x03;
/// 临时件写出闸。
pub const OP_SPILL: u8 = 0x04;
/// 归并回读闸。
pub const OP_MERGE: u8 = 0x05;
/// 输出写出闸。
pub const OP_OUTPUT: u8 = 0x06;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x07;
/// 红档率标定。
pub const OP_RATE: u8 = 0x08;
/// 缺陷码格。
pub const OP_DEFECT: u8 = 0x09;

/// 操作码个数。
pub const OP_COUNT: usize = 9usize;

/// 操作码表。
pub const OP_BITS: [u8; OP_COUNT] = [
    OP_ARGS,
    OP_TMPDIR,
    OP_BUFFER,
    OP_SPILL,
    OP_MERGE,
    OP_OUTPUT,
    OP_CORPUS,
    OP_RATE,
    OP_DEFECT,
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

/// 折入一个字（u32 的四字节，高位先折——字节次序即身份）。
fn fold_word(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut k: u32 = 0;
    while k < 4 {
        let shift = 8 * (3 - k);
        x = fold_byte(x, ((v >> shift) & 0xff) as u8);
        k += 1;
    }
    x
}

/// 折入一个布尔。
fn fold_flag(h: u64, v: bool) -> u64 {
    fold_word(h, if v { 1 } else { 0 })
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

/// 折入一段字节名（闸门与账本的留痕用）。
fn fold_name(h: u64, bytes: &[u8]) -> u64 {
    let mut v = fold_str(h, core_str(bytes));
    fold_byte(v, b'|')
}

/// 把 ASCII 字节串当 `&str` 用（本仓只有 ASCII 字面量走这条路，不做校验）。
fn core_str(bytes: &[u8]) -> &str {
    match core_str_safe(bytes) {
        Some(s) => s,
        None => "",
    }
}

/// ASCII 安全的字节转串；出现非 ASCII 就给空串（不 panic、不猜测）。
fn core_str_safe(bytes: &[u8]) -> Option<&str> {
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes.get(i) {
            Some(b) => {
                if *b >= 0x80 {
                    return None;
                }
            }
            None => return None,
        }
        i += 1;
    }
    match core_from_utf8(bytes) {
        Some(s) => Some(s),
        None => None,
    }
}

/// `from_utf8` 的无 panic 薄封装：先做 ASCII 检查，再交给 core 的转换。
fn core_from_utf8(bytes: &[u8]) -> Option<&str> {
    if bytes.is_empty() {
        return Some("");
    }
    match core_utf8_slice(bytes) {
        Some(s) => Some(s),
        None => None,
    }
}

/// 把已通过 ASCII 检查的字节切成 `&str`。
fn core_utf8_slice(bytes: &[u8]) -> Option<&str> {
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes.get(i) {
            Some(b) => {
                if *b >= 0x80 {
                    return None;
                }
            }
            None => return None,
        }
        i += 1;
    }
    // 安全转换的等价写法：core 的 `from_utf8` 在 `no_std` 下同样可用，
    // 但本卡只用 ASCII，留一条显式的错误出口而不是 `unwrap`。
    match core::str::from_utf8(bytes) {
        Ok(s) => Some(s),
        Err(_) => None,
    }
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
fn hex_text(v: u32, width: usize) -> String {
    let mut s = String::new();
    let mut n = width;
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
fn bool_text(v: bool) -> String {
    if v {
        String::from("true")
    } else {
        String::from("false")
    }
}

/// 段位文本（诊断用，定宽 5 位十六进制）。
pub fn seg_text(v: u32) -> String {
    hex_text(v, 5)
}

/// `key=value` 片段。
fn kv(out: &mut String, key: &str, val: u32) {
    out.push_str(key);
    out.push('=');
    out.push_str(dec_text(val).as_str());
}

/// 字段分隔（一个空格）。
fn sep(out: &mut String) {
    out.push(' ');
}

/// `N 行 × 16 字节` 的字节文本（判据量是行，字节只进释义行）。
pub fn bytes_text(lines: u32) -> String {
    match lines.checked_mul(LINE_BYTES) {
        Some(v) => dec_text(v),
        None => String::from("overflow"),
    }
}

/// 行/字节二联文本。
pub fn pair_text(lines: u32) -> String {
    let mut s = dec_text(lines);
    s.push_str(" lines / ");
    s.push_str(bytes_text(lines).as_str());
    s.push_str(" bytes");
    s
}

// ---------------------------------------------------------------------------
// 文本小工具（无 std：自己写的包含/前后缀/计数）
// ---------------------------------------------------------------------------

/// `hay` 是否含 `needle`（逐字节滑窗，不做任何分配）。
pub fn has_sub(hay: &str, needle: &str) -> bool {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        return true;
    }
    if n.len() > h.len() {
        return false;
    }
    let last = h.len() - n.len();
    let mut i = 0usize;
    while i <= last {
        let mut same = true;
        let mut j = 0usize;
        while j < n.len() {
            let a = match h.get(i + j) {
                Some(v) => *v,
                None => {
                    same = false;
                    break;
                }
            };
            let b = match n.get(j) {
                Some(v) => *v,
                None => {
                    same = false;
                    break;
                }
            };
            if a != b {
                same = false;
                break;
            }
            j += 1;
        }
        if same {
            return true;
        }
        i += 1;
    }
    false
}

/// `hay` 是否以 `needle` 开头。
pub fn starts_with(hay: &str, needle: &str) -> bool {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.len() > h.len() {
        return false;
    }
    let mut i = 0usize;
    while i < n.len() {
        let a = match h.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let b = match n.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a != b {
            return false;
        }
        i += 1;
    }
    true
}

/// 单字符出现次数。
pub fn count_char(hay: &str, c: u8) -> u32 {
    let b = hay.as_bytes();
    let mut n = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(x) = b.get(i) {
            if *x == c {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// strerror 与文案（四落点 × 两家族，逐字）
// ---------------------------------------------------------------------------

/// strerror(errno) 的 glibc 原文（本卡涉及的八个值）。
pub fn strerror(code: u32) -> &'static str {
    match code {
        ERR_ENOSPC => "No space left on device",
        ERR_ENOENT => "No such file or directory",
        ERR_ENOTDIR => "Not a directory",
        ERR_EACCES => "Permission denied",
        ERR_EIO => "Input/output error",
        ERR_EDQUOT => "Quota exceeded",
        ERR_ENOMEM => "Cannot allocate memory",
        ERR_NONE => "",
        _ => "Unknown error",
    }
}

/// errno 的符号名（进释义行，不参与判据）。
pub fn errno_name(code: u32) -> &'static str {
    match code {
        ERR_ENOSPC => "ENOSPC",
        ERR_ENOENT => "ENOENT",
        ERR_ENOTDIR => "ENOTDIR",
        ERR_EACCES => "EACCES",
        ERR_EIO => "EIO",
        ERR_EDQUOT => "EDQUOT",
        ERR_ENOMEM => "ENOMEM",
        ERR_NONE => "0",
        _ => "?",
    }
}

/// 本卡认得的 errno 全集（`ERRNO_SET_SIZE` 个）。
pub const ERRNO_SET: [u32; ERRNO_SET_SIZE] = [
    ERR_NONE,
    ERR_ENOSPC,
    ERR_ENOENT,
    ERR_ENOTDIR,
    ERR_EACCES,
    ERR_EIO,
    ERR_EDQUOT,
    ERR_ENOMEM,
];

/// 错误码 → 语义短名（第二形态，供账本与测试复核段位）。
pub fn code_name(code: u32) -> &'static str {
    match code {
        ST_ESTAGE => "st-stage",
        ST_ESTATE => "st-state",
        ST_ECODE => "st-code",
        ST_EEMIT => "st-emit",
        ST_ENOTE => "st-note",
        ST_EANCH => "st-anchor",
        ST_EINMEM => "st-inmem",
        ST_EINPUT => "st-input",
        ST_ECOUNT => "st-count",
        ST_ESLOT => "st-slot",
        ST_ESEAL => "st-seal",
        ST_ENOBS => "st-nobs",
        ST_EBAL => "st-bal",
        _ => "clean",
    }
}

/// 错误码在段内的偏移（`-1` 表示不在本段）。
pub fn code_offset(code: u32) -> i32 {
    if code < SEG_BASE || code > SEG_BASE + 0x0d {
        return -1;
    }
    match code.checked_sub(SEG_BASE) {
        Some(v) => v as i32,
        None => -1,
    }
}

/// GNU sort 的**临时件创建失败**原文：目录是主语，不是文件。
///
/// `sort: cannot create temporary file in '<dir>': <strerror>`
pub fn msg_create_tmp_failed(dir: &str, code: u32) -> String {
    let mut s = String::from("sort: cannot create temporary file in '");
    s.push_str(dir);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU sort 的**临时件写出失败**原文：件名是主语。
///
/// `sort: write failed: '<tmp>': <strerror>`
pub fn msg_spill_write_failed(tmp: &str, code: u32) -> String {
    let mut s = String::from("sort: write failed: '");
    s.push_str(tmp);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU sort 的**目的地文件写出失败**原文（`-o FILE`）。
pub fn msg_out_write_failed(path: &str, code: u32) -> String {
    let mut s = String::from("sort: write failed: '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU sort 的**标准输出写出失败**原文：主语是固定名且带引号。
pub fn msg_stdout_write_failed(code: u32) -> String {
    let mut s = String::from("sort: write failed: 'standard output': ");
    s.push_str(strerror(code));
    s
}

/// GNU sort 的**输入打不开**原文。
///
/// `sort: cannot read: '<in>': <strerror>`
pub fn msg_cannot_read(path: &str, code: u32) -> String {
    let mut s = String::from("sort: cannot read: '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU sort 的归并回读失败：主语是**上一段临时件**，errno 是 ENOENT。
pub fn msg_part_gone(tmp: &str) -> String {
    msg_cannot_read(tmp, ERR_ENOENT)
}

/// GNU sort 的合并缓冲分配失败原文（无 strerror 尾巴）。
pub fn msg_memory_exhausted() -> String {
    String::from("sort: memory exhausted")
}

/// GNU sort 的 `-c` 乱序原文：`sort: -:<行号>: disorder: <记录>`。
pub fn msg_disordered(line: u32, rec: &str) -> String {
    let mut s = String::from("sort: -:");
    s.push_str(dec_text(line).as_str());
    s.push_str(": disorder: ");
    s.push_str(rec);
    s
}

/// GNU sort 的未知选项原文（两行：选项行 + 引导行）。
pub fn msg_gnu_usage(letter: char) -> String {
    let mut s = String::from("sort: invalid option -- '");
    s.push(letter);
    s.push('\'');
    s.push_str("\nTry 'sort --help' for more information.");
    s
}

/// busybox sort 的临时件创建失败原文：**不带**被引号包住的路径。
pub fn msg_bb_create_tmp(code: u32) -> String {
    let mut s = String::from("sort: can't create temp file: ");
    s.push_str(strerror(code));
    s
}

/// busybox sort 的临时件写出失败原文。
pub fn msg_bb_spill_write(code: u32) -> String {
    let mut s = String::from("sort: can't write to temp file: ");
    s.push_str(strerror(code));
    s
}

/// busybox sort 的目的地写出失败原文。
pub fn msg_bb_out_write(code: u32) -> String {
    let mut s = String::from("sort: write error: ");
    s.push_str(strerror(code));
    s
}

/// busybox applet 的用法原文（一行 usage，不带选项字母）。
pub fn msg_bb_usage() -> String {
    String::from("usage: sort [-nrcdfu] [FILE]...")
}

/// 降级锚的现场摘要（本卡自研取证行，把四元组一次摊开）。
pub fn msg_degrade_marker(tmp: &str, fail_at: u32, spills: u32, emitted: u32) -> String {
    let mut s = String::from("sort degrade: locus=");
    s.push_str(tmp);
    s.push_str(" chunk ");
    s.push_str(dec_text(fail_at).as_str());
    s.push_str(" of ");
    s.push_str(dec_text(spills).as_str());
    s.push_str(", emitted ");
    s.push_str(dec_text(emitted).as_str());
    s.push_str(" records");
    s
}

/// 诚实闸的取证行：没落盘却报盘满。
pub fn msg_memory_lie(lines: u32, buf: u32) -> String {
    let mut s = String::from("sort honesty: input ");
    s.push_str(dec_text(lines).as_str());
    s.push_str(" lines fits buffer ");
    s.push_str(dec_text(buf).as_str());
    s.push_str(" — no temporary file was ever created, ENOSPC claim refused");
    s
}

/// 输入完好性的取证行（正判据锚）。
pub fn msg_input_intact(inputs: u32) -> String {
    let mut s = String::from("sort inputs untouched: ");
    s.push_str(dec_text(inputs).as_str());
    s.push_str(" of ");
    s.push_str(dec_text(inputs).as_str());
    s.push_str(" files still byte-for-byte");
    s
}

/// 输入受损的取证行（红线）。
pub fn msg_input_damaged(path: &str, bytes: u32) -> String {
    let mut s = String::from("sort input damaged: '");
    s.push_str(path);
    s.push_str("' holds only ");
    s.push_str(dec_text(bytes).as_str());
    s.push_str(" bytes — anchor withheld regardless of message wording");
    s
}

/// 临时件残留的取证行。
pub fn msg_temp_leak(dir: &str, left: u32, made: u32) -> String {
    let mut s = String::from("sort temp files left behind: ");
    s.push_str(dec_text(left).as_str());
    s.push_str(" of ");
    s.push_str(dec_text(made).as_str());
    s.push_str(" chunk files remain in '");
    s.push_str(dir);
    s.push('\'');
    s
}

/// `TMPDIR` 解析的取证行（三态一句话说清用了哪个目录）。
pub fn msg_tmpdir_source(kind: &TmpDir, dir: &str) -> String {
    match kind {
        TmpDir::Unset => {
            let mut s = String::from("TMPDIR unset: spill target is '");
            s.push_str(dir);
            s.push('\'');
            s
        }
        TmpDir::EnvEmpty => {
            let mut s = String::from("TMPDIR empty (not a user choice): spill target is '");
            s.push_str(dir);
            s.push('\'');
            s
        }
        TmpDir::EnvSet => {
            let mut s = String::from("TMPDIR honoured: spill target is '");
            s.push_str(dir);
            s.push('\'');
            s
        }
        TmpDir::EnvBad => String::from("TMPDIR names something that is not a usable directory"),
        TmpDir::EnvFullDir => {
            String::from("TMPDIR names a directory whose entry slots are exhausted")
        }
        TmpDir::Explicit => {
            let mut s = String::from("-T overrides TMPDIR: spill target is '");
            s.push_str(dir);
            s.push('\'');
            s
        }
    }
}

/// 残缺有序流的取证行（输出落点独有）。
pub fn msg_partial_stream(emitted: u32, need: u32) -> String {
    let mut s = String::from("sort emitted a truncated sorted stream: ");
    s.push_str(dec_text(emitted).as_str());
    s.push_str(" of ");
    s.push_str(dec_text(need).as_str());
    s.push_str(" records are already outside the process");
    s
}

// ---------------------------------------------------------------------------
// 工具家族
// ---------------------------------------------------------------------------

/// 家族个数。
pub const FLAVOR_COUNT: usize = 2usize;

/// `sort` 的实现家族。
///
/// 这一维不是装饰：GNU 与 busybox 在**退码阶梯**、**文案是否带引号路径**、
/// **是否认 `-S`/`-T`** 三处全部不同。把 busybox 的 1 记成 GNU 的档位，或要求
/// busybox 打出带引号的临时件全名，都是本卡 Note 维的死槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    /// GNU coreutils：麻烦退 **2**，只有 `-C` 静默校验退 **1**；路径用单引号包住。
    Gnu,
    /// busybox applet：**一律退 1**；文案是 `sort: can't ...: <strerror>`，
    /// 主语后面**不带**路径，因为 applet 压根没把临时件名留在文案里。
    Busybox,
}

impl Flavor {
    /// 两族全集。
    pub const ALL: [Flavor; FLAVOR_COUNT] = [Flavor::Gnu, Flavor::Busybox];

    /// 序号（0/1）。
    pub fn index(&self) -> usize {
        match self {
            Flavor::Gnu => 0,
            Flavor::Busybox => 1,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Flavor::Gnu => "gnu",
            Flavor::Busybox => "busybox",
        }
    }

    /// 长名（取证行用）。
    pub fn name(&self) -> &'static str {
        match self {
            Flavor::Gnu => "GNU coreutils sort",
            Flavor::Busybox => "busybox sort applet",
        }
    }

    /// 这一族的**麻烦退码**：GNU 2，busybox 1。
    ///
    /// GNU 的 `sort` 把"用法错误 / 打不开输入 / 写挂 / 校验乱序"统归 trouble
    /// （EXIT_TROUBLE = 2），而 busybox 的 applet 走 `bb_error_msg` + `EXIT_FAILURE`
    /// 一律退 1。这是本卡 Code 维最常被认错的一格。
    pub fn trouble_exit(&self) -> u32 {
        match self {
            Flavor::Gnu => EXIT_TROUBLE,
            Flavor::Busybox => EXIT_QUIET_FAIL,
        }
    }

    /// 这一族的**用法错误退码**：GNU 2，busybox 1。
    pub fn usage_exit(&self) -> u32 {
        self.trouble_exit()
    }

    /// 这一族的**校验静默档退码**：`-C` 只说不、不解释，两族都是 1。
    ///
    /// 注意 GNU 在这里**打破**了自己的阶梯：`-c` 乱序退 2 并打 `disorder:` 行，
    /// `-C` 乱序退 1 且**什么都不打**。同一份乱序输入，两档差一个码。
    pub fn quiet_check_exit(&self) -> u32 {
        let _ = self;
        EXIT_QUIET_FAIL
    }

    /// 这一族的文案是否把路径用单引号包住。
    pub fn quotes_paths(&self) -> bool {
        match self {
            Flavor::Gnu => true,
            Flavor::Busybox => false,
        }
    }

    /// 这一族是否认 `-T DIR`（临时目录覆盖）。
    ///
    /// GNU 认；busybox 的 `sort` applet 没有 `-T`，它的临时件固定在
    /// `_PATH_TMPDIR`，所以"busybox 用 `-T` 把落点指到满盘"这条记录**不成立**。
    pub fn accepts_tempdir_flag(&self) -> bool {
        match self {
            Flavor::Gnu => true,
            Flavor::Busybox => false,
        }
    }

    /// 这一族是否认 `-S SIZE`（缓冲上限）。
    ///
    /// 只有 GNU 有 `-S`；busybox 按 `MAX_SMALL_BUFFER` 之类的固定档走，
    /// "busybox 上 `-S 1` 强制落盘"这条记录同样**不成立**。
    pub fn accepts_buffer_size_flag(&self) -> bool {
        match self {
            Flavor::Gnu => true,
            Flavor::Busybox => false,
        }
    }

    /// 这一族的临时件命名前缀（取证用，不参与判定）。
    pub fn temp_stem(&self) -> &'static str {
        match self {
            Flavor::Gnu => "sort",
            Flavor::Busybox => "sort",
        }
    }

    /// 这一族是否会为归并建立临时件（`-m`/`-c`/`-C` 之外恒为真）。
    pub fn spills(&self) -> bool {
        true
    }

    /// 折叠进指纹。
    pub fn fold(&self, h: u64) -> u64 {
        fold_word(h, self.index() as u32 + 1)
    }
}

/// 两族是否在这条记录上自洽（家族与选项、文案互相顶替 = 越界）。
pub fn family_accepts(flavor: &Flavor, opts: u8) -> bool {
    if !flavor.accepts_tempdir_flag() && (opts & OPT_TDIR != 0) {
        return false;
    }
    if !flavor.accepts_buffer_size_flag() && (opts & OPT_BUFSIZE != 0) {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 临时目录来源
// ---------------------------------------------------------------------------

/// `sort` 落盘目录的**来源**六态。
///
/// 这一维决定"目录开不出名字"该记在谁头上：未设/空值都退回缺省目录，
/// 它们不是用户的选择；只有 `EnvSet`/`Explicit` 才是用户把落点指过去的那只手。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TmpDir {
    /// `TMPDIR` 未设 → 落点为缺省 `/tmp`。
    Unset,
    /// `TMPDIR=""` 空值 → 同样落缺省 `/tmp`（空串**不算**用户指了个坏目录）。
    EnvEmpty,
    /// `TMPDIR` 设成了一个可用目录。
    EnvSet,
    /// `TMPDIR` 指向的不是目录（ENOTDIR/EACCES/ENOENT）。
    EnvBad,
    /// `TMPDIR` 是目录，但**目录项/inode/日志面已尽**，开不出新名字。
    EnvFullDir,
    /// `-T DIR` 覆盖了环境量（只有 GNU 认这一维）。
    Explicit,
}

impl TmpDir {
    /// 六态全集。
    pub const ALL: [TmpDir; TMPDIR_KINDS] = [
        TmpDir::Unset,
        TmpDir::EnvEmpty,
        TmpDir::EnvSet,
        TmpDir::EnvBad,
        TmpDir::EnvFullDir,
        TmpDir::Explicit,
    ];

    /// 序号（0..=5）。
    pub fn index(&self) -> usize {
        match self {
            TmpDir::Unset => 0,
            TmpDir::EnvEmpty => 1,
            TmpDir::EnvSet => 2,
            TmpDir::EnvBad => 3,
            TmpDir::EnvFullDir => 4,
            TmpDir::Explicit => 5,
        }
    }

    /// 由序号还原（越界 → `None`，本卡不允许凭空造第六态之外的档）。
    pub fn from_index(i: usize) -> Option<TmpDir> {
        match TmpDir::ALL.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            TmpDir::Unset => "unset",
            TmpDir::EnvEmpty => "empty",
            TmpDir::EnvSet => "env",
            TmpDir::EnvBad => "bad",
            TmpDir::EnvFullDir => "envfull",
            TmpDir::Explicit => "flag",
        }
    }

    /// 是否落回缺省目录（未设与空值都是）。
    pub fn falls_back(&self) -> bool {
        matches!(self, TmpDir::Unset | TmpDir::EnvEmpty)
    }

    /// 是否是**用户主动**指定的落点。
    ///
    /// `EnvBad` 也算主动（用户确实指了个坏地方），`EnvEmpty` 不算——
    /// 这一格是本卡与"环境变量语义"卡最容易互抄的地方。
    pub fn is_user_choice(&self) -> bool {
        matches!(
            self,
            TmpDir::EnvSet | TmpDir::EnvBad | TmpDir::EnvFullDir | TmpDir::Explicit
        )
    }

    /// 目录本身可用吗（可用 ≠ 写得进：`EnvFullDir` 可用但开不出名字）。
    pub fn usable(&self) -> bool {
        !matches!(self, TmpDir::EnvBad)
    }

    /// 能否**开出一个件名**：`EnvFullDir` 与 `EnvBad` 都不能。
    pub fn can_create(&self) -> bool {
        matches!(self, TmpDir::Unset | TmpDir::EnvEmpty | TmpDir::EnvSet | TmpDir::Explicit)
    }

    /// 这一态隐含的 errno（0 = 没有失败）。
    pub fn errno_implied(&self) -> u32 {
        match self {
            TmpDir::Unset | TmpDir::EnvEmpty | TmpDir::EnvSet | TmpDir::Explicit => ERR_NONE,
            TmpDir::EnvBad => ERR_ENOTDIR,
            TmpDir::EnvFullDir => ERR_ENOSPC,
        }
    }

    /// 折叠进指纹。
    pub fn fold(&self, h: u64) -> u64 {
        fold_word(h, self.index() as u32 + 1)
    }
}

// ---------------------------------------------------------------------------
// 落点
// ---------------------------------------------------------------------------

/// ENOSPC 在 `sort` 上的**落点**六档。
///
/// 本卡的排他职责就在这里：同一个 28，主语不同、后果不同、可否降级不同。
/// `Nowhere`（一个临时件都没建）与 `Memory`（合并缓冲分配失败）**不是**空间事故，
/// 把它们混进降级锚是本卡的头号错案。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locus {
    /// 没有落点：全内存排序，或 `-m`/`-c`/`-C` 形态。
    Nowhere,
    /// 临时件**创建**失败：目录项/inode/日志面尽，一个名字都没开出来。
    TmpCreate,
    /// 临时件**写出**失败：件在，数据块写不进去。
    SpillWrite,
    /// 归并**回读**失败：件被回收逻辑删了，是 ENOENT 不是 ENOSPC。
    MergeRead,
    /// 最终**输出**失败：stdout 或 `-o` 目的地满，已吐的行收不回来。
    OutWrite,
    /// 合并缓冲分配失败：`sort: memory exhausted`，是 ENOMEM 不是 ENOSPC。
    Memory,
}

impl Locus {
    /// 六档全集。
    pub const ALL: [Locus; LOCUS_COUNT] = [
        Locus::Nowhere,
        Locus::TmpCreate,
        Locus::SpillWrite,
        Locus::MergeRead,
        Locus::OutWrite,
        Locus::Memory,
    ];

    /// 序号（0..=5）。
    pub fn index(&self) -> usize {
        match self {
            Locus::Nowhere => 0,
            Locus::TmpCreate => 1,
            Locus::SpillWrite => 2,
            Locus::MergeRead => 3,
            Locus::OutWrite => 4,
            Locus::Memory => 5,
        }
    }

    /// 由序号还原。
    pub fn from_index(i: usize) -> Option<Locus> {
        match Locus::ALL.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Locus::Nowhere => "nowhere",
            Locus::TmpCreate => "tmp-create",
            Locus::SpillWrite => "spill-write",
            Locus::MergeRead => "merge-read",
            Locus::OutWrite => "out-write",
            Locus::Memory => "memory",
        }
    }

    /// 长名（取证行的主语字段）。
    pub fn name(&self) -> &'static str {
        match self {
            Locus::Nowhere => "进程内存（未落盘）",
            Locus::TmpCreate => "临时目录（开不出件名）",
            Locus::SpillWrite => "临时件（写不进数据块）",
            Locus::MergeRead => "已回落的临时件（回读不到）",
            Locus::OutWrite => "目的地（stdout 或 -o 文件）",
            Locus::Memory => "合并缓冲（ENOMEM）",
        }
    }

    /// 文案的主语是什么（`<none>`/目录/件名/固定名）。
    pub fn subject(&self) -> &'static str {
        match self {
            Locus::Nowhere => "none",
            Locus::TmpCreate => "dir",
            Locus::SpillWrite => "temp-file",
            Locus::MergeRead => "temp-file",
            Locus::OutWrite => "output",
            Locus::Memory => "memory",
        }
    }

    /// 是否**碰过**临时盘（创建/写出/回读三档都碰过）。
    pub fn touches_temp(&self) -> bool {
        matches!(self, Locus::TmpCreate | Locus::SpillWrite | Locus::MergeRead)
    }

    /// 是否落在目的地那一路。
    pub fn names_output(&self) -> bool {
        matches!(self, Locus::OutWrite)
    }

    /// 这一档**允许**已吐出行数非零吗？
    ///
    /// 只有输出落点可以：前三档（创建/写出/回读）都在归并开始之前，
    /// `emitted` 恒为 0；把它们记成非零是本卡 Value 维的死槽。
    pub fn may_emit(&self) -> bool {
        matches!(self, Locus::OutWrite)
    }

    /// 这一档上 ENOSPC 是不是**真**空间事故（可落降级锚的前提）。
    pub fn space_legit(&self) -> bool {
        matches!(self, Locus::TmpCreate | Locus::SpillWrite | Locus::OutWrite)
    }

    /// 这一档归到第几道闸。
    pub fn stage(&self) -> Stage {
        match self {
            Locus::Nowhere => Stage::Buffer,
            Locus::TmpCreate => Stage::Tmpdir,
            Locus::SpillWrite => Stage::Spill,
            Locus::MergeRead => Stage::Merge,
            Locus::OutWrite => Stage::Output,
            Locus::Memory => Stage::Merge,
        }
    }

    /// 这一档是否越界（本卡不认的落点，如"归并阶段 rename 报 28"）。
    pub fn in_bounds(&self) -> bool {
        true
    }

    /// 折叠进指纹。
    pub fn fold(&self, h: u64) -> u64 {
        fold_word(h, self.index() as u32 + 1)
    }
}

/// 落点与目录来源是否自洽：只有主动指过去（`-T`/`TMPDIR`）才谈得上"落点是满盘"。
pub fn locus_dir_agrees(locus: &Locus, dir: &TmpDir) -> bool {
    if !locus.touches_temp() {
        return true;
    }
    match dir {
        TmpDir::Explicit | TmpDir::EnvFullDir | TmpDir::EnvBad => true,
        _ => locus.can_be_default_dir(),
    }
}

impl Locus {
    /// 这一档可否就发生在缺省 `/tmp` 上。
    pub fn can_be_default_dir(&self) -> bool {
        matches!(self, Locus::SpillWrite | Locus::OutWrite | Locus::MergeRead)
    }
}

// ---------------------------------------------------------------------------
// 九道闸
// ---------------------------------------------------------------------------

/// `sort` 的纵线阶段（秩 1..=9，与文件头**闸门次序**一一对应）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 秩 1：命令行本身（未知选项、`-S`/`-T` 取值不成形状）。
    Args,
    /// 秩 2：临时目录能不能开出一个件名。
    Tmpdir,
    /// 秩 3：输入的打开与读（含 `-c`/`-C` 边读边比的乱序判定）。
    Input,
    /// 秩 4：缓冲规划——这次到底要不要落盘、落几件（**诚实闸**）。
    Buffer,
    /// 秩 5：往已开出来的临时件里写字节。
    Spill,
    /// 秩 6：k 路归并的回读与合并缓冲。
    Merge,
    /// 秩 7：最终写出（stdout 或 `-o` 目的地），唯一可有 `emitted > 0` 的闸。
    Output,
    /// 秩 8：临时件回收与输入完好性红线。
    Cleanup,
    /// 秩 9：成功收尾，唯一允许封印的秩。
    Settle,
}

impl Stage {
    /// 九闸全集（按秩升序）。
    pub const ALL: [Stage; STAGE_COUNT as usize] = [
        Stage::Args,
        Stage::Tmpdir,
        Stage::Input,
        Stage::Buffer,
        Stage::Spill,
        Stage::Merge,
        Stage::Output,
        Stage::Cleanup,
        Stage::Settle,
    ];

    /// 秩（1 起算，与闸门次序同步）。
    pub fn rank(&self) -> u32 {
        match self {
            Stage::Args => 1,
            Stage::Tmpdir => 2,
            Stage::Input => 3,
            Stage::Buffer => 4,
            Stage::Spill => 5,
            Stage::Merge => 6,
            Stage::Output => 7,
            Stage::Cleanup => 8,
            Stage::Settle => 9,
        }
    }

    /// 由秩还原（越界 → `None`）。
    pub fn from_rank(r: u32) -> Option<Stage> {
        if r == 0 || r > STAGE_LAST_RANK {
            return None;
        }
        match r {
            1 => Some(Stage::Args),
            2 => Some(Stage::Tmpdir),
            3 => Some(Stage::Input),
            4 => Some(Stage::Buffer),
            5 => Some(Stage::Spill),
            6 => Some(Stage::Merge),
            7 => Some(Stage::Output),
            8 => Some(Stage::Cleanup),
            _ => Some(Stage::Settle),
        }
    }

    /// 序号（0..=8）。
    pub fn index(&self) -> usize {
        match self.rank().checked_sub(1) {
            Some(v) => v as usize,
            None => 0,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Stage::Args => "args",
            Stage::Tmpdir => "tmpdir",
            Stage::Input => "input",
            Stage::Buffer => "buffer",
            Stage::Spill => "spill",
            Stage::Merge => "merge",
            Stage::Output => "output",
            Stage::Cleanup => "cleanup",
            Stage::Settle => "settle",
        }
    }

    /// 长名。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Args => "命令行闸",
            Stage::Tmpdir => "临时目录闸",
            Stage::Input => "输入闸",
            Stage::Buffer => "缓冲规划闸",
            Stage::Spill => "临时件写出闸",
            Stage::Merge => "归并回读闸",
            Stage::Output => "输出写出闸",
            Stage::Cleanup => "清理与红线闸",
            Stage::Settle => "收尾结算闸",
        }
    }

    /// 下一闸（第九闸没有下一闸）。
    pub fn next(&self) -> Option<Stage> {
        Stage::from_rank(self.rank() + 1)
    }

    /// 是否严格在前。
    pub fn before(&self, other: &Stage) -> bool {
        self.rank() < other.rank()
    }

    /// 是否属于"开局三闸"（还没往盘上写任何一个字节）。
    pub fn is_opening(&self) -> bool {
        matches!(self, Stage::Args | Stage::Tmpdir | Stage::Input)
    }

    /// 是否属于"落盘四闸"（缓冲规划 → 输出写出）。
    pub fn is_writing(&self) -> bool {
        matches!(
            self,
            Stage::Buffer | Stage::Spill | Stage::Merge | Stage::Output
        )
    }

    /// 是否属于"结算两闸"。
    pub fn settles(&self) -> bool {
        matches!(self, Stage::Cleanup | Stage::Settle)
    }

    /// 这一闸失败时是否**必然**没吐过行（输出闸之后才可能有残缺有序流）。
    pub fn emits_nothing(&self) -> bool {
        self.rank() < Stage::Output.rank()
    }

    /// 这一闸是否可以承载空间事故。
    pub fn can_host_space(&self) -> bool {
        matches!(
            self,
            Stage::Tmpdir | Stage::Spill | Stage::Output | Stage::Cleanup
        )
    }

    /// 这一闸是不是本卡的诚实闸。
    pub fn is_honesty(&self) -> bool {
        matches!(self, Stage::Buffer)
    }

    /// 折叠进指纹。
    pub fn fold(&self, h: u64) -> u64 {
        fold_word(h, self.rank())
    }
}

/// 秩 → 闸的一一映射自检用表（与 `Stage::ALL` 同序）。
pub const STAGE_RANKS: [u32; STAGE_COUNT as usize] = [1, 2, 3, 4, 5, 6, 7, 8, 9];

// ---------------------------------------------------------------------------
// 文案档位（Note）
// ---------------------------------------------------------------------------

/// 文案档位个数。
pub const NOTE_COUNT: usize = 24usize;

/// 成族（秩 0..=3）。
pub const NOTE_FIRST_OK: u32 = 0;
pub const NOTE_LAST_OK: u32 = 3;
/// 空间族（秩 4..=10）。
pub const NOTE_FIRST_SPACE: u32 = 4;
pub const NOTE_LAST_SPACE: u32 = 10;
/// 异因族（秩 11..=17）：红，但不是空间事故。
pub const NOTE_FIRST_FOREIGN: u32 = 11;
pub const NOTE_LAST_FOREIGN: u32 = 17;
/// 拒收族（秩 18..=23）：本卡不给真实档，记录本身无可采信现场。
pub const NOTE_FIRST_REFUSED: u32 = 18;
pub const NOTE_LAST_REFUSED: u32 = 23;

/// `sort` 的文案/形态档位——本卡 Note 维的全部取值。
///
/// 命名规则：`*Gnu` 是 GNU 句式，`*Busy` 是 busybox 句式，其余与家族无关。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 秩 0：全内存排完，一行临时件都没建，退 0。
    DoneFull,
    /// 秩 1：落了盘、归并完、临时件回收干净，退 0。
    DoneSpilled,
    /// 秩 2：`-m` 流式归并已排序输入，不建临时件，退 0。
    DoneMerge,
    /// 秩 3：`-c`/`-C` 顺序校验通过，退 0。
    DoneCheckOk,
    /// 秩 4：`sort: cannot create temporary file in '<dir>': <err>`。
    TmpCreateGnu,
    /// 秩 5：`sort: can't create temp file: <err>`（busybox，无引号路径）。
    TmpCreateBusy,
    /// 秩 6：`sort: write failed: '<tmp>': No space left on device`。
    SpillWriteGnu,
    /// 秩 7：`sort: can't write to temp file: <err>`（busybox）。
    SpillWriteBusy,
    /// 秩 8：`sort: write failed: '<out>': <err>`（`-o` 目的地）。
    OutFileGnu,
    /// 秩 9：`sort: write failed: 'standard output': <err>`。
    StdoutGnu,
    /// 秩 10：`sort: write error: <err>`（busybox 的目的地档）。
    OutBusy,
    /// 秩 11：`sort: cannot read: '<tmp>': No such file or directory`（归并回读）。
    PartGone,
    /// 秩 12：`sort: memory exhausted`（合并缓冲，ENOMEM）。
    MemExhausted,
    /// 秩 13：`sort: cannot read: '<in>': No such file or directory`（输入闸）。
    CannotRead,
    /// 秩 14：`sort: -:L: disorder: REC`（`-c`，退 2）。
    Disordered,
    /// 秩 15：`-C` 静默乱序，什么都不打，退 1。
    DisorderedQuiet,
    /// 秩 16：`TMPDIR` 指到的不是可用目录。
    TmpdirBad,
    /// 秩 17：配额/EIO 冒充 ENOSPC（`ForeignErr`）。
    ForeignErr,
    /// 秩 18：诚实闸拒收——没落盘却报临时盘 ENOSPC。
    MemoryLie,
    /// 秩 19：落点越界（本卡不认的落点，如 rename）。
    LocusLie,
    /// 秩 20：输入被截/被毁（红线，永不落锚）。
    InputDamaged,
    /// 秩 21：临时件残留（降级不合格）。
    TempLeak,
    /// 秩 22：几何自相矛盾（件数与行数/缓冲对不上）。
    PlanBroken,
    /// 秩 23：用法档（未知选项、缺参）。
    Usage,
}

impl Note {
    /// 全 24 档（按秩升序）。
    pub const ALL: [Note; NOTE_COUNT] = [
        Note::DoneFull,
        Note::DoneSpilled,
        Note::DoneMerge,
        Note::DoneCheckOk,
        Note::TmpCreateGnu,
        Note::TmpCreateBusy,
        Note::SpillWriteGnu,
        Note::SpillWriteBusy,
        Note::OutFileGnu,
        Note::StdoutGnu,
        Note::OutBusy,
        Note::PartGone,
        Note::MemExhausted,
        Note::CannotRead,
        Note::Disordered,
        Note::DisorderedQuiet,
        Note::TmpdirBad,
        Note::ForeignErr,
        Note::MemoryLie,
        Note::LocusLie,
        Note::InputDamaged,
        Note::TempLeak,
        Note::PlanBroken,
        Note::Usage,
    ];

    /// 秩（0..=23）。
    pub fn rank(&self) -> u32 {
        match self {
            Note::DoneFull => 0,
            Note::DoneSpilled => 1,
            Note::DoneMerge => 2,
            Note::DoneCheckOk => 3,
            Note::TmpCreateGnu => 4,
            Note::TmpCreateBusy => 5,
            Note::SpillWriteGnu => 6,
            Note::SpillWriteBusy => 7,
            Note::OutFileGnu => 8,
            Note::StdoutGnu => 9,
            Note::OutBusy => 10,
            Note::PartGone => 11,
            Note::MemExhausted => 12,
            Note::CannotRead => 13,
            Note::Disordered => 14,
            Note::DisorderedQuiet => 15,
            Note::TmpdirBad => 16,
            Note::ForeignErr => 17,
            Note::MemoryLie => 18,
            Note::LocusLie => 19,
            Note::InputDamaged => 20,
            Note::TempLeak => 21,
            Note::PlanBroken => 22,
            Note::Usage => 23,
        }
    }

    /// 序号（与秩同值，语料槽位算术用）。
    pub fn index(&self) -> usize {
        self.rank() as usize
    }

    /// 由秩还原（越界 → `None`）。
    pub fn from_rank(r: u32) -> Option<Note> {
        if r >= NOTE_COUNT as u32 {
            return None;
        }
        match Note::ALL.get(r as usize) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 短名（指纹与渲染用）。
    pub fn slug(&self) -> &'static str {
        match self {
            Note::DoneFull => "in-memory",
            Note::DoneSpilled => "spilled-ok",
            Note::DoneMerge => "merge-only",
            Note::DoneCheckOk => "check-ok",
            Note::TmpCreateGnu => "gnu-tmp-create",
            Note::TmpCreateBusy => "bb-tmp-create",
            Note::SpillWriteGnu => "gnu-spill-write",
            Note::SpillWriteBusy => "bb-spill-write",
            Note::OutFileGnu => "gnu-out-file",
            Note::StdoutGnu => "gnu-stdout",
            Note::OutBusy => "bb-out",
            Note::PartGone => "part-gone",
            Note::MemExhausted => "mem-exhausted",
            Note::CannotRead => "cannot-read",
            Note::Disordered => "disorder",
            Note::DisorderedQuiet => "disorder-quiet",
            Note::TmpdirBad => "tmpdir-bad",
            Note::ForeignErr => "foreign-errno",
            Note::MemoryLie => "memory-lie",
            Note::LocusLie => "locus-lie",
            Note::InputDamaged => "input-damaged",
            Note::TempLeak => "temp-leak",
            Note::PlanBroken => "plan-broken",
            Note::Usage => "usage",
        }
    }

    /// 这一档在真实世界里属于成（只有前四档）。
    pub fn success(&self) -> bool {
        self.rank() <= NOTE_LAST_OK
    }

    /// 是否空间族（可落降级锚的**候选**，仍要 errno 与善后过关）。
    pub fn space_family(&self) -> bool {
        let r = self.rank();
        r >= NOTE_FIRST_SPACE && r <= NOTE_LAST_SPACE
    }

    /// 是否异因族（红，但不是空间事故）。
    pub fn foreign_family(&self) -> bool {
        let r = self.rank();
        r >= NOTE_FIRST_FOREIGN && r <= NOTE_LAST_FOREIGN
    }

    /// 是否拒收族（本卡不给真实档）。
    pub fn refused_family(&self) -> bool {
        let r = self.rank();
        r >= NOTE_FIRST_REFUSED && r <= NOTE_LAST_REFUSED
    }

    /// 文案主语是不是**临时盘**（件或目录）。
    pub fn blames_temp(&self) -> bool {
        matches!(
            self,
            Note::TmpCreateGnu | Note::TmpCreateBusy | Note::SpillWriteGnu | Note::SpillWriteBusy
        )
    }

    /// 文案主语是不是**目的地**。
    pub fn blames_output(&self) -> bool {
        matches!(self, Note::OutFileGnu | Note::StdoutGnu | Note::OutBusy)
    }

    /// 文案主语是不是**目录**（而非件名）：只有创建档是。
    pub fn subject_is_dir(&self) -> bool {
        matches!(self, Note::TmpCreateGnu | Note::TmpCreateBusy)
    }

    /// 文案是否点名"standard output"这一固定主语。
    pub fn subject_is_stdout(&self) -> bool {
        matches!(self, Note::StdoutGnu)
    }

    /// 是否 GNU 专属档。
    pub fn gnu_only(&self) -> bool {
        matches!(
            self,
            Note::TmpCreateGnu | Note::SpillWriteGnu | Note::OutFileGnu | Note::StdoutGnu
        )
    }

    /// 是否 busybox 专属档。
    pub fn busybox_only(&self) -> bool {
        matches!(
            self,
            Note::TmpCreateBusy | Note::SpillWriteBusy | Note::OutBusy
        )
    }

    /// 本卡是否拒收这条记录（拒收 ⇒ 连错误码都不给真实档）。
    pub fn refused(&self) -> bool {
        matches!(
            self,
            Note::MemoryLie
                | Note::LocusLie
                | Note::InputDamaged
                | Note::TempLeak
                | Note::PlanBroken
        )
    }

    /// 是否诚实闸产物（本卡排他维）。
    pub fn honesty(&self) -> bool {
        matches!(self, Note::MemoryLie)
    }

    /// 是否输入完整性红线。
    pub fn red_line(&self) -> bool {
        matches!(self, Note::InputDamaged)
    }

    /// 这一档隐含的 errno 归因（0 = 无 errno 参与）。
    pub fn errno_implied(&self) -> u32 {
        match self {
            Note::DoneFull | Note::DoneSpilled | Note::DoneMerge | Note::DoneCheckOk => ERR_NONE,
            Note::TmpCreateGnu
            | Note::TmpCreateBusy
            | Note::SpillWriteGnu
            | Note::SpillWriteBusy
            | Note::OutFileGnu
            | Note::StdoutGnu
            | Note::OutBusy => ERR_ENOSPC,
            Note::PartGone | Note::CannotRead => ERR_ENOENT,
            Note::MemExhausted => ERR_ENOMEM,
            Note::Disordered | Note::DisorderedQuiet | Note::Usage => ERR_NONE,
            Note::TmpdirBad => ERR_ENOTDIR,
            Note::ForeignErr => ERR_EDQUOT,
            Note::MemoryLie | Note::LocusLie => ERR_ENOSPC,
            Note::InputDamaged | Note::TempLeak | Note::PlanBroken => ERR_NONE,
        }
    }

    /// 这一档在指定家族下**应当**打的退出码。
    pub fn exit_code(&self, flavor: &Flavor) -> u32 {
        if self.success() {
            return EXIT_OK;
        }
        match self {
            Note::DisorderedQuiet => EXIT_QUIET_FAIL,
            Note::MemoryLie | Note::LocusLie | Note::InputDamaged | Note::PlanBroken => {
                EXIT_UNJUDGED
            }
            Note::TempLeak => EXIT_UNJUDGED,
            _ => flavor.trouble_exit(),
        }
    }

    /// 这一档可否落降级锚（空间族且非拒收；errno 与善后另判）。
    pub fn anchor_candidate(&self) -> bool {
        self.space_family()
    }

    /// 折叠进指纹。
    pub fn fold(&self, h: u64) -> u64 {
        fold_word(h, self.rank())
    }
}

// ---------------------------------------------------------------------------
// 档位（Tier）
// ---------------------------------------------------------------------------

/// 报表四档：本卡的四种现场归类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 全内存：一个临时件都没建（含 `-m`/`-c`/`-C`）。
    InMem,
    /// 落盘并成功：建过临时件、归并完成、回收干净。
    Spilled,
    /// 降级：临时面 ENOSPC 且善后合格，允许落锚。
    Degraded,
    /// 拒收：谎报、异因、红线、越界。
    Refused,
}

/// 档位个数。
pub const TIER_ALL: [Tier; TIER_COUNT] = [Tier::InMem, Tier::Spilled, Tier::Degraded, Tier::Refused];

impl Tier {
    /// 序号。
    pub fn index(&self) -> usize {
        match self {
            Tier::InMem => 0,
            Tier::Spilled => 1,
            Tier::Degraded => 2,
            Tier::Refused => 3,
        }
    }

    /// 由序号还原。
    pub fn from_index(i: usize) -> Option<Tier> {
        match TIER_ALL.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Tier::InMem => "in-memory",
            Tier::Spilled => "spilled",
            Tier::Degraded => "degraded",
            Tier::Refused => "refused",
        }
    }

    /// 长名。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::InMem => "全内存",
            Tier::Spilled => "落盘成功",
            Tier::Degraded => "降级可锚",
            Tier::Refused => "拒收",
        }
    }

    /// 是否可落降级锚（只有降级档可以）。
    pub fn anchors(&self) -> bool {
        matches!(self, Tier::Degraded)
    }

    /// 折叠进指纹。
    pub fn fold(&self, h: u64) -> u64 {
        fold_word(h, self.index() as u32 + 1)
    }
}

// ---------------------------------------------------------------------------
// 判据缺欠（Fault）
// ---------------------------------------------------------------------------

/// 六维比对的结果（秩 0 = 全合）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 秩 0：申报与事实六维全合。
    Ok,
    /// 秩 1：成败互报（含把全内存现场报成临时盘 ENOSPC 失败）。
    Estate,
    /// 秩 2：落点/结束闸不合。
    Stage,
    /// 秩 3：退出码不合家族阶梯。
    Code,
    /// 秩 4：已吐出行数不合落点算术。
    Value,
    /// 秩 5：文案家族与形态不合。
    Note,
    /// 秩 6：降级锚位不合。
    Default,
}

impl Fault {
    /// 七格全集（含 `Ok`）。
    pub const ALL: [Fault; 7] = [
        Fault::Ok,
        Fault::Estate,
        Fault::Stage,
        Fault::Code,
        Fault::Value,
        Fault::Note,
        Fault::Default,
    ];

    /// 秩（0..=6），即判定次序。
    pub fn rank(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Estate => 1,
            Fault::Stage => 2,
            Fault::Code => 3,
            Fault::Value => 4,
            Fault::Note => 5,
            Fault::Default => 6,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Fault::Ok => "ok",
            Fault::Estate => "estate",
            Fault::Stage => "stage",
            Fault::Code => "code",
            Fault::Value => "value",
            Fault::Note => "note",
            Fault::Default => "default",
        }
    }

    /// 本维的一句话说明（检查器 detail 用）。
    pub fn blurb(&self) -> &'static str {
        match self {
            Fault::Ok => "申报与事实六维全合",
            Fault::Estate => "成败互报：真实退出码的成/败与申报相反",
            Fault::Stage => "落点错位：第几道闸定的性说错了",
            Fault::Code => "退码不合：GNU 的 2 与 busybox 的 1 或 -C 的 1 记混",
            Fault::Value => "行数不合：前三落点恒 0，只有输出落点可非 0",
            Fault::Note => "文案不合：创建/写出/回读/输出四式或家族混认",
            Fault::Default => "锚位不合：降级锚该落不落、不该落却落",
        }
    }

    /// 落到本段的错误码（`Ok` 无码，返回 0）。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Ok => ERR_NONE,
            Fault::Estate => ST_ESTATE,
            Fault::Stage => ST_ESTAGE,
            Fault::Code => ST_ECODE,
            Fault::Value => ST_EEMIT,
            Fault::Note => ST_ENOTE,
            Fault::Default => ST_EANCH,
        }
    }

    /// 是否缺欠（非 `Ok`）。
    pub fn is_fault(&self) -> bool {
        !matches!(self, Fault::Ok)
    }

    /// 折叠进指纹。
    pub fn fold(&self, h: u64) -> u64 {
        fold_word(h, self.rank())
    }
}

/// 判序（固定：状态 → 阶段 → 退出码 → 行数 → 注解 → 锚位）。
pub const JUDGE_ORDER: [Fault; 6] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

/// 判序的秩必须严格递增（本卡判序不可颠倒的机器复核）。
pub fn judge_order_is_strict() -> bool {
    let mut prev: u32 = 0;
    let mut i: usize = 0;
    while i < JUDGE_ORDER.len() {
        let f = match JUDGE_ORDER.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let r = f.rank();
        if r <= prev {
            return false;
        }
        prev = r;
        i += 1;
    }
    prev == 6
}

// ---------------------------------------------------------------------------
// 现场名（目录 / 临时件 / 目的地）
// ---------------------------------------------------------------------------

/// 缺省落盘目录（`TMPDIR` 未设或为空值时用它）。
pub const DIR_DEFAULT: &str = "/tmp";
/// `TMPDIR` 设成的可用目录。
pub const DIR_ENV: &str = "/mnt/data/tmp";
/// `TMPDIR` 指到的非目录。
pub const DIR_BAD: &str = "/mnt/loop/nope";
/// `TMPDIR` 指到的**目录项已尽**的目录。
pub const DIR_FULL: &str = "/mnt/loop";
/// `-T` 指定的目录。
pub const DIR_FLAG: &str = "/var/tmp/run";
/// 输入件名（语料固定用这三个名字，便于文案逐字复核）。
pub const IN_GONE: &str = "gone.txt";
/// 正常输入件名。
pub const IN_DATA: &str = "big.txt";
/// `-o` 目的地名。
pub const OUT_FILE: &str = "sorted.txt";
/// GNU 文案里 stdout 的固定主语。
pub const STDIO_NAME: &str = "standard output";
/// 临时件名后缀表（真实实现是随机 6 字符；语料用定表，逐字可复核）。
pub const TMP_SUFFIXES: [&str; 8] = [
    "Z3fQaL", "hK9tYb", "qW2sVc", "nR7pXe", "bT4mYf", "jV8cQa", "wL3sKd", "fY6tMh",
];

/// 临时件名前缀（`<dir>/sort`）。
pub const TEMP_STEM: &str = "sort";

/// 目录来源 → 目录名。
pub fn dir_name(kind: &TmpDir) -> &'static str {
    match kind {
        TmpDir::Unset | TmpDir::EnvEmpty => DIR_DEFAULT,
        TmpDir::EnvSet => DIR_ENV,
        TmpDir::EnvBad => DIR_BAD,
        TmpDir::EnvFullDir => DIR_FULL,
        TmpDir::Explicit => DIR_FLAG,
    }
}

/// 第 `seq`（1 起）个临时件的全名：`<dir>/sort<后缀>`。
pub fn chunk_name(dir: &str, seq: u32) -> String {
    let mut s = String::from(dir);
    s.push('/');
    s.push_str(TEMP_STEM);
    let idx = match seq.checked_sub(1) {
        Some(v) => (v as usize) % TMP_SUFFIXES.len(),
        None => 0,
    };
    match TMP_SUFFIXES.get(idx) {
        Some(t) => s.push_str(t),
        None => s.push_str("Z3fQaL"),
    }
    s
}

/// 目的地名：`-o FILE` 用文件名，否则用 `standard output` 这一固定主语。
pub fn out_name(opts: u8) -> &'static str {
    if opts & OPT_OUTPUT != 0 {
        OUT_FILE
    } else {
        STDIO_NAME
    }
}

/// 现场主语（按落点取件名/目录名/目的地名）。
pub fn subject_name(s: &Shape) -> String {
    match s.locus {
        Locus::TmpCreate | Locus::Nowhere | Locus::Memory => String::from(dir_name(&s.dir)),
        Locus::SpillWrite | Locus::MergeRead => {
            let seq = if s.fail_at == 0 { 1 } else { s.fail_at };
            chunk_name(dir_name(&s.dir), seq)
        }
        Locus::OutWrite => String::from(out_name(s.opts)),
    }
}

// ---------------------------------------------------------------------------
// 落盘规划
// ---------------------------------------------------------------------------

/// 由几何量推出的落盘计划（本卡诚实闸的输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// 几何上**应当**落几个 chunk 文件（尾段留内存，不计入）。
    pub spills: u32,
    /// 输入字节数（行数 × 定长行）。
    pub bytes: u32,
    /// 留在内存里的尾段行数。
    pub tail: u32,
    /// 本次是否根本不该落盘（`-m`/`-c`/`-C` 或输入装得下）。
    pub no_spill: bool,
    /// 只校验不排序（`-c`/`-C`）。
    pub check_only: bool,
    /// 只归并（`-m`）。
    pub merge_only: bool,
    /// 稳定排序（`-s`，与本卡判定无关，但计入指纹）。
    pub stable: bool,
    /// 成功时应当吐出的行数（校验档为 0）。
    pub emits: u32,
}

impl Plan {
    /// 构造。
    pub const fn new(
        spills: u32,
        bytes: u32,
        tail: u32,
        no_spill: bool,
        check_only: bool,
        merge_only: bool,
        stable: bool,
        emits: u32,
    ) -> Plan {
        Plan {
            spills,
            bytes,
            tail,
            no_spill,
            check_only,
            merge_only,
            stable,
            emits,
        }
    }

    /// 落点是否允许存在（不允许 ⇒ 一切指向临时盘的申报都是谎报）。
    pub fn may_touch_temp(&self) -> bool {
        !self.no_spill
    }

    /// 折叠进指纹。
    pub fn fold(&self, h: u64) -> u64 {
        let h = fold_word(h, self.spills);
        let h = fold_word(h, self.bytes);
        let h = fold_word(h, self.tail);
        let h = fold_flag(h, self.no_spill);
        let h = fold_flag(h, self.check_only);
        let h = fold_flag(h, self.merge_only);
        let h = fold_flag(h, self.stable);
        fold_word(h, self.emits)
    }

    /// 一行渲染（取证用）。
    pub fn render(&self) -> String {
        let mut s = String::from("plan chunks=");
        s.push_str(dec_text(self.spills).as_str());
        s.push_str(", tail=");
        s.push_str(dec_text(self.tail).as_str());
        s.push_str(", bytes=");
        s.push_str(dec_text(self.bytes).as_str());
        s
    }
}

/// 由现场几何量算出落盘计划。
///
/// GNU 的做法是：缓冲装满一整段就甩一个 chunk 文件，**最后不满一段的尾巴留在
/// 内存里**参与归并，所以件数是 `floor((lines-1)/room)` 而不是 `ceil(lines/room)`。
/// 把尾段也记成一个文件，是本卡几何闸专门要抓的偏差（残留数会莫名多一）。
pub fn plan(s: &Shape) -> Plan {
    let check_only = s.opts & OPT_CHECKING != 0;
    let merge_only = s.opts & OPT_MERGE != 0;
    let stable = s.opts & OPT_STABLE != 0;
    let spills = if check_only || merge_only || s.lines == 0 {
        0
    } else if s.room == 0 {
        1
    } else {
        s.lines.saturating_sub(1) / s.room
    };
    let bytes = s.lines.saturating_mul(LINE_BYTES);
    let tail = s.lines.saturating_sub(spills.saturating_mul(s.room));
    let no_spill = spills == 0 || check_only || merge_only;
    let emits = if check_only { 0 } else { s.lines };
    Plan::new(spills, bytes, tail, no_spill, check_only, merge_only, stable, emits)
}

// ---------------------------------------------------------------------------
// 现场（Shape）
// ---------------------------------------------------------------------------

/// 一次 `sort` 运行的**事实面**：观测到什么，就记什么，不含任何结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 实现家族。
    pub flavor: Flavor,
    /// 落盘目录的来源六态。
    pub dir: TmpDir,
    /// 选项位（`OPT_*` 的并集）。
    pub opts: u8,
    /// 输入行数（记录数）。
    pub lines: u32,
    /// 缓冲容量（多少行装满一段）。
    pub room: u32,
    /// 实际开出来的临时件数。
    pub made: u32,
    /// 第几件写挂（0 = 没有写挂）。
    pub fail_at: u32,
    /// 失败落点（成档用 `Nowhere`）。
    pub locus: Locus,
    /// stderr 里出现过的 errno（0 = 没有 errno）。
    pub errno_seen: u32,
    /// 观测到的退出码（可能与事实不符，本卡不采信它，只作为对照）。
    pub code_seen: u32,
    /// 已经交给下游的行数。
    pub emitted: u32,
    /// 输入件是否逐字节原封不动。
    pub inputs_ok: bool,
    /// 退出时残留的临时件数。
    pub left: u32,
    /// 输入是否已有序（`-m`/`-c`/`-C` 的前提）。
    pub ordered: bool,
    /// 命令行里出现了本卡不认的选项（`-z` 之类）——与 `opts` 的已知位正交。
    pub bad_opt: bool,
}

impl Shape {
    /// 常规构造（十四参数；不认的选项位一律为假）。
    pub const fn new(
        flavor: Flavor,
        dir: TmpDir,
        opts: u8,
        lines: u32,
        room: u32,
        made: u32,
        fail_at: u32,
        locus: Locus,
        errno_seen: u32,
        code_seen: u32,
        emitted: u32,
        inputs_ok: bool,
        left: u32,
        ordered: bool,
    ) -> Shape {
        Shape::new_full(
            flavor,
            dir,
            opts,
            lines,
            room,
            made,
            fail_at,
            locus,
            errno_seen,
            code_seen,
            emitted,
            inputs_ok,
            left,
            ordered,
            false,
        )
    }

    /// 完整构造（十五参数，末位是本卡不认的选项位）。
    pub const fn new_full(
        flavor: Flavor,
        dir: TmpDir,
        opts: u8,
        lines: u32,
        room: u32,
        made: u32,
        fail_at: u32,
        locus: Locus,
        errno_seen: u32,
        code_seen: u32,
        emitted: u32,
        inputs_ok: bool,
        left: u32,
        ordered: bool,
        bad_opt: bool,
    ) -> Shape {
        Shape {
            flavor,
            dir,
            opts,
            lines,
            room,
            made,
            fail_at,
            locus,
            errno_seen,
            code_seen,
            emitted,
            inputs_ok,
            left,
            ordered,
            bad_opt,
        }
    }

    /// 本卡的缺省现场：GNU、`TMPDIR` 未设、无选项、零输入、全内存、干净收尾。
    pub const BASE: Shape = Shape {
        flavor: Flavor::Gnu,
        dir: TmpDir::Unset,
        opts: 0,
        lines: 0,
        room: 0,
        made: 0,
        fail_at: 0,
        locus: Locus::Nowhere,
        errno_seen: ERR_NONE,
        code_seen: EXIT_OK,
        emitted: 0,
        inputs_ok: true,
        left: 0,
        ordered: true,
        bad_opt: false,
    };

    /// 折叠现场全部十五维进指纹。
    pub fn fold(&self, h: u64) -> u64 {
        let h = self.flavor.fold(h);
        let h = self.dir.fold(h);
        let h = fold_word(h, self.opts as u32);
        let h = fold_word(h, self.lines);
        let h = fold_word(h, self.room);
        let h = fold_word(h, self.made);
        let h = fold_word(h, self.fail_at);
        let h = self.locus.fold(h);
        let h = fold_word(h, self.errno_seen);
        let h = fold_word(h, self.code_seen);
        let h = fold_word(h, self.emitted);
        let h = fold_flag(h, self.inputs_ok);
        let h = fold_word(h, self.left);
        let h = fold_flag(h, self.ordered);
        fold_flag(h, self.bad_opt)
    }

    /// 现场的一行摘要（渲染与账本共用）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(self.flavor.slug());
        sep(&mut s);
        s.push_str(self.dir.slug());
        sep(&mut s);
        s.push_str("opts=");
        s.push_str(hex_text(self.opts as u32, 2).as_str());
        sep(&mut s);
        s.push_str(self.locus.slug());
        sep(&mut s);
        s.push_str("lines=");
        s.push_str(dec_text(self.lines).as_str());
        sep(&mut s);
        s.push_str("made=");
        s.push_str(dec_text(self.made).as_str());
        sep(&mut s);
        s.push_str("emit=");
        s.push_str(dec_text(self.emitted).as_str());
        s
    }

    /// 是否申报了本卡不认的选项（`-z` 之类；与已知位的并集正交）。
    pub fn has_unknown_opt(&self) -> bool {
        self.bad_opt
    }

    /// 家族是否越界（busybox 报了 `-S`/`-T`）。
    pub fn family_overreach(&self) -> bool {
        !family_accepts(&self.flavor, self.opts)
    }

    /// 是否落了盘。
    pub fn touched_disk(&self) -> bool {
        self.made != 0
    }

    /// 几何是否自相矛盾（申报的件数/落点/已吐行数对不上）。
    pub fn geometry_broken(&self) -> bool {
        let p = plan(self);
        if self.made > p.spills {
            return true;
        }
        if self.fail_at > self.made {
            return true;
        }
        if p.no_spill && (self.made != 0 || self.fail_at != 0) {
            return true;
        }
        if self.emitted > self.lines {
            return true;
        }
        // 失败落点在输出闸之前 → 一行都不许宣称已外发；成功跑（`Nowhere`）不受此限。
        if self.locus != Locus::Nowhere && !self.locus.may_emit() && self.emitted != 0 {
            return true;
        }
        // `-c`/`-C` 从不写输出，吐了行就是假。
        if p.check_only && self.emitted != 0 {
            return true;
        }
        if self.left > self.made {
            return true;
        }
        false
    }

    /// 诚实闸：本卡排他判据——没落盘却报临时盘 ENOSPC。
    ///
    /// 返回 `Err(ST_EINMEM)` 表示这条现场不该有任何指向临时盘的申报。
    pub fn honesty_gate(&self) -> Result<(), u32> {
        let p = plan(self);
        if p.no_spill && self.locus.touches_temp() {
            return Err(ST_EINMEM);
        }
        if self.locus == Locus::Nowhere && self.errno_seen == ERR_ENOSPC {
            return Err(ST_EINMEM);
        }
        if self.locus == Locus::Memory && self.errno_seen == ERR_ENOSPC {
            return Err(ST_EINMEM);
        }
        Ok(())
    }

    /// 输入完整性红线：`sort` 从不改输入，改了就是做错事。
    pub fn input_guard(&self) -> Result<(), u32> {
        if self.inputs_ok {
            Ok(())
        } else {
            Err(ST_EINPUT)
        }
    }

    /// 回收闸：临时件残留 = 降级不合格。
    pub fn cleanup_gate(&self) -> Result<(), u32> {
        if self.left == 0 {
            return Ok(());
        }
        if self.left > self.made {
            return Err(ST_EBAL);
        }
        Err(ST_EANCH)
    }

    /// 落点是否在本卡认的范围内（rename 之类的越界申报由 `Claim` 侧拦）。
    pub fn locus_in_bounds(&self) -> bool {
        self.locus.in_bounds()
    }
}

// ---------------------------------------------------------------------------
// 结论（Outcome）
// ---------------------------------------------------------------------------

/// 本卡对现场算出的**事实面结论**（六维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 定性落在第几道闸。
    pub stage: Stage,
    /// 这一现场**应当**有的退出码（家族阶梯算出来的，不是抄观测的）。
    pub code: u32,
    /// 已吐出的行数（前三落点恒 0，只有输出落点可非 0）。
    pub value: u32,
    /// 文案档位。
    pub note: Note,
    /// 降级锚位：本卡是否允许这次落 ENOSPC 降级锚。
    pub default: bool,
}

impl Outcome {
    /// 构造。
    pub const fn new(stage: Stage, code: u32, value: u32, note: Note, default: bool) -> Outcome {
        Outcome {
            stage,
            code,
            value,
            note,
            default,
        }
    }

    /// 成败位：退 0 才是成。99（未判定）与 1/2 都是败。
    pub fn ok(&self) -> bool {
        self.code == EXIT_OK
    }

    /// 是否属于本卡拒收族。
    pub fn refused(&self) -> bool {
        self.note.refused()
    }

    /// 是否需要打印 stderr（`-C` 与成功档不打印任何东西）。
    pub fn prints(&self) -> bool {
        !self.note.success() && self.note != Note::DisorderedQuiet
    }

    /// 折叠五维进指纹（固定次序：闸→码→行数→文案→锚位）。
    pub fn fold(&self, h: u64) -> u64 {
        let h = self.stage.fold(h);
        let h = fold_word(h, self.code);
        let h = fold_word(h, self.value);
        let h = fold_word(h, self.note.rank());
        fold_flag(h, self.default)
    }

    /// 一行渲染。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("stage=");
        s.push_str(self.stage.slug());
        sep(&mut s);
        s.push_str("exit=");
        s.push_str(dec_text(self.code).as_str());
        sep(&mut s);
        s.push_str("emitted=");
        s.push_str(dec_text(self.value).as_str());
        sep(&mut s);
        s.push_str("note=");
        s.push_str(self.note.slug());
        sep(&mut s);
        s.push_str("anchor=");
        s.push_str(bool_text(self.default).as_str());
        s
    }
}

/// 由现场算出结论（本卡的全部判理集中在这里）。
pub fn define(s: &Shape) -> Outcome {
    let p = plan(s);
    let trouble = s.flavor.trouble_exit();

    // 闸 1：命令行——未知选项位、或家族越界（busybox 报 -S/-T）都定死在这里。
    if s.has_unknown_opt() || s.family_overreach() {
        return Outcome::new(Stage::Args, s.flavor.usage_exit(), 0, Note::Usage, false);
    }

    // 闸 2：临时目录本身不可用（非 ENOSPC 的创建失败）。
    if s.locus == Locus::TmpCreate && s.errno_seen != ERR_ENOSPC {
        return with_red_lines(s, Outcome::new(Stage::Tmpdir, trouble, 0, Note::TmpdirBad, false));
    }

    // 闸 3：输入打不开（主语是输入件，errno 是 ENOENT/EACCES，与盘满无关）。
    if s.locus == Locus::Nowhere && (s.errno_seen == ERR_ENOENT || s.errno_seen == ERR_EACCES) {
        return with_red_lines(s, Outcome::new(Stage::Input, trouble, 0, Note::CannotRead, false));
    }

    // 闸 3b：`-c`/`-C`/`-m` 的乱序判定就发生在边读边比的时候。
    if (p.check_only || p.merge_only) && !s.ordered {
        if s.opts & OPT_CHECK_QUIET != 0 {
            return with_red_lines(
                s,
                Outcome::new(
                    Stage::Input,
                    s.flavor.quiet_check_exit(),
                    0,
                    Note::DisorderedQuiet,
                    false,
                ),
            );
        }
        return with_red_lines(
            s,
            Outcome::new(Stage::Input, trouble, 0, Note::Disordered, false),
        );
    }

    // 闸 4：诚实闸——本次压根不该落盘，却有指向临时盘的申报。
    if let Err(_) = s.honesty_gate() {
        return with_red_lines(
            s,
            Outcome::new(Stage::Buffer, EXIT_UNJUDGED, 0, Note::MemoryLie, false),
        );
    }

    // 闸 4b：几何自相矛盾（件数、残留、已吐行数任一对不上）。
    if s.geometry_broken() {
        return with_red_lines(
            s,
            Outcome::new(Stage::Buffer, EXIT_UNJUDGED, s.emitted, Note::PlanBroken, false),
        );
    }

    // 闸 5/6/7：四个真落点各自的结论。
    match s.locus {
        Locus::TmpCreate | Locus::SpillWrite | Locus::OutWrite => space_outcome(s),
        Locus::MergeRead => {
            if s.errno_seen == ERR_ENOENT {
                with_red_lines(s, Outcome::new(Stage::Merge, trouble, 0, Note::PartGone, false))
            } else if s.errno_seen == ERR_ENOSPC {
                // 回读不会撞 ENOSPC：这是把 rename/目录动作冒充归并读，落点越界。
                with_red_lines(
                    s,
                    Outcome::new(Stage::Merge, EXIT_UNJUDGED, 0, Note::LocusLie, false),
                )
            } else {
                with_red_lines(
                    s,
                    Outcome::new(Stage::Merge, trouble, 0, Note::ForeignErr, false),
                )
            }
        }
        Locus::Memory => with_red_lines(
            s,
            Outcome::new(Stage::Merge, trouble, 0, Note::MemExhausted, false),
        ),
        Locus::Nowhere => success_outcome(s, &p),
    }
}

/// 空间族三落点：errno 必须是 28，否则是配额/EIO 冒充。
fn space_outcome(s: &Shape) -> Outcome {
    let trouble = s.flavor.trouble_exit();
    if s.errno_seen != ERR_ENOSPC {
        let note = if s.errno_seen == ERR_ENOENT || s.errno_seen == ERR_EACCES {
            Note::CannotRead
        } else {
            Note::ForeignErr
        };
        let stage = s.locus.stage();
        return with_red_lines(s, Outcome::new(stage, trouble, 0, note, false));
    }
    let note = match s.locus {
        Locus::TmpCreate => {
            if s.flavor.quotes_paths() {
                Note::TmpCreateGnu
            } else {
                Note::TmpCreateBusy
            }
        }
        Locus::SpillWrite => {
            if s.flavor.quotes_paths() {
                Note::SpillWriteGnu
            } else {
                Note::SpillWriteBusy
            }
        }
        _ => {
            if s.flavor.quotes_paths() {
                if s.opts & OPT_OUTPUT != 0 {
                    Note::OutFileGnu
                } else {
                    Note::StdoutGnu
                }
            } else {
                Note::OutBusy
            }
        }
    };
    let value = if s.locus.may_emit() { s.emitted } else { 0 };
    with_red_lines(
        s,
        Outcome::new(s.locus.stage(), trouble, value, note, true),
    )
}

/// 成功档的四种形态各归各位。
fn success_outcome(s: &Shape, p: &Plan) -> Outcome {
    let note = if p.merge_only {
        Note::DoneMerge
    } else if p.check_only {
        Note::DoneCheckOk
    } else if s.made != 0 {
        Note::DoneSpilled
    } else {
        Note::DoneFull
    };
    let value = if p.check_only { 0 } else { s.emitted };
    with_red_lines(s, Outcome::new(Stage::Settle, EXIT_OK, value, note, false))
}

/// 红线与回收闸后置覆盖：输入受损 / 临时件残留，任一条都把结论钉在第 8 闸。
fn with_red_lines(s: &Shape, base: Outcome) -> Outcome {
    if !s.inputs_ok {
        return Outcome::new(
            Stage::Cleanup,
            EXIT_UNJUDGED,
            base.value,
            Note::InputDamaged,
            false,
        );
    }
    if s.left != 0 && s.made != 0 {
        return Outcome::new(
            Stage::Cleanup,
            EXIT_UNJUDGED,
            base.value,
            Note::TempLeak,
            false,
        );
    }
    base
}

// ---------------------------------------------------------------------------
// 文案渲染（结论 → 真实 stderr 原文）
// ---------------------------------------------------------------------------

/// 把结论折算成这一族**应当**打出的原文。
///
/// `-C` 与成功档返回空串：它们什么都不打。这条"空"本身就是判据——
/// 凡是给 `-C` 配上 `disorder:` 行的记录，都是 Note 维的死槽。
pub fn message(s: &Shape, o: &Outcome) -> String {
    let dir = dir_name(&s.dir);
    let subject = subject_name(s);
    let errno = if s.errno_seen == ERR_NONE {
        o.note.errno_implied()
    } else {
        s.errno_seen
    };
    match o.note {
        Note::DoneFull | Note::DoneSpilled | Note::DoneMerge | Note::DoneCheckOk => {
            String::new()
        }
        Note::DisorderedQuiet => String::new(),
        Note::TmpCreateGnu => msg_create_tmp_failed(dir, errno),
        Note::TmpCreateBusy => msg_bb_create_tmp(errno),
        Note::SpillWriteGnu => msg_spill_write_failed(subject.as_str(), errno),
        Note::SpillWriteBusy => msg_bb_spill_write(errno),
        Note::OutFileGnu => msg_out_write_failed(out_name(s.opts), errno),
        Note::StdoutGnu => msg_stdout_write_failed(errno),
        Note::OutBusy => msg_bb_out_write(errno),
        Note::PartGone => msg_part_gone(subject.as_str()),
        Note::MemExhausted => msg_memory_exhausted(),
        Note::CannotRead => {
            // 输入闸的主语是**输入件**：ENOENT 是那个已消失的名字，EACCES 是本名。
            let name = if s.errno_seen == ERR_EACCES {
                IN_DATA
            } else {
                IN_GONE
            };
            msg_cannot_read(name, errno)
        }
        Note::Disordered => msg_disordered(1, "zeta alpha"),
        Note::TmpdirBad => msg_create_tmp_failed(dir, errno),
        Note::ForeignErr => msg_spill_write_failed(subject.as_str(), errno),
        Note::MemoryLie => msg_memory_lie(s.lines, s.room),
        Note::LocusLie => msg_partial_stream(0, s.lines),
        Note::InputDamaged => msg_input_damaged(IN_DATA, s.lines.saturating_mul(LINE_BYTES) / 2),
        Note::TempLeak => msg_temp_leak(dir, s.left, s.made),
        Note::PlanBroken => msg_memory_lie(s.lines, s.room),
        Note::Usage => {
            if s.flavor.quotes_paths() {
                msg_gnu_usage('?')
            } else {
                msg_bb_usage()
            }
        }
    }
}

/// 结论 + 现场的完整取证行（报表与账本共用）。
pub fn evidence(s: &Shape, o: &Outcome) -> String {
    let mut m = message(s, o);
    if m.is_empty() {
        m = String::from("sort: (no stderr)");
    }
    m.push_str(" | ");
    m.push_str(o.render().as_str());
    m
}

// ---------------------------------------------------------------------------
// 申报（Claim）与案例（Case）
// ---------------------------------------------------------------------------

/// 被复核的那一份结论：六维申报。
///
/// 比 `Outcome` 多一个显式成败位——这一位单独存在是必要的：真实事故里最常见的
/// 谎报不是码写错，而是**日志作者按自己的期待写了个"成功"**，而 stderr 明明
/// 打了 `write failed`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 申报的成败（本次 sort 是否交付了一个完整的有序流）。
    pub success: bool,
    /// 申报的定性闸。
    pub stage: Stage,
    /// 申报的退出码。
    pub code: u32,
    /// 申报的已吐出行数。
    pub value: u32,
    /// 申报的文案档位。
    pub note: Note,
    /// 申报是否落 ENOSPC 降级锚。
    pub default: bool,
}

impl Claim {
    /// 构造。
    pub const fn new(
        success: bool,
        stage: Stage,
        code: u32,
        value: u32,
        note: Note,
        default: bool,
    ) -> Claim {
        Claim {
            success,
            stage,
            code,
            value,
            note,
            default,
        }
    }

    /// 申报的成败位。
    pub fn success_flag(&self) -> bool {
        self.success
    }

    /// 申报折算出的退出码（只区分 0/非 0，真实档位由 `code` 给）。
    pub fn exit_hint(&self) -> u32 {
        if self.success {
            EXIT_OK
        } else {
            EXIT_TROUBLE
        }
    }

    /// 由事实面照抄一份申报（活槽用；死槽在此之后只翻一维）。
    pub fn from_outcome(actual: &Outcome, success: bool) -> Claim {
        Claim::new(
            success,
            actual.stage,
            actual.code,
            actual.value,
            actual.note,
            actual.default,
        )
    }

    /// 折叠六维进指纹（与 `Outcome::fold` 同序，另加成败位）。
    pub fn fold(&self, h: u64) -> u64 {
        let h = fold_flag(h, self.success);
        let h = self.stage.fold(h);
        let h = fold_word(h, self.code);
        let h = fold_word(h, self.value);
        let h = fold_word(h, self.note.rank());
        fold_flag(h, self.default)
    }

    /// 一行渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("claim ok=");
        s.push_str(bool_text(self.success).as_str());
        sep(&mut s);
        s.push_str("stage=");
        s.push_str(self.stage.slug());
        sep(&mut s);
        s.push_str("exit=");
        s.push_str(dec_text(self.code).as_str());
        sep(&mut s);
        s.push_str("emitted=");
        s.push_str(dec_text(self.value).as_str());
        sep(&mut s);
        s.push_str("note=");
        s.push_str(self.note.slug());
        sep(&mut s);
        s.push_str("anchor=");
        s.push_str(bool_text(self.default).as_str());
        s
    }

    /// 只翻**一**维（死槽构造：其余五维照抄事实面）。
    pub fn flip_stage(&self, v: Stage) -> Claim {
        Claim {
            stage: v,
            ..*self
        }
    }

    /// 只翻退出码维。
    pub fn flip_code(&self, v: u32) -> Claim {
        Claim { code: v, ..*self }
    }

    /// 只翻已吐行数维。
    pub fn flip_value(&self, v: u32) -> Claim {
        Claim { value: v, ..*self }
    }

    /// 只翻文案维。
    pub fn flip_note(&self, v: Note) -> Claim {
        Claim { note: v, ..*self }
    }

    /// 只翻锚位维。
    pub fn flip_default(&self, v: bool) -> Claim {
        Claim {
            default: v,
            ..*self
        }
    }

    /// 只翻成败申报维。
    pub fn flip_success(&self, v: bool) -> Claim {
        Claim { success: v, ..*self }
    }
}

/// 一个待判案例：现场 + 申报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 事实面。
    pub shape: Shape,
    /// 申报面。
    pub claim: Claim,
}

impl Case {
    /// 构造。
    pub const fn new(shape: Shape, claim: Claim) -> Case {
        Case { shape, claim }
    }

    /// 折叠两面的指纹。
    pub fn fold(&self, h: u64) -> u64 {
        self.claim.fold(self.shape.fold(h))
    }
}

/// 六维比对（判序固定：状态 → 闸 → 码 → 行数 → 文案 → 锚位）。
pub fn judge(c: &Case) -> Fault {
    let actual = define(&c.shape);
    if actual.ok() != c.claim.success {
        return Fault::Estate;
    }
    if actual.stage != c.claim.stage {
        return Fault::Stage;
    }
    if actual.code != c.claim.code {
        return Fault::Code;
    }
    if actual.value != c.claim.value {
        return Fault::Value;
    }
    if actual.note != c.claim.note {
        return Fault::Note;
    }
    if actual.default != c.claim.default {
        return Fault::Default;
    }
    Fault::Ok
}

/// 第 i 槽的缺欠（越界 → `Fault::Ok` 之外的哨兵由 `verdict_at` 处理）。
pub fn verdict_at(i: usize) -> Option<Fault> {
    let shape = match SHAPES.get(i) {
        Some(v) => *v,
        None => return None,
    };
    let claim = match CLAIMS.get(i) {
        Some(v) => *v,
        None => return None,
    };
    Some(judge(&Case::new(shape, claim)))
}

/// 第 i 槽的事实面。
pub fn shape_at(i: usize) -> Option<Shape> {
    match SHAPES.get(i) {
        Some(v) => Some(*v),
        None => None,
    }
}

/// 第 i 槽的申报（原表）。
pub fn claim_at(i: usize) -> Option<Claim> {
    match CLAIMS.get(i) {
        Some(v) => Some(*v),
        None => None,
    }
}

/// 第 i 槽申报里的行数（期望表的第三形态）。
pub fn want_value(i: usize) -> Option<u32> {
    match WANT_VALUE.get(i) {
        Some(v) => Some(*v),
        None => None,
    }
}

/// 用期望表逐格重建申报（含行数），与 `claim_at` 对账。
pub fn claim_from_want(i: usize) -> Option<Claim> {
    let stage = match WANT_STAGE.get(i) {
        Some(v) => *v,
        None => return None,
    };
    let code = match WANT_CODE.get(i) {
        Some(v) => *v,
        None => return None,
    };
    let note = match WANT_NOTE.get(i) {
        Some(v) => *v,
        None => return None,
    };
    let default = match WANT_DEFAULT.get(i) {
        Some(v) => *v,
        None => return None,
    };
    let success = match WANT_OK.get(i) {
        Some(v) => *v,
        None => return None,
    };
    let value = match WANT_VALUE.get(i) {
        Some(v) => *v,
        None => return None,
    };
    Some(Claim::new(success, stage, code, value, note, default))
}

/// 期望表与申报表逐维一致（本卡的"同一批事实写两遍"复核）。
pub fn want_matches_claims() -> bool {
    let mut i: usize = 0;
    while i < CORPUS {
        let a = match CLAIMS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let b = match claim_from_want(i) {
            Some(v) => v,
            None => return false,
        };
        if a != b {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 语料：八个活形态（事实面）
// ---------------------------------------------------------------------------

/// 槽 1 `in-memory-ok`：8 行小输入，缓冲装得下，一个临时件都不建，退 0 吐 8 行。
///
/// 这一槽是本卡正判据的基准形态：`TMPDIR` 指向满盘也照样成功。
const SHAPE_IN_MEMORY: Shape = Shape {
    lines: 8,
    room: 64,
    made: 0,
    fail_at: 0,
    locus: Locus::Nowhere,
    errno_seen: ERR_NONE,
    code_seen: EXIT_OK,
    emitted: 8,
    left: 0,
    ..Shape::BASE
};

/// 槽 2 `merge-only-ok`：`-m` 归并三份已排序输入，流式，零临时件，退 0 吐 12 行。
const SHAPE_MERGE_OK: Shape = Shape {
    opts: OPT_MERGE,
    lines: 12,
    room: 64,
    emitted: 12,
    ordered: true,
    ..Shape::BASE
};

/// 槽 3 `spilled-ok`：100 行、缓冲 30 行/段 → 落 3 个 chunk 文件，尾段 10 行留内存，
/// 归并完、回收干净、退 0 吐 100 行。
const SHAPE_SPILLED_OK: Shape = Shape {
    dir: TmpDir::EnvSet,
    lines: 100,
    room: 30,
    made: 3,
    emitted: 100,
    ..Shape::BASE
};

/// 槽 4 `spill-write-enospc`（**本卡验收文案那一枪**）：第 3 个 chunk 写挂，
/// stderr 打 `sort: write failed: '<tmp>': No space left on device`，退 2。
///
/// 归并未起跑，所以 `emitted` 恒 0；件名已开出，所以主语是**件**而不是目录。
const SHAPE_SPILL3_ENOSPC: Shape = Shape {
    dir: TmpDir::EnvSet,
    lines: 100,
    room: 30,
    made: 3,
    fail_at: 3,
    locus: Locus::SpillWrite,
    errno_seen: ERR_ENOSPC,
    code_seen: EXIT_TROUBLE,
    emitted: 0,
    ..Shape::BASE
};

/// 槽 5 `tmp-create-enospc`：`TMPDIR` 是目录但目录项已尽，一个名字都开不出来，
/// 主语是**目录**，退 2。与槽 4 同一个 28，句式却不同——两句混认是本卡 Note 维死槽。
const SHAPE_TMP_CREATE_ENOSPC: Shape = Shape {
    dir: TmpDir::EnvFullDir,
    lines: 100,
    room: 30,
    made: 0,
    locus: Locus::TmpCreate,
    errno_seen: ERR_ENOSPC,
    code_seen: EXIT_TROUBLE,
    ..Shape::BASE
};

/// 槽 6 `stdout-partial-enospc`：三段临时件都写成了，归并把 100 行里的 40 行
/// 交给下游之后 `standard output` 满 → 退 2。这是**唯一**允许 `emitted > 0` 的落点。
const SHAPE_STDOUT_PARTIAL: Shape = Shape {
    dir: TmpDir::Unset,
    lines: 100,
    room: 30,
    made: 3,
    locus: Locus::OutWrite,
    errno_seen: ERR_ENOSPC,
    code_seen: EXIT_TROUBLE,
    emitted: 40,
    ..Shape::BASE
};

/// 槽 7 `busybox-spill-enospc`：busybox applet，60 行、缓冲 20 行/段 → 2 件，
/// 第 2 件写挂，退 **1**，文案 `sort: can't write to temp file: ...`（无引号路径）。
const SHAPE_BUSY_SPILL: Shape = Shape {
    flavor: Flavor::Busybox,
    lines: 60,
    room: 20,
    made: 2,
    fail_at: 2,
    locus: Locus::SpillWrite,
    errno_seen: ERR_ENOSPC,
    code_seen: EXIT_QUIET_FAIL,
    ..Shape::BASE
};

/// 槽 8 `check-quiet-disorder`：`-C` 校验乱序输入，**什么都不打**，退 1。
///
/// 这一槽专门用来钉死两件事：校验档从不写输出（`emitted` 必须 0），
/// 且它是 GNU 家族里唯一退 1 的失败档。
const SHAPE_CHECK_QUIET_BAD: Shape = Shape {
    opts: OPT_CHECK_QUIET,
    lines: 6,
    room: 64,
    code_seen: EXIT_QUIET_FAIL,
    ordered: false,
    ..Shape::BASE
};

/// 全部 14 槽的输入形态。后 6 槽的事实面**整槽复用**前面的活形态——
/// 一槽只坏一维，坏的是申报而不是事实，这是语料纪律的硬要求。
pub const SHAPES: [Shape; CORPUS] = [
    SHAPE_IN_MEMORY,
    SHAPE_MERGE_OK,
    SHAPE_SPILLED_OK,
    SHAPE_SPILL3_ENOSPC,
    SHAPE_TMP_CREATE_ENOSPC,
    SHAPE_STDOUT_PARTIAL,
    SHAPE_BUSY_SPILL,
    SHAPE_CHECK_QUIET_BAD,
    // 死 1：事实是全内存跑成功（槽 1），申报却说第 3 件写挂。
    SHAPE_IN_MEMORY,
    // 死 2..5：全部复用槽 4 的"第三个 chunk 写挂"现场。
    SHAPE_SPILL3_ENOSPC,
    SHAPE_SPILL3_ENOSPC,
    SHAPE_SPILL3_ENOSPC,
    SHAPE_SPILL3_ENOSPC,
    // 死 6：复用槽 5 的"目录开不出名字"现场。
    SHAPE_TMP_CREATE_ENOSPC,
];

/// 14 槽的申报结论（活槽与事实面逐维相等；死槽只翻一维）。
pub const CLAIMS: [Claim; CORPUS] = [
    // 槽 1：全内存成功，六维照实。
    Claim::new(true, Stage::Settle, EXIT_OK, 8, Note::DoneFull, false),
    // 槽 2：`-m` 成功，吐 12 行。
    Claim::new(true, Stage::Settle, EXIT_OK, 12, Note::DoneMerge, false),
    // 槽 3：落盘成功，吐 100 行，回收干净。
    Claim::new(true, Stage::Settle, EXIT_OK, 100, Note::DoneSpilled, false),
    // 槽 4：第 3 件写挂，退 2，吐 0 行，落降级锚。
    Claim::new(false, Stage::Spill, EXIT_TROUBLE, 0, Note::SpillWriteGnu, true),
    // 槽 5：目录开不出名字，退 2，落降级锚。
    Claim::new(false, Stage::Tmpdir, EXIT_TROUBLE, 0, Note::TmpCreateGnu, true),
    // 槽 6：stdout 残缺，退 2，已吐 40 行，落降级锚。
    Claim::new(false, Stage::Output, EXIT_TROUBLE, 40, Note::StdoutGnu, true),
    // 槽 7：busybox 第 2 件写挂，退 1，落降级锚。
    Claim::new(false, Stage::Spill, EXIT_QUIET_FAIL, 0, Note::SpillWriteBusy, true),
    // 槽 8：`-C` 乱序，静默退 1。
    Claim::new(false, Stage::Input, EXIT_QUIET_FAIL, 0, Note::DisorderedQuiet, false),
    // 死 1 estate-swap：全内存跑成功退 0，记录却写成"第 3 件写挂的降级失败"。
    Claim::new(false, Stage::Spill, EXIT_TROUBLE, 0, Note::SpillWriteGnu, true),
    // 死 2 stage-skip：件名都开出来了、写挂在第 5 闸，却说成第 2 闸开不出名字。
    Claim::new(false, Stage::Tmpdir, EXIT_TROUBLE, 0, Note::SpillWriteGnu, true),
    // 死 3 code-swap：GNU 现场的 2 记成 busybox 档的 1。
    Claim::new(false, Stage::Spill, EXIT_QUIET_FAIL, 0, Note::SpillWriteGnu, true),
    // 死 4 value-off：归并压根没起跑，却把 100 行输入报成已外发。
    Claim::new(false, Stage::Spill, EXIT_TROUBLE, 100, Note::SpillWriteGnu, true),
    // 死 5 note-swap：写出失败（件名主语）认成创建失败（目录主语）。
    Claim::new(false, Stage::Spill, EXIT_TROUBLE, 0, Note::TmpCreateGnu, true),
    // 死 6 default-bucket：目录开不出名字的 ENOSPC 是本卡认可的降级现场，却不落锚。
    Claim::new(false, Stage::Tmpdir, EXIT_TROUBLE, 0, Note::TmpCreateGnu, false),
];

/// 14 槽的标签（后 6 槽一槽只坏一维，依次挂六维）。
pub const WANT_TAGS: [&str; CORPUS] = [
    "in-memory-ok",
    "merge-only-ok",
    "spilled-ok",
    "spill-write-enospc",
    "tmp-create-enospc",
    "stdout-partial",
    "busybox-spill-enospc",
    "check-quiet-disorder",
    "estate-swap",
    "stage-skip",
    "code-swap",
    "value-off",
    "note-swap",
    "default-bucket",
];

/// 期望表的第二形态：落点阶段（与 CLAIMS 交叉校验，任一处笔误都要露出来）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Settle,
    Stage::Settle,
    Stage::Settle,
    Stage::Spill,
    Stage::Tmpdir,
    Stage::Output,
    Stage::Spill,
    Stage::Input,
    Stage::Spill,
    Stage::Tmpdir,
    Stage::Spill,
    Stage::Spill,
    Stage::Spill,
    Stage::Tmpdir,
];

/// 期望表的第二形态：申报的退出码。
pub const WANT_CODE: [u32; CORPUS] = [
    EXIT_OK,
    EXIT_OK,
    EXIT_OK,
    EXIT_TROUBLE,
    EXIT_TROUBLE,
    EXIT_TROUBLE,
    EXIT_QUIET_FAIL,
    EXIT_QUIET_FAIL,
    EXIT_TROUBLE,
    EXIT_TROUBLE,
    EXIT_QUIET_FAIL,
    EXIT_TROUBLE,
    EXIT_TROUBLE,
    EXIT_TROUBLE,
];

/// 期望表的第三形态：期望命中的判定维（0 = Ok，1..=6 = Fault 秩）。
pub const WANT_NDX: [u8; CORPUS] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6];

/// 期望表的第二形态：文案档位。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::DoneFull,
    Note::DoneMerge,
    Note::DoneSpilled,
    Note::SpillWriteGnu,
    Note::TmpCreateGnu,
    Note::StdoutGnu,
    Note::SpillWriteBusy,
    Note::DisorderedQuiet,
    Note::SpillWriteGnu,
    Note::SpillWriteGnu,
    Note::SpillWriteGnu,
    Note::SpillWriteGnu,
    Note::TmpCreateGnu,
    Note::TmpCreateGnu,
];

/// 期望表的第二形态：降级锚位。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, true, true, true, true, false, true, true, true, true, true, false,
];

/// 期望表的第二形态：申报成败位（等价于 exit==0）。
pub const WANT_OK: [bool; CORPUS] = [
    true, true, true, false, false, false, false, false, false, false, false, false, false, false,
];

/// 期望表的第二形态：申报的已吐出行数。
pub const WANT_VALUE: [u32; CORPUS] = [
    8, 12, 100, 0, 0, 40, 0, 0, 0, 0, 0, 100, 0, 0,
];

/// 事实面第二形态（与 `define` 独立列出，用来同时复核 define 与 CLAIMS）：落点阶段。
pub const ACT_STAGE: [Stage; CORPUS] = [
    Stage::Settle,
    Stage::Settle,
    Stage::Settle,
    Stage::Spill,
    Stage::Tmpdir,
    Stage::Output,
    Stage::Spill,
    Stage::Input,
    Stage::Settle,
    Stage::Spill,
    Stage::Spill,
    Stage::Spill,
    Stage::Spill,
    Stage::Tmpdir,
];

/// 事实面第二形态：本卡算出的应然退出码。
pub const ACT_CODE: [u32; CORPUS] = [
    EXIT_OK,
    EXIT_OK,
    EXIT_OK,
    EXIT_TROUBLE,
    EXIT_TROUBLE,
    EXIT_TROUBLE,
    EXIT_QUIET_FAIL,
    EXIT_QUIET_FAIL,
    EXIT_OK,
    EXIT_TROUBLE,
    EXIT_TROUBLE,
    EXIT_TROUBLE,
    EXIT_TROUBLE,
    EXIT_TROUBLE,
];

/// 事实面第二形态：应然已吐出行数。
pub const ACT_VALUE: [u32; CORPUS] = [
    8, 12, 100, 0, 0, 40, 0, 0, 8, 0, 0, 0, 0, 0,
];

/// 事实面第二形态：文案档位。
pub const ACT_NOTE: [Note; CORPUS] = [
    Note::DoneFull,
    Note::DoneMerge,
    Note::DoneSpilled,
    Note::SpillWriteGnu,
    Note::TmpCreateGnu,
    Note::StdoutGnu,
    Note::SpillWriteBusy,
    Note::DisorderedQuiet,
    Note::DoneFull,
    Note::SpillWriteGnu,
    Note::SpillWriteGnu,
    Note::SpillWriteGnu,
    Note::SpillWriteGnu,
    Note::TmpCreateGnu,
];

/// 事实面第二形态：降级锚位（14 槽里 6 槽该落锚，全在空间族三落点上）。
pub const ACT_DEFAULT: [bool; CORPUS] = [
    false, false, false, true, true, true, true, false, false, true, true, true, true, true,
];

/// 事实面第二形态：成败位。
pub const ACT_OK: [bool; CORPUS] = [
    true, true, true, false, false, false, false, false, true, false, false, false, false, false,
];

/// 事实面第三形态：现场落点（`emitted` 可否非零由此决定）。
pub const ACT_LOCUS: [Locus; CORPUS] = [
    Locus::Nowhere,
    Locus::Nowhere,
    Locus::Nowhere,
    Locus::SpillWrite,
    Locus::TmpCreate,
    Locus::OutWrite,
    Locus::SpillWrite,
    Locus::Nowhere,
    Locus::Nowhere,
    Locus::SpillWrite,
    Locus::SpillWrite,
    Locus::SpillWrite,
    Locus::SpillWrite,
    Locus::TmpCreate,
];

/// 事实面第三形态：输入行数（`emitted` 的上界）。
pub const ACT_LINES: [u32; CORPUS] = [
    8, 12, 100, 100, 100, 100, 60, 6, 8, 100, 100, 100, 100, 100,
];

/// 事实面第三形态：实际开出的临时件数。
pub const ACT_MADE: [u32; CORPUS] = [0, 0, 3, 3, 0, 3, 2, 0, 0, 3, 3, 3, 3, 0];

/// 事实面第三形态：几何上应当落的件数（尾段留内存，不计入）。
pub const ACT_SPILLS: [u32; CORPUS] = [0, 0, 3, 3, 3, 3, 2, 0, 0, 3, 3, 3, 3, 3];

/// 事实面第三形态：退出时残留的临时件数（14 槽全部回收干净）。
pub const ACT_LEFT: [u32; CORPUS] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

/// 事实面第三形态：档位归类。
pub const ACT_TIER: [Tier; CORPUS] = [
    Tier::InMem,
    Tier::InMem,
    Tier::Spilled,
    Tier::Degraded,
    Tier::Degraded,
    Tier::Degraded,
    Tier::Degraded,
    Tier::Refused,
    Tier::InMem,
    Tier::Degraded,
    Tier::Degraded,
    Tier::Degraded,
    Tier::Degraded,
    Tier::Degraded,
];

/// 死槽 → 它所复用的活槽（语料纪律：后 6 槽的事实面整槽照抄前 8 槽的某一行）。
pub const DEAD_FROM: [usize; REJECT] = [0usize, 3usize, 3usize, 3usize, 3usize, 4usize];

/// 活槽申报是否成功（与 `WANT_OK` 同形，供矩阵复核）。
pub const LIVE_OK: [bool; ACCEPT] = [true, true, true, false, false, false, false, false];

/// 由现场算出的应然结论（`define` 的公开别名，报表与测试共用）。
pub fn truth(i: usize) -> Outcome {
    match SHAPES.get(i) {
        Some(s) => define(s),
        None => Outcome::new(Stage::Args, EXIT_UNJUDGED, 0, Note::PlanBroken, false),
    }
}

// ---------------------------------------------------------------------------
// 档位归类
// ---------------------------------------------------------------------------

impl Tier {
    /// 由事实面结论归入四档之一。
    ///
    /// 归序固定：先看锚位（能落锚就是降级），再看成败（成功里分全内存与落盘），
    /// 剩下的全部进拒收档——谎报、异因、红线、越界都在这儿，不再细分。
    pub fn of(o: &Outcome) -> Tier {
        if o.default {
            return Tier::Degraded;
        }
        if o.ok() {
            if o.note == Note::DoneSpilled {
                return Tier::Spilled;
            }
            return Tier::InMem;
        }
        Tier::Refused
    }

    /// 第 i 槽的档位（按事实面）。
    pub fn of_slot(i: usize) -> Option<Tier> {
        match SHAPES.get(i) {
            Some(s) => Some(Tier::of(&define(s))),
            None => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 现场场景表（语料十四槽之外的判据现场，一槽只测一维时用不上它们）
// ---------------------------------------------------------------------------

/// 场景个数。
pub const SCENE_COUNT: usize = 11usize;

/// 场景名（与 `SCENES` 一一对位）。
pub const SCENE_NAMES: [&str; SCENE_COUNT] = [
    "memory-lie",
    "dir-not-usable",
    "part-gone-at-merge",
    "mem-exhausted",
    "edquot-spill",
    "input-damaged",
    "temp-leak",
    "rename-at-merge",
    "busybox-t-flag",
    "made-exceeds-plan",
    "check-mode-emitted",
];

/// 场 1 `memory-lie`：8 行输入装得下（不建件），却有指向临时件的写出失败申报。
const SCENE_MEMORY_LIE: Shape = Shape {
    dir: TmpDir::EnvFullDir,
    lines: 8,
    room: 64,
    locus: Locus::SpillWrite,
    errno_seen: ERR_ENOSPC,
    code_seen: EXIT_TROUBLE,
    ..Shape::BASE
};

/// 场 2 `dir-not-usable`：`TMPDIR` 指到的不是目录（ENOTDIR），创建闸就断了。
const SCENE_DIR_BAD: Shape = Shape {
    dir: TmpDir::EnvBad,
    lines: 100,
    room: 30,
    locus: Locus::TmpCreate,
    errno_seen: ERR_ENOTDIR,
    code_seen: EXIT_TROUBLE,
    ..Shape::BASE
};

/// 场 3 `part-gone-at-merge`：三段都写成了，归并回读第 2 件时它已被回收逻辑删掉。
const SCENE_PART_GONE: Shape = Shape {
    dir: TmpDir::EnvSet,
    lines: 100,
    room: 30,
    made: 3,
    fail_at: 2,
    locus: Locus::MergeRead,
    errno_seen: ERR_ENOENT,
    code_seen: EXIT_TROUBLE,
    ..Shape::BASE
};

/// 场 4 `mem-exhausted`：归并的合并缓冲 malloc 失败，`sort: memory exhausted`。
const SCENE_MEM_EXHAUSTED: Shape = Shape {
    dir: TmpDir::EnvSet,
    lines: 100,
    room: 30,
    made: 3,
    locus: Locus::Memory,
    errno_seen: ERR_ENOMEM,
    code_seen: EXIT_TROUBLE,
    ..Shape::BASE
};

/// 场 5 `edquot-spill`：同一句式后面挂的是 `Disc quota exceeded`，本卡不落锚。
const SCENE_EDQUOT_SPILL: Shape = Shape {
    dir: TmpDir::EnvFullDir,
    lines: 100,
    room: 30,
    made: 3,
    fail_at: 3,
    locus: Locus::SpillWrite,
    errno_seen: ERR_EDQUOT,
    code_seen: EXIT_TROUBLE,
    ..Shape::BASE
};

/// 场 6 `input-damaged`：第 3 件写挂的现场，输入件却只剩一半字节（红线）。
const SCENE_INPUT_DAMAGED: Shape = Shape {
    dir: TmpDir::EnvSet,
    lines: 100,
    room: 30,
    made: 3,
    fail_at: 3,
    locus: Locus::SpillWrite,
    errno_seen: ERR_ENOSPC,
    code_seen: EXIT_TROUBLE,
    inputs_ok: false,
    ..Shape::BASE
};

/// 场 7 `temp-leak`：排序成功，退出时临时目录里还剩 2 个 chunk 文件。
const SCENE_TEMP_LEAK: Shape = Shape {
    dir: TmpDir::EnvSet,
    lines: 100,
    room: 30,
    made: 3,
    emitted: 100,
    left: 2,
    ..Shape::BASE
};

/// 场 8 `rename-at-merge`：申报"归并阶段 rename 报 28"——`sort` 没有换名动作。
const SCENE_RENAME_AT_MERGE: Shape = Shape {
    dir: TmpDir::EnvSet,
    lines: 100,
    room: 30,
    made: 3,
    fail_at: 3,
    locus: Locus::MergeRead,
    errno_seen: ERR_ENOSPC,
    code_seen: EXIT_TROUBLE,
    ..Shape::BASE
};

/// 场 9 `busybox-t-flag`：busybox applet 报了 `-T`，家族越界，落命令行闸。
const SCENE_BUSY_TFLAG: Shape = Shape {
    flavor: Flavor::Busybox,
    dir: TmpDir::Explicit,
    opts: OPT_TDIR,
    lines: 100,
    room: 30,
    made: 3,
    fail_at: 3,
    locus: Locus::SpillWrite,
    errno_seen: ERR_ENOSPC,
    code_seen: EXIT_QUIET_FAIL,
    ..Shape::BASE
};

/// 场 10 `made-exceeds-plan`：几何只许 3 件，现场说开出了 4 件（自相矛盾）。
const SCENE_MADE_EXCEEDS: Shape = Shape {
    dir: TmpDir::EnvSet,
    lines: 100,
    room: 30,
    made: 4,
    fail_at: 4,
    locus: Locus::SpillWrite,
    errno_seen: ERR_ENOSPC,
    code_seen: EXIT_TROUBLE,
    ..Shape::BASE
};

/// 场 11 `check-mode-emitted`：`-c` 从不写输出，现场却记了 6 行外发。
const SCENE_CHECK_EMITTED: Shape = Shape {
    opts: OPT_CHECK,
    lines: 6,
    room: 64,
    emitted: 6,
    ordered: true,
    ..Shape::BASE
};

/// 场景表：(现场, 本卡应然结论)。11 场全部由 `define` 独立复算。
pub const SCENES: [(Shape, Outcome); SCENE_COUNT] = [
    (
        SCENE_MEMORY_LIE,
        Outcome::new(Stage::Buffer, EXIT_UNJUDGED, 0, Note::MemoryLie, false),
    ),
    (
        SCENE_DIR_BAD,
        Outcome::new(Stage::Tmpdir, EXIT_TROUBLE, 0, Note::TmpdirBad, false),
    ),
    (
        SCENE_PART_GONE,
        Outcome::new(Stage::Merge, EXIT_TROUBLE, 0, Note::PartGone, false),
    ),
    (
        SCENE_MEM_EXHAUSTED,
        Outcome::new(Stage::Merge, EXIT_TROUBLE, 0, Note::MemExhausted, false),
    ),
    (
        SCENE_EDQUOT_SPILL,
        Outcome::new(Stage::Spill, EXIT_TROUBLE, 0, Note::ForeignErr, false),
    ),
    (
        SCENE_INPUT_DAMAGED,
        Outcome::new(Stage::Cleanup, EXIT_UNJUDGED, 0, Note::InputDamaged, false),
    ),
    (
        SCENE_TEMP_LEAK,
        Outcome::new(Stage::Cleanup, EXIT_UNJUDGED, 100, Note::TempLeak, false),
    ),
    (
        SCENE_RENAME_AT_MERGE,
        Outcome::new(Stage::Merge, EXIT_UNJUDGED, 0, Note::LocusLie, false),
    ),
    (
        SCENE_BUSY_TFLAG,
        Outcome::new(Stage::Args, EXIT_QUIET_FAIL, 0, Note::Usage, false),
    ),
    (
        SCENE_MADE_EXCEEDS,
        Outcome::new(Stage::Buffer, EXIT_UNJUDGED, 0, Note::PlanBroken, false),
    ),
    (
        SCENE_CHECK_EMITTED,
        Outcome::new(Stage::Buffer, EXIT_UNJUDGED, 6, Note::PlanBroken, false),
    ),
];

/// 第 i 场的现场与期望。
pub fn scene_at(i: usize) -> Option<(Shape, Outcome)> {
    match SCENES.get(i) {
        Some((s, o)) => Some((*s, *o)),
        None => None,
    }
}

/// 第 i 场的名字。
pub fn scene_name(i: usize) -> &'static str {
    match SCENE_NAMES.get(i) {
        Some(v) => *v,
        None => "越界",
    }
}

/// 场景表逐场复核：`define` 的结论必须与期望逐维相等。
pub fn scenes_all_land() -> bool {
    let mut i: usize = 0;
    while i < SCENE_COUNT {
        let (sh, want) = match scene_at(i) {
            Some(v) => v,
            None => return false,
        };
        if define(&sh) != want {
            return false;
        }
        i += 1;
    }
    true
}

/// 场景里没有任何一格可以落降级锚（十一场全是拒收/异因/红线）。
pub fn scenes_never_anchor() -> bool {
    let mut i: usize = 0;
    while i < SCENE_COUNT {
        let want = match SCENES.get(i) {
            Some((_, o)) => *o,
            None => return false,
        };
        if want.default {
            return false;
        }
        i += 1;
    }
    true
}

/// 落点闸的申报复核（本卡的排他判据之一，返回错误码而不是布尔）。
pub fn locus_gate(s: &Shape) -> Result<Stage, u32> {
    if s.family_overreach() {
        return Err(ST_ECOUNT);
    }
    if let Err(e) = s.honesty_gate() {
        return Err(e);
    }
    if s.geometry_broken() {
        return Err(ST_EBAL);
    }
    if let Err(e) = s.input_guard() {
        return Err(e);
    }
    if let Err(e) = s.cleanup_gate() {
        return Err(e);
    }
    Ok(s.locus.stage())
}

// ---------------------------------------------------------------------------
// 缺陷维与账本格
// ---------------------------------------------------------------------------

/// 六个缺陷维（不含 Ok），次序即 judge 的判定次序。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

/// 六个缺陷维对应的错误码（互不相同，与 `REJECT_FAULTS` 一一对位）。
pub const REJECT_CODES: [u32; REJECT] = [
    ST_ESTATE,
    ST_ESTAGE,
    ST_ECODE,
    ST_EEMIT,
    ST_ENOTE,
    ST_EANCH,
];

/// 账本的 8 个正数格：前 7 格是统计数，第 8 格是红档率 929。
///
/// 七个统计数全部取自本卡的身份常量，`VALS_CHECK` 用字面量再写一遍，
/// 任何一格漂移都会在 `audit` 里露出两处不同的数字。
pub const VALS: [u32; ACCEPT] = [
    CORPUS as u32,
    ACCEPT as u32,
    REJECT as u32,
    STAGE_COUNT,
    LOCUS_COUNT as u32,
    TIER_COUNT as u32,
    FLAVOR_COUNT as u32,
    RED_RATE,
];

/// 统计格的字面期望（第二形态，交叉校验 `VALS` 的笔误）。
pub const VALS_CHECK: [u32; ACCEPT] = [14, 8, 6, 9, 6, 4, 2, 929];

/// 统计格语义（本卡的身份数，一格一句话）。
pub fn stat_meaning(i: usize) -> &'static str {
    match i {
        0 => "十四个语料槽：8 活 + 6 死",
        1 => "八个活槽：申报与事实逐维相等",
        2 => "六个死槽：一槽只坏一维",
        3 => "九道闸：命令行到收尾的纵线宽度",
        4 => "六个落点：目录/件/回读/目的地/内存/无处",
        5 => "四个档位：全内存/落盘/降级/拒收",
        6 => "两个家族阶梯：GNU 退 2、busybox 退 1",
        7 => "红档率 929：14 格中 13 的千分率",
        _ => "越界",
    }
}

/// 第 i 槽的标签。
pub fn tag_at(i: usize) -> &'static str {
    match WANT_TAGS.get(i) {
        Some(v) => *v,
        None => "越界",
    }
}

/// 第 i 槽期望命中的判定维。
pub fn want_fault(i: usize) -> Fault {
    let ndx = match WANT_NDX.get(i) {
        Some(v) => *v,
        None => return Fault::Ok,
    };
    if ndx == 0 {
        return Fault::Ok;
    }
    match Fault::ALL.get(ndx as usize) {
        Some(v) => *v,
        None => Fault::Ok,
    }
}

/// 第 i 槽的案例（现场 + 申报）。
pub fn case_at(i: usize) -> Option<Case> {
    let shape = match SHAPES.get(i) {
        Some(v) => *v,
        None => return None,
    };
    let claim = match CLAIMS.get(i) {
        Some(v) => *v,
        None => return None,
    };
    Some(Case::new(shape, claim))
}

/// 槽数（越界检查用）。
pub fn slot_count() -> usize {
    CORPUS
}

/// 第 i 槽的判定，带一个"越界注入"位（`run_with` 用）。
fn fault_at(c: &Case, i: usize, over: Option<(usize, Fault)>) -> Result<Fault, u32> {
    if i >= CORPUS {
        return Err(ST_ENOBS);
    }
    let base = judge(c);
    match over {
        Some((slot, f)) => {
            if slot == i {
                Ok(f)
            } else {
                Ok(base)
            }
        }
        None => Ok(base),
    }
}

/// 十四槽的判定结果。
pub fn verdicts() -> [Fault; CORPUS] {
    let mut out = [Fault::Ok; CORPUS];
    let mut i: usize = 0;
    while i < CORPUS {
        if let Some(c) = case_at(i) {
            if let Some(slot) = out.get_mut(i) {
                *slot = judge(&c);
            }
        }
        i += 1;
    }
    out
}

/// 全部申报逐维等于期望表（活槽合、死槽各挂一维）。
pub fn all_claims_land() -> bool {
    let mut i: usize = 0;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(v) => v,
            None => return false,
        };
        if judge(&c) != want_fault(i) {
            return false;
        }
        i += 1;
    }
    true
}

/// 第 i 槽的事实面期望（闸/码/行数/文案/锚位/成败）。
pub fn expected(i: usize) -> Option<(Stage, u32, u32, Note, bool, bool)> {
    let stage = match ACT_STAGE.get(i) {
        Some(v) => *v,
        None => return None,
    };
    let code = match ACT_CODE.get(i) {
        Some(v) => *v,
        None => return None,
    };
    let value = match ACT_VALUE.get(i) {
        Some(v) => *v,
        None => return None,
    };
    let note = match ACT_NOTE.get(i) {
        Some(v) => *v,
        None => return None,
    };
    let default = match ACT_DEFAULT.get(i) {
        Some(v) => *v,
        None => return None,
    };
    let ok = match ACT_OK.get(i) {
        Some(v) => *v,
        None => return None,
    };
    Some((stage, code, value, note, default, ok))
}

/// 期望表与 `define` 逐维相等（第二形态不许与算法漂移）。
pub fn act_matches_define() -> bool {
    let mut i: usize = 0;
    while i < CORPUS {
        let (stage, code, value, note, default, ok) = match expected(i) {
            Some(v) => v,
            None => return false,
        };
        let o = truth(i);
        if o.stage != stage || o.code != code || o.value != value || o.note != note {
            return false;
        }
        if o.default != default || o.ok() != ok {
            return false;
        }
        i += 1;
    }
    true
}

/// 第 i 槽的档位覆盖（矩阵按档位分桶时用）。
pub fn cover(i: usize) -> Option<Tier> {
    match ACT_TIER.get(i) {
        Some(v) => Some(*v),
        None => None,
    }
}

/// 四档是否都有活槽命中（语料覆盖纪律）。
pub fn tiers_all_covered() -> bool {
    let mut hit = [false; TIER_COUNT];
    let mut i: usize = 0;
    while i < ACCEPT {
        if let Some(t) = cover(i) {
            if let Some(slot) = hit.get_mut(t.index()) {
                *slot = true;
            }
        }
        i += 1;
    }
    let mut k: usize = 0;
    while k < TIER_COUNT {
        match hit.get(k) {
            Some(true) => {}
            _ => return false,
        }
        k += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 矩阵
// ---------------------------------------------------------------------------

/// 账本统计格的类型码（与九道闸的操作码并置，互不重叠）。
pub const OP_STAT: u8 = 0x0a;

/// 一档的账：投了多少槽、命中多少槽。
///
/// 也可以压根没有槽——空行永远不算完美（缺档本身就是缺陷，不是满分）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// 所属档位。
    pub tier: Tier,
    /// 该档的槽数。
    pub total: u32,
    /// 该档命中期望结论的槽数。
    pub good: u32,
}

impl Row {
    /// 建一行。
    pub fn of(tier: Tier) -> Row {
        Row {
            tier,
            total: 0,
            good: 0,
        }
    }

    /// 是否满命中（空行判不完美）。
    pub fn perfect(&self) -> bool {
        self.total > 0 && self.total == self.good
    }

    /// 是否空行。
    pub fn empty(&self) -> bool {
        self.total == 0
    }

    /// 命中缺口。
    pub fn miss(&self) -> u32 {
        self.total.saturating_sub(self.good)
    }

    /// 渲染一行。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(self.tier.slug());
        s.push(':');
        s.push_str(dec_text(self.good).as_str());
        s.push('/');
        s.push_str(dec_text(self.total).as_str());
        s
    }
}

/// 四档矩阵。`total` 是投槽数、`synced` 是登记数、`aligned()` 是四档行求和——
/// 三者本应相等，任何一格漂移都要被 `covered()` 抓到（所以它们各自独立累计）。
#[derive(Clone, Copy, Debug)]
pub struct Matrix {
    /// 四档行。
    pub rows: [Row; TIER_COUNT],
    /// 投槽总数。
    pub total: u32,
    /// 登记总数（与 total 同路累计，但走的是另一条计数）。
    pub synced: u32,
    /// 命中总数。
    pub matched: u32,
}

impl Matrix {
    /// 空矩阵：四档就位、计数全 0。
    pub fn new() -> Matrix {
        Matrix {
            rows: [
                Row::of(Tier::InMem),
                Row::of(Tier::Spilled),
                Row::of(Tier::Degraded),
                Row::of(Tier::Refused),
            ],
            total: 0,
            synced: 0,
            matched: 0,
        }
    }

    /// 落一槽事实。
    pub fn push_actual(&mut self, tier: Tier, hit: bool) {
        self.total = self.total.saturating_add(1);
        self.synced = self.synced.saturating_add(1);
        if hit {
            self.matched = self.matched.saturating_add(1);
        }
        let mut k: usize = 0;
        while k < TIER_COUNT {
            if let Some(r) = self.rows.get_mut(k) {
                if r.tier == tier {
                    r.total = r.total.saturating_add(1);
                    if hit {
                        r.good = r.good.saturating_add(1);
                    }
                }
            }
            k += 1;
        }
    }

    /// 只加投槽数、不进任何档——用来造 `total != aligned()` 这种账目不平的形态。
    pub fn desync(&mut self) {
        self.total = self.total.saturating_add(1);
    }

    /// 四档求和（独立于 total 的第二条口径）。
    pub fn aligned(&self) -> u32 {
        let mut sum: u32 = 0;
        let mut k: usize = 0;
        while k < TIER_COUNT {
            if let Some(r) = self.rows.get(k) {
                sum = sum.saturating_add(r.total);
            }
            k += 1;
        }
        sum
    }

    /// 两条口径是否闭合。
    pub fn covered(&self) -> bool {
        self.total == self.aligned() && self.aligned() == self.synced
    }

    /// 满命中的非空行数。
    pub fn perfect_rows(&self) -> u32 {
        let mut n: u32 = 0;
        let mut k: usize = 0;
        while k < TIER_COUNT {
            if let Some(r) = self.rows.get(k) {
                if r.perfect() {
                    n = n.saturating_add(1);
                }
            }
            k += 1;
        }
        n
    }

    /// 非空行数。
    pub fn occupied(&self) -> u32 {
        let mut n: u32 = 0;
        let mut k: usize = 0;
        while k < TIER_COUNT {
            if let Some(r) = self.rows.get(k) {
                if !r.empty() {
                    n = n.saturating_add(1);
                }
            }
            k += 1;
        }
        n
    }

    /// 取某档的行。
    pub fn row(&self, tier: Tier) -> Option<Row> {
        let mut k: usize = 0;
        while k < TIER_COUNT {
            if let Some(r) = self.rows.get(k) {
                if r.tier == tier {
                    return Some(*r);
                }
            }
            k += 1;
        }
        None
    }

    /// 该档的槽数（缺行按 0）。
    pub fn count_of(&self, tier: Tier) -> u32 {
        match self.row(tier) {
            Some(r) => r.total,
            None => 0,
        }
    }

    /// 该档命中的槽数。
    pub fn good_of(&self, tier: Tier) -> u32 {
        match self.row(tier) {
            Some(r) => r.good,
            None => 0,
        }
    }

    /// 由全语料建矩阵：命中定义为"判出的结论等于该槽期望结论"。
    pub fn from_cases() -> Matrix {
        let mut m = Matrix::new();
        let mut i: usize = 0;
        while i < CORPUS {
            let c = match case_at(i) {
                Some(v) => v,
                None => break,
            };
            let tier = Tier::of(&define(&c.shape));
            let hit = judge(&c) == want_fault(i);
            m.push_actual(tier, hit);
            i += 1;
        }
        m
    }

    /// 渲染四档。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut k: usize = 0;
        while k < TIER_COUNT {
            if let Some(r) = self.rows.get(k) {
                if k > 0 {
                    sep(&mut s);
                }
                s.push_str(r.render().as_str());
            }
            k += 1;
        }
        sep(&mut s);
        kv(&mut s, "aligned", self.aligned());
        sep(&mut s);
        kv(&mut s, "total", self.total);
        s
    }
}

// ---------------------------------------------------------------------------
// 报告
// ---------------------------------------------------------------------------

/// 全语料运行的报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 语料槽数。
    pub total: usize,
    /// 活槽数。
    pub accept: usize,
    /// 死槽数。
    pub reject: usize,
    /// 全内存档槽数。
    pub in_mem: usize,
    /// 落盘成功档槽数。
    pub spilled: usize,
    /// 降级可锚档槽数。
    pub degraded: usize,
    /// 拒收档槽数。
    pub refused: usize,
    /// 命中期望结论的槽数。
    pub matched: usize,
}

impl Report {
    /// 全绿运行的逐字渲染（按手算钉死；越界一红即变串）。
    pub const RENDER_OK: &'static str = "sort2s total=14 accept=8 reject=6 in-memory=3 spilled=1 degraded=9 refused=1 matched=14 rate=1000 verdict=green";

    /// 千分率：`(matched*1000 + total/2) / total`。14 格中 13 → 929（红档率标定）。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        let num = (self.matched as u32)
            .saturating_mul(1000)
            .saturating_add((self.total / 2) as u32);
        num / (self.total as u32)
    }

    /// 四档求和。
    pub fn tier_sum(&self) -> usize {
        self.in_mem
            .saturating_add(self.spilled)
            .saturating_add(self.degraded)
            .saturating_add(self.refused)
    }

    /// 是否全绿。
    pub fn green(&self) -> bool {
        self.gate().is_ok()
    }

    /// 是否恰中 13/14（红档率标定态）——一红即 929，这一格把 929 与渲染路绑死。
    pub fn is_red_calibrated(&self) -> bool {
        self.total == CORPUS && self.matched + 1 == CORPUS && self.rate() == RED_RATE
    }

    /// 出口闸门：逐项校验语料宽度、活死配比、四档闭合、命中率、千分率。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total != CORPUS {
            return Err(ST_ESLOT);
        }
        if self.accept.saturating_add(self.reject) != self.total {
            return Err(ST_ECOUNT);
        }
        if self.tier_sum() != self.total {
            return Err(ST_EBAL);
        }
        if self.matched != self.total {
            return Err(ST_ESTATE);
        }
        if self.rate() != RATE_FULL {
            return Err(ST_ENOBS);
        }
        Ok(())
    }

    /// 逐字渲染（`RENDER_OK` 钉的就是这一串）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("sort2s");
        sep(&mut s);
        kv(&mut s, "total", self.total as u32);
        sep(&mut s);
        kv(&mut s, "accept", self.accept as u32);
        sep(&mut s);
        kv(&mut s, "reject", self.reject as u32);
        sep(&mut s);
        kv(&mut s, "in-memory", self.in_mem as u32);
        sep(&mut s);
        kv(&mut s, "spilled", self.spilled as u32);
        sep(&mut s);
        kv(&mut s, "degraded", self.degraded as u32);
        sep(&mut s);
        kv(&mut s, "refused", self.refused as u32);
        sep(&mut s);
        kv(&mut s, "matched", self.matched as u32);
        sep(&mut s);
        kv(&mut s, "rate", self.rate());
        sep(&mut s);
        s.push_str("verdict=");
        s.push_str(if self.green() { "green" } else { "red" });
        s
    }

    /// 取某一档的槽数（按档位枚举）。
    pub fn tier_count(&self, tier: Tier) -> usize {
        match tier {
            Tier::InMem => self.in_mem,
            Tier::Spilled => self.spilled,
            Tier::Degraded => self.degraded,
            Tier::Refused => self.refused,
        }
    }
}

/// 全绿运行的逐字渲染（出口别名）。
pub const RENDER_OK: &str = Report::RENDER_OK;

/// 跑一遍全语料。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 带注入的运行：`over` 点中某槽时强行改判其结论，槽号越界 → `ST_ESLOT`。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((slot, _)) = over {
        if slot >= CORPUS {
            return Err(ST_ESLOT);
        }
    }
    let mut total: usize = 0;
    let mut matched: usize = 0;
    let mut in_mem: usize = 0;
    let mut spilled: usize = 0;
    let mut degraded: usize = 0;
    let mut refused: usize = 0;
    let mut i: usize = 0;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(v) => v,
            None => return Err(ST_ESLOT),
        };
        let got = match fault_at(&c, i, over) {
            Ok(f) => f,
            Err(e) => return Err(e),
        };
        let tier = Tier::of(&define(&c.shape));
        total = total.saturating_add(1);
        if got == want_fault(i) {
            matched = matched.saturating_add(1);
        }
        match tier {
            Tier::InMem => in_mem = in_mem.saturating_add(1),
            Tier::Spilled => spilled = spilled.saturating_add(1),
            Tier::Degraded => degraded = degraded.saturating_add(1),
            Tier::Refused => refused = refused.saturating_add(1),
        }
        i += 1;
    }
    Ok(Report {
        total,
        accept: ACCEPT,
        reject: REJECT,
        in_mem,
        spilled,
        degraded,
        refused,
        matched,
    })
}

// ---------------------------------------------------------------------------
// 九闸纵线（单调不倒退）
// ---------------------------------------------------------------------------

/// 阶段闸门。规则：只允许前进到**下一道**闸；倒退 → `ST_ESTAGE`，跳档 → `ST_EBAL`，
/// 未走完纵线就封印 → `ST_ESTAGE`，封印之后一切动作 → `ST_ESTATE`。
/// 被拒的动作同样改指纹——这是"拒动也要留痕"的硬要求。
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    rank: u32,
    closed: bool,
    fp: u64,
    moves: u32,
    rejected: u32,
}

impl Gate {
    /// 新建：秩 0（未起跑，命令行都还没解析）。
    pub fn new() -> Gate {
        Gate {
            rank: 0,
            closed: false,
            fp: FNV_OFF,
            moves: 0,
            rejected: 0,
        }
    }

    /// 当前秩。
    pub fn rank(&self) -> u32 {
        self.rank
    }

    /// 当前阶段（秩 0 没有阶段）。
    pub fn stage(&self) -> Option<Stage> {
        Stage::from_rank(self.rank)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 尝试次数（含被拒的）。
    pub fn moves(&self) -> u32 {
        self.moves
    }

    /// 被拒次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 是否已封印。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 前进一道。只有 `rank + 1` 这一格被接受。
    pub fn step(&mut self, to: Stage) -> Result<(), u32> {
        self.moves = self.moves.saturating_add(1);
        self.fp = fold_word(self.fp, to.rank());
        if self.closed {
            self.rejected = self.rejected.saturating_add(1);
            self.fp = fold_name(self.fp, b"after-close");
            return Err(ST_ESTATE);
        }
        let want = self.rank.saturating_add(1);
        let r = to.rank();
        if r == want {
            self.rank = r;
            self.fp = fold_name(self.fp, b"advance");
            return Ok(());
        }
        self.rejected = self.rejected.saturating_add(1);
        self.fp = fold_name(self.fp, b"reject");
        if r < self.rank {
            return Err(ST_ESTAGE);
        }
        Err(ST_EBAL)
    }

    /// 封印：必须已经站到第九闸（收尾）。
    pub fn close(&mut self) -> Result<(), u32> {
        self.moves = self.moves.saturating_add(1);
        self.fp = fold_name(self.fp, b"close");
        if self.closed {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ST_ESTATE);
        }
        if self.rank != Stage::Settle.rank() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ST_ESTAGE);
        }
        self.closed = true;
        Ok(())
    }

    /// 走完整条纵线并封印（正例通道）。
    pub fn walk_and_seal(&mut self) -> Result<(), u32> {
        let mut k: usize = 0;
        while k < Stage::ALL.len() {
            let st = match Stage::ALL.get(k) {
                Some(v) => *v,
                None => return Err(ST_ESLOT),
            };
            match self.step(st) {
                Ok(()) => {}
                Err(e) => return Err(e),
            }
            k += 1;
        }
        self.close()
    }

    /// 按现场的闸序走一遍：结论落在第几闸，纵线就只许走到第几闸。
    pub fn walk_to(&mut self, goal: Stage) -> Result<(), u32> {
        if goal.rank() > STAGE_LAST_RANK {
            return Err(ST_ESLOT);
        }
        let mut k: usize = 0;
        while k < goal.rank() as usize {
            let st = match Stage::ALL.get(k) {
                Some(v) => *v,
                None => return Err(ST_ECOUNT),
            };
            match self.step(st) {
                Ok(()) => {}
                Err(e) => return Err(e),
            }
            k += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 一格账。`op` 决定这一格是统计格、红档格还是缺陷码格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位（0..14，必须严格等于登记时的账本长度）。
    pub slot: u8,
    /// 格类型：`OP_STAT` / `OP_RATE` / `OP_DEFECT`。
    pub op: u8,
    /// 成败位——与 `code == 0` 必须互推，否则就是被篡改的一格。
    pub ok: bool,
    /// 错误码（统计格与红档格恒 0；缺陷格挂 `REJECT_CODES`）。
    pub code: u32,
    /// 数值（统计格挂 `VALS`，红档格挂 929，缺陷格必须为 0）。
    pub value: u32,
}

/// 空白格（构造定容数组用）。
const ZERO_SAMPLE: Sample = Sample {
    slot: 0,
    op: 0,
    ok: true,
    code: 0,
    value: 0,
};

impl Sample {
    /// 统计格。
    pub fn stat(slot: usize, value: u32) -> Sample {
        Sample {
            slot: slot as u8,
            op: OP_STAT,
            ok: true,
            code: 0,
            value,
        }
    }

    /// 红档格（值恒 929）。
    pub fn rate(slot: usize) -> Sample {
        Sample {
            slot: slot as u8,
            op: OP_RATE,
            ok: true,
            code: 0,
            value: RED_RATE,
        }
    }

    /// 缺陷码格：值必须为 0，成败位必须为败。
    pub fn defect(slot: usize, code: u32) -> Sample {
        Sample {
            slot: slot as u8,
            op: OP_DEFECT,
            ok: false,
            code,
            value: 0,
        }
    }

    /// 格类型短名。
    pub fn kind(&self) -> &'static str {
        match self.op {
            OP_STAT => "stat",
            OP_RATE => "rate",
            OP_DEFECT => "defect",
            _ => "illegal",
        }
    }

    /// 折叠一格进指纹（五维全覆盖，次序固定）。
    pub fn fold(&self, h: u64) -> u64 {
        let x = fold_byte(h, self.slot);
        let x = fold_byte(x, self.op);
        let x = fold_flag(x, self.ok);
        let x = fold_word(x, self.code);
        fold_word(x, self.value)
    }

    /// 本格自洽性：op 合法且 ok 与 code 互推。
    pub fn self_consistent(&self) -> bool {
        if self.ok != (self.code == 0) {
            return false;
        }
        match self.op {
            OP_STAT => self.ok && self.code == 0 && self.value > 0,
            OP_RATE => self.ok && self.code == 0 && self.value == RED_RATE,
            OP_DEFECT => !self.ok && self.code != 0 && self.value == 0,
            _ => false,
        }
    }

    /// 渲染一格。
    pub fn render(&self) -> String {
        let mut s = String::new();
        kv(&mut s, "slot", self.slot as u32);
        sep(&mut s);
        s.push_str("kind=");
        s.push_str(self.kind());
        sep(&mut s);
        s.push_str("ok=");
        s.push_str(bool_text(self.ok).as_str());
        sep(&mut s);
        s.push_str("code=");
        s.push_str(code_name(self.code));
        sep(&mut s);
        kv(&mut s, "value", self.value);
        s
    }
}

/// 14 格账本。登记要求 `slot == len`（跳号/越界一律拒入账并计 `rejects`），
/// 篡改（自相矛盾的一格）拒入账并归 `ST_ECOUNT`；封印与冻结各自留指纹。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    items: [Sample; SAMPLES],
    len: usize,
    sealed: bool,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    /// 空账。
    pub fn new() -> Ledger {
        Ledger {
            items: [ZERO_SAMPLE; SAMPLES],
            len: 0,
            sealed: false,
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }

    /// 已入账格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 被拒登记次数（越界、跳号、篡改、冻结后动作全计入，显式不静默）。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否已封印。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 根指纹（未封印为 0）。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 取一格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        match self.items.get(i) {
            Some(s) => {
                if i < self.len {
                    Some(*s)
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// 登记一格。返回 Err 时**不入账**，只加 rejects。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ST_ESTATE);
        }
        if (s.slot as usize) != self.len {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ST_ESLOT);
        }
        if !s.self_consistent() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ST_ECOUNT);
        }
        if self.len >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ST_ESLOT);
        }
        match self.items.get_mut(self.len) {
            Some(slot) => *slot = s,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(ST_ESLOT);
            }
        }
        self.len = self.len.saturating_add(1);
        Ok(())
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < self.len {
            if let Some(s) = self.sample(i) {
                if s.ok {
                    n = n.saturating_add(1);
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

    /// 按格类型计数。
    pub fn count_op(&self, op: u8) -> usize {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < self.len {
            if let Some(s) = self.sample(i) {
                if s.op == op {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 逐格复核：前 7 格对 `VALS` 与 `VALS_CHECK`、第 8 格对 929、
    /// 后 6 格对 `REJECT_CODES` 且值必为 0。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(ST_ESLOT);
        }
        let mut i: usize = 0;
        while i < ACCEPT {
            let s = match self.sample(i) {
                Some(v) => v,
                None => return Err(ST_ESLOT),
            };
            let want_v = match VALS.get(i) {
                Some(v) => *v,
                None => return Err(ST_ESLOT),
            };
            let check_v = match VALS_CHECK.get(i) {
                Some(v) => *v,
                None => return Err(ST_ESLOT),
            };
            if want_v != check_v {
                return Err(ST_EBAL);
            }
            if s.slot as usize != i {
                return Err(ST_ECOUNT);
            }
            if i < STAT_CELLS {
                if s.op != OP_STAT || s.code != 0 || !s.ok || s.value != want_v {
                    return Err(ST_EEMIT);
                }
            } else if s.op != OP_RATE || s.code != 0 || s.value != RED_RATE {
                return Err(ST_EEMIT);
            }
            i += 1;
        }
        let mut j: usize = 0;
        while j < REJECT {
            let idx = ACCEPT.saturating_add(j);
            let s = match self.sample(idx) {
                Some(v) => v,
                None => return Err(ST_ESLOT),
            };
            let want_c = match REJECT_CODES.get(j) {
                Some(v) => *v,
                None => return Err(ST_ESLOT),
            };
            if s.op != OP_DEFECT || s.slot as usize != idx || s.code != want_c {
                return Err(ST_ECOUNT);
            }
            if s.value != 0 || s.ok {
                return Err(ST_EEMIT);
            }
            if code_offset(s.code) < 1 {
                return Err(ST_ESEAL);
            }
            j += 1;
        }
        Ok(())
    }

    /// 折叠全账（不写入状态，纯函数，`replay` 用它独立复算）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i: usize = 0;
        while i < self.len {
            if let Some(s) = self.sample(i) {
                h = s.fold(h);
            }
            i += 1;
        }
        fold_str(h, "sort-tmpfile-enospc")
    }

    /// 封印：折叠全账并落根指纹。
    pub fn seal(&mut self) -> u64 {
        self.root = self.fingerprint();
        self.sealed = true;
        self.root
    }

    /// 冻结：封印之后不再接受任何登记。
    pub fn freeze(&mut self) -> u64 {
        let r = self.seal();
        self.frozen = true;
        r
    }

    /// 重放：独立重建一遍账再比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.sealed {
            return Err(ST_ESTATE);
        }
        let mut rebuilt = Ledger::new();
        let mut i: usize = 0;
        while i < self.len {
            if let Some(s) = self.sample(i) {
                if rebuilt.register(s).is_err() {
                    return Err(ST_ECOUNT);
                }
            }
            i += 1;
        }
        if rebuilt.len != self.len {
            return Err(ST_ESLOT);
        }
        if rebuilt.fingerprint() != self.root {
            return Err(ST_ESEAL);
        }
        Ok(self.root)
    }

    /// 渲染整账（诊断用）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        kv(&mut s, "len", self.len as u32);
        sep(&mut s);
        kv(&mut s, "ok", self.ok_count() as u32);
        sep(&mut s);
        kv(&mut s, "err", self.err_count() as u32);
        sep(&mut s);
        kv(&mut s, "rejects", self.rejects);
        sep(&mut s);
        s.push_str("root=");
        s.push_str(hex_text(self.root as u32, 8).as_str());
        s
    }
}

/// 造 14 格账：7 统计格 + 1 红档格（929）+ 6 缺陷码格。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut i: usize = 0;
    while i < STAT_CELLS {
        let v = match VALS.get(i) {
            Some(x) => *x,
            None => 0,
        };
        match l.register(Sample::stat(i, v)) {
            Ok(()) => {}
            Err(_) => {}
        }
        i += 1;
    }
    match l.register(Sample::rate(STAT_CELLS)) {
        Ok(()) => {}
        Err(_) => {}
    }
    let mut j: usize = 0;
    while j < REJECT {
        let c = match REJECT_CODES.get(j) {
            Some(x) => *x,
            None => 0,
        };
        let idx = STAT_CELLS.saturating_add(1).saturating_add(j);
        match l.register(Sample::defect(idx, c)) {
            Ok(()) => {}
            Err(_) => {}
        }
        j += 1;
    }
    l
}

/// 造一格错值的账：第 4 统计格塞进 999，`audit` 必须以 `ST_EEMIT` 抓到。
pub fn broken_value_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut i: usize = 0;
    while i < STAT_CELLS {
        let v = match VALS.get(i) {
            Some(x) => *x,
            None => 0,
        };
        let patched = if i == 3 { 999 } else { v };
        match l.register(Sample::stat(i, patched)) {
            Ok(()) => {}
            Err(_) => {}
        }
        i += 1;
    }
    match l.register(Sample::rate(STAT_CELLS)) {
        Ok(()) => {}
        Err(_) => {}
    }
    let mut j: usize = 0;
    while j < REJECT {
        let c = match REJECT_CODES.get(j) {
            Some(x) => *x,
            None => 0,
        };
        let idx = STAT_CELLS.saturating_add(1).saturating_add(j);
        match l.register(Sample::defect(idx, c)) {
            Ok(()) => {}
            Err(_) => {}
        }
        j += 1;
    }
    l
}

/// 造一格身份错的账：缺陷码被换成不在 `REJECT_CODES` 里的 `ST_EINMEM`。
pub fn broken_code_ledger() -> Ledger {
    let mut l = build_ledger();
    match l.items.get_mut(STAT_CELLS.saturating_add(1).saturating_add(2)) {
        Some(s) => s.code = ST_EINMEM,
        None => {}
    }
    l
}

/// 造一格被篡改的账（缺陷格值非 0，`audit` 必须以 `ST_EEMIT` 判死）。
pub fn broken_defect_ledger() -> Ledger {
    let mut l = build_ledger();
    match l.items.get_mut(STAT_CELLS.saturating_add(1)) {
        Some(s) => s.value = 7,
        None => {}
    }
    l
}

/// 造一格自相矛盾的账（缺陷码格却报成成功），登记时必须被拒。
pub fn contradictory_sample() -> Sample {
    Sample {
        slot: SAMPLES as u8,
        op: OP_DEFECT,
        ok: true,
        code: ST_ENOTE,
        value: 0,
    }
}

/// 四路注入自检：表路 / 判路 / 渲染路 / 账本路，各自独立，四格全真才算过。
pub fn inject_four() -> [bool; 4] {
    // 路 1 表路：14 槽申报逐格命中期望结论，且期望表与申报表逐维一致。
    let table = all_claims_land() && slot_count() == CORPUS && want_matches_claims();

    // 路 2 判路：把一格的结论注入成期望之外的维，命中数必须正好掉 1，
    // 千分率必须正好落在红档标定 929 上。
    let judged = match run_with(Some((2usize, Fault::Note))) {
        Ok(r) => r.matched + 1 == r.total && r.rate() == RED_RATE && !r.green(),
        Err(_) => false,
    };

    // 路 3 渲染路：全绿渲染必须逐字等于 RENDER_OK，且出口闸放行。
    let rendered = match run() {
        Ok(r) => r.render().as_str() == RENDER_OK && r.gate().is_ok(),
        Err(_) => false,
    };

    // 路 4 账本路：建账→审计→冻结→重放→乱序拒登→篡改拒登，一环都不能松。
    let mut ledger_route = true;
    let mut l = build_ledger();
    if l.audit() != Ok(()) {
        ledger_route = false;
    }
    let root = l.freeze();
    if root == 0 || !l.is_frozen() {
        ledger_route = false;
    }
    if l.replay() != Ok(root) {
        ledger_route = false;
    }
    if l.register(Sample::stat(SAMPLES, 1)).is_ok() {
        ledger_route = false;
    }
    if l.rejects() == 0 {
        ledger_route = false;
    }

    // 顺带复核纵线：走完整条之后倒退必须被拒且留下指纹变化。
    let mut g = Gate::new();
    let mut spine_ok = true;
    match g.walk_and_seal() {
        Ok(()) => {}
        Err(_) => spine_ok = false,
    }
    let before = g.fingerprint();
    if g.step(Stage::Args).is_ok() {
        spine_ok = false;
    }
    if g.fingerprint() == before {
        spine_ok = false;
    }
    [table, judged, rendered, ledger_route && spine_ok]
}

/// 四路的短名（自检明细与测试共用）。
pub const INJECT_ROUTES: [&str; 4] = ["table", "judge", "render", "ledger"];

// ---------------------------------------------------------------------------
// 锚位对齐（本卡自声明的近邻段基址复算）
// ---------------------------------------------------------------------------

/// 三张近邻卡的段基址必须两两不同，且与本卡的位距逐字成立。
pub fn anchors_agree() -> bool {
    if ANCHOR_F11681_SEG.saturating_add(ANCHOR_F11681_GAP) != SEG_BASE {
        return false;
    }
    if ANCHOR_F11682_SEG.saturating_add(ANCHOR_F11682_GAP) != SEG_BASE {
        return false;
    }
    if ANCHOR_F11684_SEG.saturating_add(ANCHOR_F11684_GAP) != SEG_BASE {
        return false;
    }
    if SEG_BASE.saturating_add(NEXT_F11686_GAP) != NEXT_F11686_SEG {
        return false;
    }
    if ANCHOR_F11681_SEG == ANCHOR_F11682_SEG
        || ANCHOR_F11682_SEG == ANCHOR_F11684_SEG
        || ANCHOR_F11681_SEG == ANCHOR_F11684_SEG
    {
        return false;
    }
    if NEXT_F11686_SEG <= SEG_BASE {
        return false;
    }
    if ANCHOR_ENOSPC != ERR_ENOSPC {
        return false;
    }
    if ANCHOR_SORT_TROUBLE != EXIT_TROUBLE {
        return false;
    }
    if ANCHOR_ERRNO_TEXT != strerror(ERR_ENOSPC) {
        return false;
    }
    if !has_sub(strerror(ERR_EDQUOT), "Quota") {
        return false;
    }
    if SEG_BASE.saturating_add(0x0d) > 0x000f_ffff {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 公共小谓词
// ---------------------------------------------------------------------------

/// 一段短名里不许有重复（O(n²) 逐对比较，只用 `.get()`）。
pub fn strs_unique(xs: &[&str]) -> bool {
    let mut i: usize = 0;
    while i < xs.len() {
        let a = match xs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a.is_empty() {
            return false;
        }
        let mut j: usize = i.saturating_add(1);
        while j < xs.len() {
            let b = match xs.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 秩数组必须从 `start` 起连续递增。
pub fn ranks_contiguous(ranks: &[u32], start: u32) -> bool {
    let mut i: usize = 0;
    while i < ranks.len() {
        let r = match ranks.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if r != start.saturating_add(i as u32) {
            return false;
        }
        i += 1;
    }
    !ranks.is_empty()
}

/// 值是否落在表里。
pub fn in_table(xs: &[u32], v: u32) -> bool {
    let mut i: usize = 0;
    while i < xs.len() {
        if let Some(x) = xs.get(i) {
            if *x == v {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 表里数出满足条件的格数。
pub fn count_where(flags: &[bool], want: bool) -> usize {
    let mut n: usize = 0;
    let mut i: usize = 0;
    while i < flags.len() {
        if let Some(x) = flags.get(i) {
            if *x == want {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// 谓词 01：名称与段基址
// ---------------------------------------------------------------------------

fn name_ok() -> bool {
    if SEG_BASE != 0x6d310 {
        return false;
    }
    if ALL_CODES.len() != CODE_COUNT || CODE_COUNT != 13 {
        return false;
    }
    let mut i: usize = 0;
    while i < ALL_CODES.len() {
        let c = match ALL_CODES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want = match SEG_BASE.checked_add((i + 1) as u32) {
            Some(v) => v,
            None => return false,
        };
        if c != want {
            return false;
        }
        if code_offset(c) != (i as i32) + 1 {
            return false;
        }
        if code_name(c) == "clean" {
            return false;
        }
        i += 1;
    }
    let mut names: [&str; CODE_COUNT] = ["a"; CODE_COUNT];
    let mut k: usize = 0;
    while k < CODE_COUNT {
        let c = match ALL_CODES.get(k) {
            Some(v) => *v,
            None => return false,
        };
        match names.get_mut(k) {
            Some(slot) => *slot = code_name(c),
            None => return false,
        }
        k += 1;
    }
    if !strs_unique(&names) {
        return false;
    }
    if code_offset(SEG_BASE) != -1 {
        return false;
    }
    if code_offset(SEG_BASE.saturating_add(0x0e)) != -1 {
        return false;
    }
    if code_offset(ANCHOR_F11681_SEG.saturating_add(1)) != -1 {
        return false;
    }
    if code_name(ERR_NONE) != "clean" {
        return false;
    }
    if !anchors_agree() {
        return false;
    }
    if strerror(ERR_NONE).len() != 0 {
        return false;
    }
    if errno_name(ERR_ENOSPC) != "ENOSPC" {
        return false;
    }
    if errno_name(7000) != "?" || errno_name(ERR_EIO).is_empty() {
        return false;
    }
    if !in_table(&ERRNO_SET, ERR_EDQUOT) || in_table(&ERRNO_SET, ERR_EACCES.saturating_add(1)) {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 谓词 02：枚举与九闸纵线
// ---------------------------------------------------------------------------

fn enums_ok() -> bool {
    // 九闸
    if Stage::ALL.len() != STAGE_COUNT as usize {
        return false;
    }
    if STAGE_RANKS.len() != Stage::ALL.len() {
        return false;
    }
    let mut ranks: [u32; STAGE_COUNT as usize] = [0; STAGE_COUNT as usize];
    let mut slugs: [&str; STAGE_COUNT as usize] = ["z"; STAGE_COUNT as usize];
    let mut i: usize = 0;
    while i < Stage::ALL.len() {
        let st = match Stage::ALL.get(i) {
            Some(v) => *v,
            None => return false,
        };
        match ranks.get_mut(i) {
            Some(x) => *x = st.rank(),
            None => return false,
        }
        match slugs.get_mut(i) {
            Some(x) => *x = st.slug(),
            None => return false,
        }
        let back = match Stage::from_rank(st.rank()) {
            Some(v) => v,
            None => return false,
        };
        if back != st || st.index() != i {
            return false;
        }
        if st.name().is_empty() {
            return false;
        }
        if i > 0 {
            let prev = match Stage::ALL.get(i - 1) {
                Some(v) => *v,
                None => return false,
            };
            if !prev.before(&st) {
                return false;
            }
            if let Some(nx) = prev.next() {
                if nx != st {
                    return false;
                }
            } else {
                return false;
            }
        }
        i += 1;
    }
    if !ranks_contiguous(&ranks, 1) {
        return false;
    }
    if !strs_unique(&slugs) {
        return false;
    }
    if Stage::from_rank(0).is_some() || Stage::from_rank(STAGE_LAST_RANK + 1).is_some() {
        return false;
    }
    if Stage::Settle.next().is_some() || Stage::Args.before(&Stage::Args) {
        return false;
    }
    // 只有收尾闸不属失败族；清理闸是结算闸
    let mut settle_only: usize = 0;
    let mut k: usize = 0;
    while k < Stage::ALL.len() {
        if let Some(st) = Stage::ALL.get(k) {
            if st.settles() {
                settle_only = settle_only.saturating_add(1);
            }
        }
        k += 1;
    }
    if settle_only != 2 {
        return false;
    }
    // 24 档文案
    if Note::ALL.len() != NOTE_COUNT {
        return false;
    }
    let mut note_ranks: [u32; NOTE_COUNT] = [0; NOTE_COUNT];
    let mut note_slugs: [&str; NOTE_COUNT] = ["y"; NOTE_COUNT];
    let mut space_flags = [false; NOTE_COUNT];
    let mut foreign_flags = [false; NOTE_COUNT];
    let mut refused_flags = [false; NOTE_COUNT];
    let mut ok_flags = [false; NOTE_COUNT];
    let mut m: usize = 0;
    while m < NOTE_COUNT {
        let nt = match Note::ALL.get(m) {
            Some(v) => *v,
            None => return false,
        };
        match note_ranks.get_mut(m) {
            Some(x) => *x = nt.rank(),
            None => return false,
        }
        match note_slugs.get_mut(m) {
            Some(x) => *x = nt.slug(),
            None => return false,
        }
        if nt.index() != m {
            return false;
        }
        if Note::from_rank(nt.rank()) != Some(nt) {
            return false;
        }
        if nt.space_family() != (nt.rank() >= NOTE_FIRST_SPACE && nt.rank() <= NOTE_LAST_SPACE) {
            return false;
        }
        if nt.foreign_family()
            != (nt.rank() >= NOTE_FIRST_FOREIGN && nt.rank() <= NOTE_LAST_FOREIGN)
        {
            return false;
        }
        if nt.refused_family()
            != (nt.rank() >= NOTE_FIRST_REFUSED && nt.rank() <= NOTE_LAST_REFUSED)
        {
            return false;
        }
        if nt.success() != (nt.rank() <= NOTE_LAST_OK) {
            return false;
        }
        if let Some(x) = space_flags.get_mut(m) {
            *x = nt.space_family();
        }
        if let Some(x) = foreign_flags.get_mut(m) {
            *x = nt.foreign_family();
        }
        if let Some(x) = refused_flags.get_mut(m) {
            *x = nt.refused_family();
        }
        if let Some(x) = ok_flags.get_mut(m) {
            *x = nt.success();
        }
        if nt.anchor_candidate() != nt.space_family() {
            return false;
        }
        if nt.gnu_only() == nt.busybox_only() {
            return false;
        }
        m += 1;
    }
    if !ranks_contiguous(&note_ranks, 0) || !strs_unique(&note_slugs) {
        return false;
    }
    if Note::from_rank(NOTE_COUNT as u32).is_some() {
        return false;
    }
    if count_where(&ok_flags, true) != 4 {
        return false;
    }
    if count_where(&space_flags, true) != 7 {
        return false;
    }
    if count_where(&foreign_flags, true) != 7 {
        return false;
    }
    if count_where(&refused_flags, true) != 6 {
        return false;
    }
    // 四档
    if TIER_ALL.len() != TIER_COUNT {
        return false;
    }
    let mut t: usize = 0;
    while t < TIER_COUNT {
        let tr = match TIER_ALL.get(t) {
            Some(v) => *v,
            None => return false,
        };
        if tr.index() != t || Tier::from_index(t) != Some(tr) || tr.name().is_empty() {
            return false;
        }
        if tr.anchors() != (tr == Tier::Degraded) {
            return false;
        }
        t += 1;
    }
    if Tier::from_index(TIER_COUNT).is_some() {
        return false;
    }
    // 六落点
    if Locus::ALL.len() != LOCUS_COUNT {
        return false;
    }
    let mut locus_idx: [u32; LOCUS_COUNT] = [0; LOCUS_COUNT];
    let mut locus_slug: [&str; LOCUS_COUNT] = ["w"; LOCUS_COUNT];
    let mut z: usize = 0;
    while z < LOCUS_COUNT {
        let l = match Locus::ALL.get(z) {
            Some(v) => *v,
            None => return false,
        };
        match locus_idx.get_mut(z) {
            Some(x) => *x = l.index() as u32,
            None => return false,
        }
        match locus_slug.get_mut(z) {
            Some(x) => *x = l.slug(),
            None => return false,
        }
        if Locus::from_index(l.index()) != Some(l) || l.subject().is_empty() {
            return false;
        }
        if l.in_bounds() != true {
            return false;
        }
        z += 1;
    }
    if !ranks_contiguous(&locus_idx, 0) || !strs_unique(&locus_slug) {
        return false;
    }
    if Locus::from_index(LOCUS_COUNT).is_some() {
        return false;
    }
    // 目录六态
    if TmpDir::ALL.len() != TMPDIR_KINDS {
        return false;
    }
    let mut d: usize = 0;
    let mut dir_slug: [&str; TMPDIR_KINDS] = ["v"; TMPDIR_KINDS];
    let mut dir_idx: [u32; TMPDIR_KINDS] = [0; TMPDIR_KINDS];
    while d < TMPDIR_KINDS {
        let dk = match TmpDir::ALL.get(d) {
            Some(v) => *v,
            None => return false,
        };
        match dir_slug.get_mut(d) {
            Some(x) => *x = dk.slug(),
            None => return false,
        }
        match dir_idx.get_mut(d) {
            Some(x) => *x = dk.index() as u32,
            None => return false,
        }
        if TmpDir::from_index(dk.index()) != Some(dk) || dir_name(&dk).is_empty() {
            return false;
        }
        d += 1;
    }
    if !ranks_contiguous(&dir_idx, 0) || !strs_unique(&dir_slug) {
        return false;
    }
    if TmpDir::from_index(TMPDIR_KINDS).is_some() {
        return false;
    }
    // 两族
    if Flavor::ALL.len() != FLAVOR_COUNT {
        return false;
    }
    let gnu = match Flavor::ALL.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let busy = match Flavor::ALL.get(1) {
        Some(v) => *v,
        None => return false,
    };
    if gnu.trouble_exit() == busy.trouble_exit() || gnu.quotes_paths() == busy.quotes_paths() {
        return false;
    }
    if gnu.accepts_tempdir_flag() == busy.accepts_tempdir_flag() {
        return false;
    }
    if gnu.accepts_buffer_size_flag() == busy.accepts_buffer_size_flag() {
        return false;
    }
    if !gnu.spills() || gnu.temp_stem().is_empty() || gnu.name().is_empty() {
        return false;
    }
    if gnu.quiet_check_exit() != busy.quiet_check_exit() {
        return false;
    }
    // 判定维
    if Fault::ALL.len() != 7 || JUDGE_ORDER.len() != REJECT {
        return false;
    }
    if !judge_order_is_strict() {
        return false;
    }
    if !ranks_contiguous(&[0, 1, 2, 3, 4, 5, 6], 0) {
        return false;
    }
    let mut f: usize = 0;
    while f < REJECT {
        let want_f = match REJECT_FAULTS.get(f) {
            Some(v) => *v,
            None => return false,
        };
        let order_f = match JUDGE_ORDER.get(f) {
            Some(v) => *v,
            None => return false,
        };
        if want_f != order_f {
            return false;
        }
        let all_f = match Fault::ALL.get(f + 1) {
            Some(v) => *v,
            None => return false,
        };
        if all_f != want_f || want_f.rank() != (f as u32) + 1 {
            return false;
        }
        if want_f.blurb().is_empty() || want_f.slug().is_empty() {
            return false;
        }
        let c = match REJECT_CODES.get(f) {
            Some(v) => *v,
            None => return false,
        };
        if want_f.code() != c || code_offset(c) != (f as i32) + 1 {
            return false;
        }
        if !want_f.is_fault() {
            return false;
        }
        f += 1;
    }
    if Fault::Ok.is_fault() || Fault::Ok.code() != ERR_NONE {
        return false;
    }
    let mut fs: [&str; 7] = ["u"; 7];
    let mut q: usize = 0;
    while q < 7 {
        if let Some(x) = fs.get_mut(q) {
            *x = match Fault::ALL.get(q) {
                Some(v) => v.slug(),
                None => return false,
            };
        }
        q += 1;
    }
    strs_unique(&fs)
}

// ---------------------------------------------------------------------------
// 谓词 03：落点与目录六态
// ---------------------------------------------------------------------------

fn locus_ok() -> bool {
    // 落点 → 闸的映射必须与文件头的闸门次序一致
    let expect: [(Locus, u32); LOCUS_COUNT] = [
        (Locus::Nowhere, 4),
        (Locus::TmpCreate, 2),
        (Locus::SpillWrite, 5),
        (Locus::MergeRead, 6),
        (Locus::OutWrite, 7),
        (Locus::Memory, 6),
    ];
    let mut i: usize = 0;
    let mut emit_flags = [false; LOCUS_COUNT];
    let mut temp_flags = [false; LOCUS_COUNT];
    let mut space_flags = [false; LOCUS_COUNT];
    while i < LOCUS_COUNT {
        let (l, rank) = match expect.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if l.stage().rank() != rank {
            return false;
        }
        if let Some(x) = emit_flags.get_mut(i) {
            *x = l.may_emit();
        }
        if let Some(x) = temp_flags.get_mut(i) {
            *x = l.touches_temp();
        }
        if let Some(x) = space_flags.get_mut(i) {
            *x = l.space_legit();
        }
        // 落点归到第几闸，必须落在 1..=9 之内，且写挂闸不许是收尾闸
        let st = l.stage();
        if st.settles() || st.rank() > STAGE_LAST_RANK {
            return false;
        }
        i += 1;
    }
    if count_where(&emit_flags, true) != 1 {
        return false;
    }
    if count_where(&temp_flags, true) != 3 {
        return false;
    }
    if count_where(&space_flags, true) != 3 {
        return false;
    }
    // 主语：目录 / 件名 / 目的地 / 内存 / 无
    let mut subj: [&str; LOCUS_COUNT] = [""; LOCUS_COUNT];
    let mut k: usize = 0;
    while k < LOCUS_COUNT {
        if let Some(x) = subj.get_mut(k) {
            *x = match Locus::ALL.get(k) {
                Some(v) => v.subject(),
                None => return false,
            };
        }
        k += 1;
    }
    if let Some(first) = subj.get(0) {
        if *first != "none" {
            return false;
        }
    } else {
        return false;
    }
    if let Some(second) = subj.get(1) {
        if *second != "dir" {
            return false;
        }
    } else {
        return false;
    }
    if let Some(last) = subj.get(5) {
        if *last != "memory" {
            return false;
        }
    } else {
        return false;
    }
    if let Some(fourth) = subj.get(4) {
        if *fourth != "output" {
            return false;
        }
    } else {
        return false;
    }
    // 创建档的主语是目录，写出档的主语是件名——两者必须可分
    if Locus::TmpCreate.subject() == Locus::SpillWrite.subject() {
        return false;
    }
    if Note::TmpCreateGnu.subject_is_dir() != true || Note::SpillWriteGnu.subject_is_dir() != false
    {
        return false;
    }
    if !Note::StdoutGnu.subject_is_stdout() || Note::SpillWriteGnu.subject_is_stdout() {
        return false;
    }
    // 缺省目录可否承载这一档
    if Locus::TmpCreate.can_be_default_dir() {
        return false;
    }
    if !Locus::SpillWrite.can_be_default_dir() || !Locus::OutWrite.can_be_default_dir() {
        return false;
    }
    // 目录六态的语义表
    if !(TmpDir::Unset.falls_back() && TmpDir::EnvEmpty.falls_back()) {
        return false;
    }
    if TmpDir::EnvSet.falls_back() || TmpDir::Explicit.falls_back() {
        return false;
    }
    if TmpDir::EnvEmpty.is_user_choice() || TmpDir::Unset.is_user_choice() {
        return false;
    }
    if !TmpDir::EnvBad.is_user_choice() || !TmpDir::EnvFullDir.is_user_choice() {
        return false;
    }
    if TmpDir::EnvBad.usable() || !TmpDir::EnvFullDir.usable() {
        return false;
    }
    if !TmpDir::EnvFullDir.can_create() || TmpDir::EnvBad.can_create() {
        return false;
    }
    if !(TmpDir::Unset.can_create()
        && TmpDir::EnvEmpty.can_create()
        && TmpDir::EnvSet.can_create()
        && TmpDir::Explicit.can_create())
    {
        return false;
    }
    if TmpDir::EnvFullDir.errno_implied() != ERR_ENOSPC {
        return false;
    }
    if TmpDir::EnvBad.errno_implied() != ERR_ENOTDIR {
        return false;
    }
    if !(TmpDir::Unset.errno_implied() == ERR_NONE
        && TmpDir::EnvEmpty.errno_implied() == ERR_NONE
        && TmpDir::EnvSet.errno_implied() == ERR_NONE
        && TmpDir::Explicit.errno_implied() == ERR_NONE)
    {
        return false;
    }
    // 空值与未设都退回缺省目录，且是同一个目录名
    if dir_name(&TmpDir::Unset) != DIR_DEFAULT || dir_name(&TmpDir::EnvEmpty) != DIR_DEFAULT {
        return false;
    }
    if dir_name(&TmpDir::EnvSet) == DIR_DEFAULT || dir_name(&TmpDir::Explicit) == DIR_DEFAULT {
        return false;
    }
    if !dir_name(&TmpDir::EnvBad).starts_with(DIR_FULL) {
        return false;
    }
    // 件名是 `<dir>/sort<后缀>`，序号从 1 起，越界回绕
    if chunk_name(DIR_DEFAULT, 1) != "/tmp/sortZ3fQaL" {
        return false;
    }
    if chunk_name(DIR_DEFAULT, 3) != "/tmp/sortqW2sVc" {
        return false;
    }
    if chunk_name(DIR_DEFAULT, 9) != chunk_name(DIR_DEFAULT, 1) {
        return false;
    }
    if !chunk_name(DIR_ENV, 2).starts_with(DIR_ENV) {
        return false;
    }
    if TMP_SUFFIXES.len() != 8 {
        return false;
    }
    // 目的地主语二选一
    if out_name(OPT_OUTPUT) != OUT_FILE || out_name(0) != STDIO_NAME {
        return false;
    }
    if subject_name(&SHAPE_STDOUT_PARTIAL) != STDIO_NAME {
        return false;
    }
    if subject_name(&SHAPE_SPILL3_ENOSPC) != "/mnt/data/tmp/sortqW2sVc" {
        return false;
    }
    // 语料十四槽的落点与目录来源全部自洽
    let mut z: usize = 0;
    while z < CORPUS {
        let sh = match SHAPES.get(z) {
            Some(v) => *v,
            None => return false,
        };
        if !locus_dir_agrees(&sh.locus, &sh.dir) {
            return false;
        }
        if !sh.locus_in_bounds() {
            return false;
        }
        z += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 谓词 04：形态与合法域
// ---------------------------------------------------------------------------

fn shape_ok() -> bool {
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS {
        return false;
    }
    // 缺省现场的每一维都站在合法域里
    let base = Shape::BASE;
    if base.locus != Locus::Nowhere || base.dir != TmpDir::Unset || base.opts != 0 {
        return false;
    }
    if base.code_seen != EXIT_OK || base.errno_seen != ERR_NONE || base.lines != 0 {
        return false;
    }
    if !base.inputs_ok || base.left != 0 || !base.ordered || base.bad_opt {
        return false;
    }
    if define(&base).note != Note::DoneFull {
        return false;
    }
    if plan(&base).spills != 0 || !plan(&base).no_spill {
        return false;
    }
    let mut i: usize = 0;
    while i < CORPUS {
        let sh = match SHAPES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        // 选项位必须全在本卡认得的八位之内
        if sh.opts & !OPT_KNOWN != 0 {
            return false;
        }
        // errno 必须是本卡认得的八个值之一
        if !in_table(&ERRNO_SET, sh.errno_seen) {
            return false;
        }
        // 几何量不得越出声明的上界
        if sh.lines > LINES_MAX {
            return false;
        }
        if sh.lines.saturating_mul(LINE_BYTES) > BYTES_MAX {
            return false;
        }
        if sh.room > LINES_MAX {
            return false;
        }
        if sh.left > sh.made {
            return false;
        }
        if sh.fail_at > sh.made {
            return false;
        }
        if sh.emitted > sh.lines {
            return false;
        }
        if sh.made > plan(&sh).spills {
            return false;
        }
        // 家族越界与不认的选项在语料里一律不许出现
        if sh.family_overreach() || sh.has_unknown_opt() {
            return false;
        }
        // 红线与回收闸：十四槽全部过关（死槽坏的是申报，不是事实）
        if sh.input_guard().is_err() {
            return false;
        }
        if sh.cleanup_gate().is_err() {
            return false;
        }
        if sh.honesty_gate().is_err() {
            return false;
        }
        if sh.geometry_broken() {
            return false;
        }
        // 结论的码必须落在四档阶梯里
        let code = match ACT_CODE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if code != EXIT_OK
            && code != EXIT_QUIET_FAIL
            && code != EXIT_TROUBLE
            && code != EXIT_UNJUDGED
        {
            return false;
        }
        let o = truth(i);
        if o.code != code {
            return false;
        }
        if o.note.refused() || o.note == Note::Usage || o.note == Note::PlanBroken {
            return false;
        }
        i += 1;
    }
    // 折叠：现场全维进指纹后必须可区分（八个活形态两两不同）
    let mut seen: [u64; ACCEPT] = [0; ACCEPT];
    let mut k: usize = 0;
    while k < ACCEPT {
        let sh = match SHAPES.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let h = sh.fold(FNV_OFF);
        let mut j: usize = 0;
        while j < k {
            if let Some(prev) = seen.get(j) {
                if *prev == h {
                    return false;
                }
            }
            j += 1;
        }
        if let Some(slot) = seen.get_mut(k) {
            *slot = h;
        }
        k += 1;
    }
    // 渲染的每一维都出现在串里
    let s0 = match SHAPES.get(3) {
        Some(v) => *v,
        None => return false,
    };
    let r = s0.render();
    if !has_sub(r.as_str(), "spill-write") || !has_sub(r.as_str(), "made=3") {
        return false;
    }
    if !has_sub(r.as_str(), "lines=100") || !has_sub(r.as_str(), "gnu") {
        return false;
    }
    let c3 = match CLAIMS.get(3) {
        Some(v) => *v,
        None => return false,
    };
    let cr = c3.render();
    if !has_sub(cr.as_str(), "ok=false") || !has_sub(cr.as_str(), "anchor=true") {
        return false;
    }
    if c3.exit_hint() != EXIT_TROUBLE {
        return false;
    }
    let c0 = match CLAIMS.get(0) {
        Some(v) => *v,
        None => return false,
    };
    if c0.exit_hint() != EXIT_OK || !c0.success_flag() {
        return false;
    }
    // 申报的翻转构造只动一维
    let base_claim = match CLAIMS.get(3) {
        Some(v) => *v,
        None => return false,
    };
    if base_claim.flip_stage(Stage::Tmpdir).code != base_claim.code {
        return false;
    }
    let flipped = base_claim.flip_code(EXIT_QUIET_FAIL);
    if flipped.stage != base_claim.stage
        || flipped.value != base_claim.value
        || flipped.note != base_claim.note
        || flipped.default != base_claim.default
    {
        return false;
    }
    if base_claim.flip_success(true).success_flag() != true {
        return false;
    }
    if base_claim.flip_value(100).value != 100 || base_claim.flip_default(false).default {
        return false;
    }
    if base_claim.flip_note(Note::TmpCreateGnu).note != Note::TmpCreateGnu {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 探针形态（不入语料，只钉诚实闸与行数闸的边界）
// ---------------------------------------------------------------------------

/// 探针：`-m` 形态却报第 1 件写挂。
const PROBE_MERGE_LIE: Shape = Shape {
    opts: OPT_MERGE,
    lines: 100,
    room: 30,
    made: 1,
    fail_at: 1,
    locus: Locus::SpillWrite,
    errno_seen: ERR_ENOSPC,
    code_seen: EXIT_TROUBLE,
    ..Shape::BASE
};

/// 探针：`-c` 形态却报目录开不出名字。
const PROBE_CHECK_LIE: Shape = Shape {
    opts: OPT_CHECK,
    lines: 100,
    room: 30,
    made: 1,
    locus: Locus::TmpCreate,
    errno_seen: ERR_ENOSPC,
    code_seen: EXIT_TROUBLE,
    ..Shape::BASE
};

/// 探针：`-C` 形态却报第 2 件写挂。
const PROBE_CHECKQ_LIE: Shape = Shape {
    opts: OPT_CHECK_QUIET,
    lines: 100,
    room: 30,
    made: 2,
    fail_at: 2,
    locus: Locus::SpillWrite,
    errno_seen: ERR_ENOSPC,
    code_seen: EXIT_TROUBLE,
    ..Shape::BASE
};

/// 探针：落点是 `Nowhere`，stderr 里却有 ENOSPC。
const PROBE_NOWHERE_LIE: Shape = Shape {
    dir: TmpDir::EnvFullDir,
    lines: 8,
    room: 64,
    errno_seen: ERR_ENOSPC,
    code_seen: EXIT_TROUBLE,
    ..Shape::BASE
};

/// 探针：合并缓冲失败却把 errno 报成 28（ENOMEM 才是它的本名）。
const PROBE_MEM_ENOSPC: Shape = Shape {
    dir: TmpDir::EnvSet,
    lines: 100,
    room: 30,
    made: 3,
    locus: Locus::Memory,
    errno_seen: ERR_ENOSPC,
    code_seen: EXIT_TROUBLE,
    ..Shape::BASE
};

/// 探针：输入正好装满缓冲（边界：不落盘）。
const PROBE_TINY_FITS: Shape = Shape {
    lines: 64,
    room: 64,
    emitted: 64,
    ..Shape::BASE
};

/// 探针：比缓冲多一行（边界：落一件，尾段一行）。
const PROBE_ONE_OVER: Shape = Shape {
    lines: 65,
    room: 64,
    made: 1,
    emitted: 65,
    ..Shape::BASE
};

/// 探针：已吐行数超过输入行数。
const PROBE_EMIT_OVER: Shape = Shape {
    lines: 8,
    room: 64,
    emitted: 9,
    ..Shape::BASE
};

/// 探针：第 3 件写挂却说已吐 7 行。
const PROBE_SPILL_EMIT: Shape = Shape {
    dir: TmpDir::EnvSet,
    lines: 100,
    room: 30,
    made: 3,
    fail_at: 3,
    locus: Locus::SpillWrite,
    errno_seen: ERR_ENOSPC,
    code_seen: EXIT_TROUBLE,
    emitted: 7,
    ..Shape::BASE
};

/// 探针：`-c` 撞上乱序输入（打 `disorder:` 并退 2）。
const PROBE_DISORDER: Shape = Shape {
    opts: OPT_CHECK,
    lines: 6,
    room: 64,
    ordered: false,
    code_seen: EXIT_TROUBLE,
    ..Shape::BASE
};

/// 探针：`-o` 目的地满（主语是文件名，不是 standard output）。
const PROBE_OUT_FILE_ENOSPC: Shape = Shape {
    dir: TmpDir::EnvSet,
    opts: OPT_OUTPUT,
    lines: 100,
    room: 30,
    made: 3,
    locus: Locus::OutWrite,
    errno_seen: ERR_ENOSPC,
    code_seen: EXIT_TROUBLE,
    emitted: 12,
    ..Shape::BASE
};

/// 探针：本卡不认的选项位 + GNU 家族。
const PROBE_BAD_OPTION: Shape = Shape {
    lines: 8,
    room: 64,
    code_seen: EXIT_TROUBLE,
    bad_opt: true,
    ..Shape::BASE
};

// ---------------------------------------------------------------------------
// 谓词 05：诚实闸与落盘规划
// ---------------------------------------------------------------------------

fn honesty_ok() -> bool {
    // 规划：尾段留内存，所以件数是 floor((lines-1)/room)
    if plan(&SHAPE_SPILL3_ENOSPC).spills != 3 {
        return false;
    }
    if plan(&SHAPE_SPILL3_ENOSPC).tail != 10 {
        return false;
    }
    if plan(&SHAPE_BUSY_SPILL).spills != 2 {
        return false;
    }
    if plan(&PROBE_TINY_FITS).spills != 0 || !plan(&PROBE_TINY_FITS).no_spill {
        return false;
    }
    let over = plan(&PROBE_ONE_OVER);
    if over.spills != 1 || over.tail != 1 || over.no_spill {
        return false;
    }
    if plan(&SHAPE_IN_MEMORY).bytes != 8 * LINE_BYTES {
        return false;
    }
    if plan(&SHAPE_MERGE_OK).merge_only != true || plan(&SHAPE_MERGE_OK).check_only != false {
        return false;
    }
    if plan(&SHAPE_CHECK_QUIET_BAD).check_only != true || plan(&SHAPE_CHECK_QUIET_BAD).emits != 0 {
        return false;
    }
    if plan(&SHAPE_SPILLED_OK).emits != 100 {
        return false;
    }
    if !plan(&SHAPE_SPILLED_OK).may_touch_temp() || plan(&SHAPE_IN_MEMORY).may_touch_temp() {
        return false;
    }
    if plan(&PROBE_TINY_FITS).stable {
        return false;
    }
    if plan(&SHAPE_IN_MEMORY).render().is_empty() {
        return false;
    }
    // 三种永不落盘的形态，报临时盘 ENOSPC 一律被诚实闸拦下
    let lies: [Shape; 6] = [
        PROBE_MERGE_LIE,
        PROBE_CHECK_LIE,
        PROBE_CHECKQ_LIE,
        PROBE_NOWHERE_LIE,
        PROBE_MEM_ENOSPC,
        SCENE_MEMORY_LIE,
    ];
    let mut k: usize = 0;
    while k < lies.len() {
        let sh = match lies.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if sh.honesty_gate() != Err(ST_EINMEM) {
            return false;
        }
        let o = define(&sh);
        if o.note != Note::MemoryLie || o.stage != Stage::Buffer || o.code != EXIT_UNJUDGED {
            return false;
        }
        if o.default || o.ok() || !o.refused() {
            return false;
        }
        if !o.note.honesty() && o.note != Note::MemoryLie {
            return false;
        }
        if Tier::of(&o) != Tier::Refused {
            return false;
        }
        k += 1;
    }
    // 诚实闸排在写出闸之前：秩 4 < 秩 5
    if !(Stage::Buffer.rank() < Stage::Spill.rank()) {
        return false;
    }
    if !Stage::Buffer.is_honesty() || Stage::Spill.is_honesty() {
        return false;
    }
    if !Stage::Buffer.emits_nothing() {
        return false;
    }
    // 正判据：满 TMPDIR 对小输入毫无影响
    let full_dir = Shape {
        dir: TmpDir::EnvFullDir,
        ..SHAPE_IN_MEMORY
    };
    let o = define(&full_dir);
    if o.code != EXIT_OK || o.note != Note::DoneFull || !o.ok() || o.default {
        return false;
    }
    if full_dir.honesty_gate().is_err() {
        return false;
    }
    // `-m` 撞满盘照样成功（一个件都不建）
    let merge_full = Shape {
        dir: TmpDir::EnvFullDir,
        ..SHAPE_MERGE_OK
    };
    if define(&merge_full).note != Note::DoneMerge {
        return false;
    }
    // 落盘形态才允许指向临时盘的申报
    if SHAPE_SPILL3_ENOSPC.honesty_gate().is_err() {
        return false;
    }
    if !SHAPE_SPILL3_ENOSPC.touched_disk() || SHAPE_IN_MEMORY.touched_disk() {
        return false;
    }
    // 谎报的取证行必须自陈"从未建过临时件"
    let m = message(&SCENE_MEMORY_LIE, &define(&SCENE_MEMORY_LIE));
    if !has_sub(m.as_str(), "no temporary file was ever created") {
        return false;
    }
    if !has_sub(m.as_str(), "input 8 lines fits buffer 64") {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 谓词 06：已吐行分账
// ---------------------------------------------------------------------------

fn emit_ok() -> bool {
    // 只有输出落点可以非零；语料逐槽复核
    let mut i: usize = 0;
    while i < CORPUS {
        let sh = match SHAPES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let locus = match ACT_LOCUS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if sh.locus != locus {
            return false;
        }
        let lines = match ACT_LINES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let o = truth(i);
        if o.value > lines {
            return false;
        }
        if o.value != lines.saturating_sub(lines) && o.value > 0 {
            if locus != Locus::Nowhere && locus != Locus::OutWrite {
                return false;
            }
        }
        if locus == Locus::SpillWrite && o.value != 0 {
            return false;
        }
        if locus == Locus::TmpCreate && o.value != 0 {
            return false;
        }
        if locus == Locus::MergeRead && o.value != 0 {
            return false;
        }
        i += 1;
    }
    // 输出残缺档：40 行已经出去，收不回来
    if truth(5).value != 40 {
        return false;
    }
    if !Locus::OutWrite.may_emit() || truth(5).stage != Stage::Output {
        return false;
    }
    let stream = msg_partial_stream(40, 100);
    if !has_sub(stream.as_str(), "40 of 100") {
        return false;
    }
    if !has_sub(stream.as_str(), "already outside the process") {
        return false;
    }
    // `-c`/`-C` 从不写输出
    if truth(7).value != 0 || plan(&SHAPE_CHECK_QUIET_BAD).emits != 0 {
        return false;
    }
    if define(&SCENE_CHECK_EMITTED).note != Note::PlanBroken {
        return false;
    }
    if define(&SCENE_CHECK_EMITTED).value != 6 {
        return false;
    }
    // 越界的已吐行数：超过输入行数 = 几何矛盾
    if PROBE_EMIT_OVER.geometry_broken() != true {
        return false;
    }
    if define(&PROBE_EMIT_OVER).note != Note::PlanBroken {
        return false;
    }
    // 写出落点上的任何已吐行数 = 几何矛盾
    if PROBE_SPILL_EMIT.geometry_broken() != true {
        return false;
    }
    if define(&PROBE_SPILL_EMIT).note != Note::PlanBroken {
        return false;
    }
    if !PROBE_SPILL_EMIT.locus.may_emit() {
        return false;
    }
    // 申报行数说谎：死槽 4 挂的就是这一维
    let claim11 = match CLAIMS.get(11) {
        Some(v) => *v,
        None => return false,
    };
    if claim11.value != 100 || truth(11).value != 0 {
        return false;
    }
    if verdict_at(11) != Some(Fault::Value) {
        return false;
    }
    // `-o` 目的地满：已吐 12 行，主语是文件名
    let pf = define(&PROBE_OUT_FILE_ENOSPC);
    if pf.value != 12 || pf.note != Note::OutFileGnu || pf.stage != Stage::Output {
        return false;
    }
    if !pf.default {
        return false;
    }
    let pm = message(&PROBE_OUT_FILE_ENOSPC, &pf);
    if !has_sub(pm.as_str(), "sorted.txt") || has_sub(pm.as_str(), "standard output") {
        return false;
    }
    // 字节折算只在释义行里，判据量始终是行
    if bytes_text(2) != "32" || pair_text(2) != "2 lines / 32 bytes" {
        return false;
    }
    if u32_text(929) != "929" {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 谓词 07：闸门单调与红线优先
// ---------------------------------------------------------------------------

/// 九闸纵线：只许前进一格，倒退归 `ST_ESTAGE`、跳档与原地重踏归 `ST_EBAL`、
/// 封印后一切动作归 `ST_ESTATE`；被拒的动作也必须改指纹（拒动留痕）。
///
/// 同时复核本卡的**优先级次序**：家族越界 > 诚实闸 > 几何闸 > 输入红线 > 回收闸，
/// 以及红线在 `define` 里后置覆盖落点闸——第 8 闸一旦触发就把结论抬过去。
fn gate_ok() -> bool {
    // 起跑线：秩 0 没有阶段，也没动过
    let fresh = Gate::new();
    if fresh.rank() != 0 || fresh.stage().is_some() {
        return false;
    }
    if fresh.moves() != 0 || fresh.rejected() != 0 || fresh.is_closed() {
        return false;
    }
    if fresh.fingerprint() != FNV_OFF {
        return false;
    }

    // 跳档被拒，但**留痕**：秩不动，moves 与 rejected 各自加一，指纹变了
    let mut g = Gate::new();
    if g.step(Stage::Spill) != Err(ST_EBAL) {
        return false;
    }
    if g.rank() != 0 || g.moves() != 1 || g.rejected() != 1 {
        return false;
    }
    if g.fingerprint() == fresh.fingerprint() {
        return false;
    }

    // 原地重踏同一闸：既不是前进也不是倒退，同样归跳档族
    if g.step(Stage::Args) != Ok(()) {
        return false;
    }
    if g.rank() != 1 || g.stage() != Some(Stage::Args) {
        return false;
    }
    if g.step(Stage::Args) != Err(ST_EBAL) {
        return false;
    }
    if g.rank() != 1 {
        return false;
    }

    // 倒退归 `ST_ESTAGE`
    if g.step(Stage::Tmpdir) != Ok(()) {
        return false;
    }
    if g.step(Stage::Input) != Ok(()) {
        return false;
    }
    if g.rank() != 3 {
        return false;
    }
    if g.step(Stage::Tmpdir) != Err(ST_ESTAGE) {
        return false;
    }
    if g.step(Stage::Args) != Err(ST_ESTAGE) {
        return false;
    }
    if g.rank() != 3 || g.moves() != 7 || g.rejected() != 4 {
        return false;
    }

    // 没走到第九闸不许封印
    if g.close() != Err(ST_ESTAGE) {
        return false;
    }
    if g.is_closed() || g.rank() != 3 {
        return false;
    }

    // 正例通道：走完整条纵线并封印
    let mut w = Gate::new();
    if w.walk_and_seal() != Ok(()) {
        return false;
    }
    if w.rank() != STAGE_LAST_RANK || w.stage() != Some(Stage::Settle) {
        return false;
    }
    if w.moves() != STAGE_LAST_RANK + 1 || w.rejected() != 0 {
        return false;
    }
    if !w.is_closed() {
        return false;
    }

    // 封印之后：一切动作归 `ST_ESTATE`，并且仍然留痕
    if w.close() != Err(ST_ESTATE) {
        return false;
    }
    let sealed = w.fingerprint();
    if w.step(Stage::Args) != Err(ST_ESTATE) {
        return false;
    }
    if w.fingerprint() == sealed {
        return false;
    }
    if w.rank() != STAGE_LAST_RANK || w.rejected() != 2 {
        return false;
    }

    // 逐闸复核：`walk_to` 停在第几闸，秩就是几；未到第九闸不许封印
    let mut k: usize = 0;
    while k < Stage::ALL.len() {
        let st = match Stage::ALL.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let mut t = Gate::new();
        if t.walk_to(st) != Ok(()) {
            return false;
        }
        if t.rank() != st.rank() || t.stage() != Some(st) {
            return false;
        }
        if t.moves() != st.rank() || t.rejected() != 0 || t.is_closed() {
            return false;
        }
        if st != Stage::Settle && t.close() != Err(ST_ESTAGE) {
            return false;
        }
        // 从这一闸往回退一律 `ST_ESTAGE`（除了第一闸没什么可退）
        if st.rank() > 1 {
            let back = match Stage::from_rank(st.rank() - 1) {
                Some(v) => v,
                None => return false,
            };
            let mut u = Gate::new();
            if u.walk_to(st) != Ok(()) {
                return false;
            }
            if u.step(back) != Err(ST_ESTAGE) {
                return false;
            }
        }
        k += 1;
    }

    // 闸序单调：`ALL` 的秩必须从 1 起严格递增，且没有第二道闸同秩
    let mut a: usize = 0;
    while a < Stage::ALL.len() {
        let sa = match Stage::ALL.get(a) {
            Some(v) => *v,
            None => return false,
        };
        if sa.rank() != (a as u32) + 1 {
            return false;
        }
        if sa.index() != a {
            return false;
        }
        let mut b: usize = a.saturating_add(1);
        while b < Stage::ALL.len() {
            let sb = match Stage::ALL.get(b) {
                Some(v) => *v,
                None => return false,
            };
            if !sa.before(&sb) {
                return false;
            }
            b += 1;
        }
        a += 1;
    }
    if !ranks_contiguous(&STAGE_RANKS, 1) {
        return false;
    }
    if Stage::ALL.len() as u32 != STAGE_COUNT {
        return false;
    }
    if Stage::Settle.next().is_some() {
        return false;
    }
    if Stage::Args.next() != Some(Stage::Tmpdir) {
        return false;
    }

    // 十四槽的定性闸全在纵线内可达（结论落在第几闸，纵线就走到第几闸）
    let mut i: usize = 0;
    while i < CORPUS {
        let o = truth(i);
        let mut t = Gate::new();
        if t.walk_to(o.stage) != Ok(()) {
            return false;
        }
        if t.rank() != o.stage.rank() {
            return false;
        }
        i += 1;
    }

    // 十一场同样可达；红线场必然停在第 8 闸
    let mut s2: usize = 0;
    while s2 < SCENE_COUNT {
        if let Some((_, o)) = scene_at(s2) {
            let mut t = Gate::new();
            if t.walk_to(o.stage) != Ok(()) {
                return false;
            }
            if o.note.red_line() || o.note == Note::TempLeak {
                if o.stage != Stage::Cleanup {
                    return false;
                }
            }
        }
        s2 += 1;
    }

    // 红线优先之一：输入受损把落点闸（第 5 闸）抬到第 8 闸
    if SCENE_INPUT_DAMAGED.locus.stage() != Stage::Spill {
        return false;
    }
    if define(&SCENE_INPUT_DAMAGED).stage != Stage::Cleanup {
        return false;
    }
    if SCENE_INPUT_DAMAGED.input_guard() != Err(ST_EINPUT) {
        return false;
    }
    if locus_gate(&SCENE_INPUT_DAMAGED) != Err(ST_EINPUT) {
        return false;
    }
    if define(&SCENE_INPUT_DAMAGED).default {
        return false;
    }
    // 现场受损、申报照抄槽 4：成败位相合，所以缺欠是 Stage 而不是 Estate
    let dmg_claim = match CLAIMS.get(3) {
        Some(v) => *v,
        None => return false,
    };
    if judge(&Case::new(SCENE_INPUT_DAMAGED, dmg_claim)) != Fault::Stage {
        return false;
    }

    // 红线优先之二：临时件残留把一次成功的落盘跑也抬到第 8 闸、收回降级锚
    if SCENE_TEMP_LEAK.cleanup_gate() != Err(ST_EANCH) {
        return false;
    }
    if locus_gate(&SCENE_TEMP_LEAK) != Err(ST_EANCH) {
        return false;
    }
    let leak = define(&SCENE_TEMP_LEAK);
    if leak.stage != Stage::Cleanup || leak.code != EXIT_UNJUDGED || leak.default {
        return false;
    }
    if leak.note != Note::TempLeak || leak.value != 100 {
        return false;
    }
    // 残留数超过开出的件数：那是账目不平（`ST_EBAL`），不是"有残留"
    let leak_over = Shape {
        left: 5,
        ..SCENE_TEMP_LEAK
    };
    if leak_over.cleanup_gate() != Err(ST_EBAL) {
        return false;
    }
    if leak_over.geometry_broken() != true {
        return false;
    }

    // 优先级：家族越界连诚实闸都不进；诚实闸排在几何闸之前
    if locus_gate(&SCENE_BUSY_TFLAG) != Err(ST_ECOUNT) {
        return false;
    }
    if SCENE_MEMORY_LIE.honesty_gate() != Err(ST_EINMEM) {
        return false;
    }
    if locus_gate(&SCENE_MEMORY_LIE) != Err(ST_EINMEM) {
        return false;
    }
    if SCENE_MADE_EXCEEDS.geometry_broken() != true {
        return false;
    }
    if locus_gate(&SCENE_MADE_EXCEEDS) != Err(ST_EBAL) {
        return false;
    }
    if locus_gate(&SHAPE_SPILL3_ENOSPC) != Ok(Stage::Spill) {
        return false;
    }
    if locus_gate(&SCENE_DIR_BAD) != Ok(Stage::Tmpdir) {
        return false;
    }

    // 判序固定：多维同时错只报最前一维（六维各测一次）
    let spill = match CLAIMS.get(3) {
        Some(v) => *v,
        None => return false,
    };
    if judge(&Case::new(SHAPE_SPILL3_ENOSPC, spill)) != Fault::Ok {
        return false;
    }
    let layer1 = Claim::new(true, Stage::Args, EXIT_TROUBLE, 99, Note::MemoryLie, true);
    if judge(&Case::new(SHAPE_SPILL3_ENOSPC, layer1)) != Fault::Estate {
        return false;
    }
    let layer2 = Claim::new(false, Stage::Args, EXIT_OK, 9, Note::DoneFull, false);
    if judge(&Case::new(SHAPE_SPILL3_ENOSPC, layer2)) != Fault::Stage {
        return false;
    }
    let layer3 = Claim::new(false, Stage::Spill, EXIT_OK, 9, Note::DoneFull, false);
    if judge(&Case::new(SHAPE_SPILL3_ENOSPC, layer3)) != Fault::Code {
        return false;
    }
    let layer4 = Claim::new(false, Stage::Spill, EXIT_TROUBLE, 9, Note::DoneFull, false);
    if judge(&Case::new(SHAPE_SPILL3_ENOSPC, layer4)) != Fault::Value {
        return false;
    }
    let layer5 = Claim::new(false, Stage::Spill, EXIT_TROUBLE, 0, Note::DoneFull, false);
    if judge(&Case::new(SHAPE_SPILL3_ENOSPC, layer5)) != Fault::Note {
        return false;
    }
    let layer6 = Claim::new(false, Stage::Spill, EXIT_TROUBLE, 0, Note::SpillWriteGnu, false);
    if judge(&Case::new(SHAPE_SPILL3_ENOSPC, layer6)) != Fault::Default {
        return false;
    }
    if judge_order_is_strict() != true {
        return false;
    }
    if JUDGE_ORDER.len() != REJECT {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 谓词 08：文案两族逐字
// ---------------------------------------------------------------------------

/// GNU：`sort: cannot create temporary file in '<dir>': <err>`
const GNU_TMP_CREATE: &str =
    "sort: cannot create temporary file in '/mnt/data/tmp': No space left on device";
/// GNU：目录项已尽的那一版（主语是用户指的目录，不是缺省 `/tmp`）
const GNU_TMP_CREATE_FULLDIR: &str =
    "sort: cannot create temporary file in '/mnt/loop': No space left on device";
/// GNU：`sort: write failed: '<tmp>': <err>`——**验收文案那一枪**逐字
const GNU_SPILL_WRITE: &str = "sort: write failed: '/mnt/data/tmp/sortqW2sVc': No space left on device";
/// GNU：目的地是 stdout 时的固定主语
const GNU_STDOUT_WRITE: &str = "sort: write failed: 'standard output': No space left on device";
/// GNU：`-o FILE` 的目的地满
const GNU_OUT_FILE_WRITE: &str = "sort: write failed: 'sorted.txt': No space left on device";
/// GNU：归并回读不到上一段的临时件（主语是件名，errno 是 ENOENT）
const GNU_MERGE_REREAD: &str =
    "sort: cannot read: '/mnt/data/tmp/sorthK9tYb': No such file or directory";
/// GNU：输入件不见了
const GNU_INPUT_GONE: &str = "sort: cannot read: 'gone.txt': No such file or directory";
/// GNU：输入件读不动（EACCES 的主语是**本名**，不是消失的那个名字）
const GNU_INPUT_DENIED: &str = "sort: cannot read: 'big.txt': Permission denied";
/// GNU：`-c` 撞上乱序
const GNU_DISORDER_LINE: &str = "sort: -:1: disorder: zeta alpha";
/// GNU：合并缓冲分配失败（这一条**不带** strerror 尾巴）
const GNU_MEM_EXHAUSTED: &str = "sort: memory exhausted";
/// busybox：临时件创建失败——不带被引号包住的路径
const BB_TMP_CREATE: &str = "sort: can't create temp file: No space left on device";
/// busybox：临时件写出失败
const BB_SPILL_WRITE: &str = "sort: can't write to temp file: No space left on device";
/// busybox：目的地写出失败
const BB_OUT_WRITE: &str = "sort: write error: No space left on device";
/// busybox：applet 的一行 usage
const BB_USAGE: &str = "usage: sort [-nrcdfu] [FILE]...";

/// 文案两族的逐字复核。
///
/// 本卡的 Note 维之所以能独立成判据，全靠这几式句式互不重叠：
/// GNU 的创建档点名**目录**、写出档点名**件名**、stdout 档用固定主语；
/// busybox 三档一律不带路径，动词也从 `cannot/create` 缩成 `can't`。
fn message_ok() -> bool {
    // 槽 4：写出档——件名是主语、带引号、尾巴是 glibc 原文
    let m3 = message(&SHAPE_SPILL3_ENOSPC, &truth(3));
    if m3.as_str() != GNU_SPILL_WRITE {
        return false;
    }
    if !starts_with(m3.as_str(), "sort: write failed: '") {
        return false;
    }
    if count_char(m3.as_str(), b'\'') != 2 {
        return false;
    }
    if has_sub(m3.as_str(), "can't") || has_sub(m3.as_str(), "cannot") {
        return false;
    }
    if !has_sub(m3.as_str(), DIR_ENV) {
        return false;
    }
    if !has_sub(m3.as_str(), ANCHOR_ERRNO_TEXT) {
        return false;
    }

    // 槽 5：创建档——主语换成了**目录**，两式绝不混认
    let m4 = message(&SHAPE_TMP_CREATE_ENOSPC, &truth(4));
    if m4.as_str() != GNU_TMP_CREATE_FULLDIR {
        return false;
    }
    if !starts_with(m4.as_str(), "sort: cannot create temporary file in '") {
        return false;
    }
    if has_sub(m4.as_str(), "write failed") {
        return false;
    }
    if has_sub(m4.as_str(), "/mnt/loop/sort") {
        return false;
    }
    if count_char(m4.as_str(), b'\'') != 2 {
        return false;
    }

    // 造式复核：件名来自主语函数，不是手抄的第二份
    let by_dir = msg_create_tmp_failed(DIR_ENV, ERR_ENOSPC);
    if by_dir.as_str() != GNU_TMP_CREATE {
        return false;
    }
    let by_tmp = msg_spill_write_failed(chunk_name(DIR_ENV, 3).as_str(), ERR_ENOSPC);
    if by_tmp.as_str() != GNU_SPILL_WRITE {
        return false;
    }
    if chunk_name(DIR_ENV, 1).as_str() != "/mnt/data/tmp/sortZ3fQaL" {
        return false;
    }
    if chunk_name(DIR_DEFAULT, 1).as_str() != "/tmp/sortZ3fQaL" {
        return false;
    }
    if chunk_name(DIR_DEFAULT, 2).as_str() != "/tmp/sorthK9tYb" {
        return false;
    }
    if !starts_with(chunk_name(DIR_ENV, 8).as_str(), DIR_ENV) {
        return false;
    }
    // 序号 8 回到后缀表首（表循环而非越界），序号 0 也必须给出一个可用件名
    if chunk_name(DIR_ENV, 9).as_str() != chunk_name(DIR_ENV, 1).as_str() {
        return false;
    }
    if chunk_name(DIR_ENV, 0).as_str() != "/mnt/data/tmp/sortZ3fQaL" {
        return false;
    }
    if count_char(chunk_name(DIR_ENV, 3).as_str(), b'/') != 4 {
        return false;
    }

    // 槽 6：stdout 档——固定主语，且带引号（与 `-o` 档同式不同名）
    let m5 = message(&SHAPE_STDOUT_PARTIAL, &truth(5));
    if m5.as_str() != GNU_STDOUT_WRITE {
        return false;
    }
    if !has_sub(m5.as_str(), STDIO_NAME) {
        return false;
    }
    if msg_stdout_write_failed(ERR_ENOSPC).as_str() != GNU_STDOUT_WRITE {
        return false;
    }

    // `-o` 目的地档：主语是文件名，绝不写 standard output
    let mof = message(&PROBE_OUT_FILE_ENOSPC, &define(&PROBE_OUT_FILE_ENOSPC));
    if mof.as_str() != GNU_OUT_FILE_WRITE {
        return false;
    }
    if !has_sub(mof.as_str(), OUT_FILE) {
        return false;
    }

    // 归并回读档：同一式 `cannot read`，但主语是**上一段的临时件**
    let mpg = message(&SCENE_PART_GONE, &define(&SCENE_PART_GONE));
    if mpg.as_str() != GNU_MERGE_REREAD {
        return false;
    }
    if msg_part_gone(chunk_name(DIR_ENV, 2).as_str()).as_str() != GNU_MERGE_REREAD {
        return false;
    }
    if !has_sub(mpg.as_str(), strerror(ERR_ENOENT)) {
        return false;
    }
    if has_sub(mpg.as_str(), ANCHOR_ERRNO_TEXT) {
        return false;
    }

    // 输入档两个名字：ENOENT 报消失的那个，EACCES 报本名
    let gone = Shape::new(
        Flavor::Gnu,
        TmpDir::Unset,
        0,
        100,
        30,
        0,
        0,
        Locus::Nowhere,
        ERR_ENOENT,
        EXIT_TROUBLE,
        0,
        true,
        0,
        true,
    );
    if message(&gone, &define(&gone)).as_str() != GNU_INPUT_GONE {
        return false;
    }
    if define(&gone).note != Note::CannotRead {
        return false;
    }
    let denied = Shape::new(
        Flavor::Gnu,
        TmpDir::Unset,
        0,
        100,
        30,
        0,
        0,
        Locus::Nowhere,
        ERR_EACCES,
        EXIT_TROUBLE,
        0,
        true,
        0,
        true,
    );
    if define(&denied).note != Note::CannotRead {
        return false;
    }
    let mdenied = message(&denied, &define(&denied));
    if mdenied.as_str() != GNU_INPUT_DENIED {
        return false;
    }
    if !has_sub(mdenied.as_str(), IN_DATA) {
        return false;
    }

    // 合并缓冲：这一条不带 strerror——把 ENOMEM 的原文塞进去就是伪造
    let mme = message(&SCENE_MEM_EXHAUSTED, &define(&SCENE_MEM_EXHAUSTED));
    if mme.as_str() != GNU_MEM_EXHAUSTED {
        return false;
    }
    if count_char(mme.as_str(), b'\'') != 0 {
        return false;
    }
    if has_sub(mme.as_str(), strerror(ERR_ENOMEM)) {
        return false;
    }
    if has_sub(mme.as_str(), ANCHOR_ERRNO_TEXT) {
        return false;
    }

    // 配额冒充盘满：句式一模一样，尾巴必须换
    let qed = message(&SCENE_EDQUOT_SPILL, &define(&SCENE_EDQUOT_SPILL));
    if !has_sub(qed.as_str(), strerror(ERR_EDQUOT)) {
        return false;
    }
    if has_sub(qed.as_str(), ANCHOR_ERRNO_TEXT) {
        return false;
    }
    if define(&SCENE_EDQUOT_SPILL).note != Note::ForeignErr {
        return false;
    }
    if !has_sub(strerror(ERR_EDQUOT), "Quota") {
        return false;
    }

    // 槽 7（busybox）：三档一律不带路径，动词缩位
    let mb = message(&SHAPE_BUSY_SPILL, &truth(6));
    if mb.as_str() != BB_SPILL_WRITE {
        return false;
    }
    if count_char(mb.as_str(), b'\'') != 1 {
        return false;
    }
    if has_sub(mb.as_str(), DIR_DEFAULT) || has_sub(mb.as_str(), DIR_ENV) {
        return false;
    }
    if msg_bb_create_tmp(ERR_ENOSPC).as_str() != BB_TMP_CREATE {
        return false;
    }
    if msg_bb_out_write(ERR_ENOSPC).as_str() != BB_OUT_WRITE {
        return false;
    }
    if count_char(msg_bb_create_tmp(ERR_ENOSPC).as_str(), b'\'') != 1 {
        return false;
    }
    if count_char(msg_bb_out_write(ERR_ENOSPC).as_str(), b'\'') != 0 {
        return false;
    }
    if !starts_with(mb.as_str(), "sort: can't write to temp file: ") {
        return false;
    }

    // `-c` 打 disorder 行、退 2；`-C` 什么都不打、退 1
    let mdis = message(&PROBE_DISORDER, &define(&PROBE_DISORDER));
    if mdis.as_str() != GNU_DISORDER_LINE {
        return false;
    }
    if !has_sub(mdis.as_str(), "disorder: ") {
        return false;
    }
    if define(&PROBE_DISORDER).note != Note::Disordered {
        return false;
    }
    let mq = message(&SHAPE_CHECK_QUIET_BAD, &truth(7));
    if !mq.is_empty() {
        return false;
    }

    // 用法档两式：GNU 两行、busybox 一行
    let mbbu = message(&SCENE_BUSY_TFLAG, &define(&SCENE_BUSY_TFLAG));
    if mbbu.as_str() != BB_USAGE {
        return false;
    }
    if !starts_with(mbbu.as_str(), "usage:") {
        return false;
    }
    if count_char(mbbu.as_str(), b'\n') != 0 {
        return false;
    }
    let mgou = message(&PROBE_BAD_OPTION, &define(&PROBE_BAD_OPTION));
    if !starts_with(mgou.as_str(), "sort: invalid option -- '") {
        return false;
    }
    if !has_sub(mgou.as_str(), "Try 'sort --help' for more information.") {
        return false;
    }
    if count_char(mgou.as_str(), b'\n') != 1 {
        return false;
    }

    // 取证行：诚实闸/红线/残留/落点越界各一句，都不带 stderr 原文的句式
    let ml = message(&SCENE_MEMORY_LIE, &define(&SCENE_MEMORY_LIE));
    if !has_sub(ml.as_str(), "sort honesty: input 8 lines fits buffer 64") {
        return false;
    }
    if !has_sub(ml.as_str(), "no temporary file was ever created") {
        return false;
    }
    if has_sub(ml.as_str(), "write failed") {
        return false;
    }
    let mdg = message(&SCENE_INPUT_DAMAGED, &define(&SCENE_INPUT_DAMAGED));
    if !has_sub(mdg.as_str(), "anchor withheld") {
        return false;
    }
    if !has_sub(mdg.as_str(), IN_DATA) {
        return false;
    }
    if !has_sub(mdg.as_str(), "800 bytes") {
        return false;
    }
    let mlk = message(&SCENE_TEMP_LEAK, &define(&SCENE_TEMP_LEAK));
    if !has_sub(mlk.as_str(), "2 of 3 chunk files remain in '/mnt/data/tmp'") {
        return false;
    }
    let mll = message(&SCENE_RENAME_AT_MERGE, &define(&SCENE_RENAME_AT_MERGE));
    if !has_sub(mll.as_str(), "0 of 100 records are already outside the process") {
        return false;
    }

    // 降级锚的现场摘要行：件数与已吐行数同时报出（本卡自研取证）
    let mk = msg_degrade_marker(chunk_name(DIR_ENV, 3).as_str(), 3, 3, 0);
    if !has_sub(mk.as_str(), "chunk 3 of 3") {
        return false;
    }
    if !has_sub(mk.as_str(), "emitted 0 records") {
        return false;
    }
    if !has_sub(mk.as_str(), TEMP_STEM) {
        return false;
    }

    // 输入完好性的正判据锚行（成档取证）
    let mi = msg_input_intact(3);
    if !has_sub(mi.as_str(), "3 of 3 files still byte-for-byte") {
        return false;
    }

    // 逐档通则：空文案当且仅当这一档不打印任何东西
    let mut ok_notes: u32 = 0;
    let mut space_notes: u32 = 0;
    let mut foreign_notes: u32 = 0;
    let mut refused_notes: u32 = 0;
    let mut honesty_notes: u32 = 0;
    let mut redline_notes: u32 = 0;
    let mut k: usize = 0;
    while k < NOTE_COUNT {
        let n = match Note::ALL.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let o = Outcome::new(Stage::Settle, EXIT_OK, 0, n, false);
        let m = message(&Shape::BASE, &o);
        if m.is_empty() == o.prints() {
            return false;
        }
        let bands = (n.success() as u32)
            + (n.space_family() as u32)
            + (n.foreign_family() as u32)
            + (n.refused_family() as u32);
        if bands != 1 {
            return false;
        }
        ok_notes = ok_notes + n.success() as u32;
        space_notes = space_notes + n.space_family() as u32;
        foreign_notes = foreign_notes + n.foreign_family() as u32;
        refused_notes = refused_notes + n.refused_family() as u32;
        honesty_notes = honesty_notes + n.honesty() as u32;
        redline_notes = redline_notes + n.red_line() as u32;
        if n.refused() && !n.refused_family() {
            return false;
        }
        if n.anchor_candidate() != n.space_family() {
            return false;
        }
        if n.space_family() && n.errno_implied() != ERR_ENOSPC {
            return false;
        }
        if n.gnu_only() && n.busybox_only() {
            return false;
        }
        if n.blames_temp() && n.blames_output() {
            return false;
        }
        if n.subject_is_stdout() && !n.blames_output() {
            return false;
        }
        if n.subject_is_dir() && !n.blames_temp() {
            return false;
        }
        if n.red_line() && n.honesty() {
            return false;
        }
        k += 1;
    }
    if ok_notes != 4 || space_notes != 7 || foreign_notes != 7 || refused_notes != 6 {
        return false;
    }
    if ok_notes + space_notes + foreign_notes + refused_notes != NOTE_COUNT as u32 {
        return false;
    }
    // 诚实闸与红线各只有一档：这两维是本卡的排他判据，不许摊到多档上
    if honesty_notes != 1 || redline_notes != 1 {
        return false;
    }
    if !Note::MemoryLie.honesty() || !Note::InputDamaged.red_line() {
        return false;
    }
    if Note::TempLeak.red_line() {
        return false;
    }

    // 十四槽逐槽：文案要么正是该族该档的那一式，要么就是本卡自研取证行
    let mut i: usize = 0;
    while i < CORPUS {
        let sh = match SHAPES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let o = truth(i);
        let m = message(sh, &o);
        if m.is_empty() == o.prints() {
            return false;
        }
        if o.note.blames_temp() || o.note.blames_output() {
            if !has_sub(m.as_str(), "sort: ") {
                return false;
            }
            if o.note.space_family() && sh.flavor.quotes_paths() {
                if count_char(m.as_str(), b'\'') != 2 {
                    return false;
                }
            }
        }
        if o.note == Note::DisorderedQuiet {
            if !m.is_empty() {
                return false;
            }
        }
        i += 1;
    }

    // 取证行 = 原文 + 结论渲染；成档与 `-C` 走 `(no stderr)` 哨兵
    let ev7 = match SHAPES.get(7) {
        Some(v) => evidence(v, &truth(7)),
        None => return false,
    };
    if !has_sub(ev7.as_str(), "sort: (no stderr)") {
        return false;
    }
    if !has_sub(ev7.as_str(), "note=disorder-quiet") {
        return false;
    }
    let ev3 = match SHAPES.get(3) {
        Some(v) => evidence(v, &truth(3)),
        None => return false,
    };
    if !has_sub(ev3.as_str(), GNU_SPILL_WRITE) {
        return false;
    }
    if !has_sub(ev3.as_str(), "anchor=true") {
        return false;
    }
    if !starts_with(ev3.as_str(), "sort: write failed: '") {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 谓词 09：退码阶梯与家族
// ---------------------------------------------------------------------------

/// 本卡认得的四个退出码（阶梯自下而上：成 → 静默失败 → 麻烦 → 未判定）。
const EXIT_SET: [u32; EXIT_RANKS] = [EXIT_OK, EXIT_QUIET_FAIL, EXIT_TROUBLE, EXIT_UNJUDGED];

/// 退码复核：GNU 把一切麻烦归 2、busybox 一律退 1，而 `-C` 在 GNU 里**打破**自己的阶梯退 1。
///
/// 本卡采信的是按家族阶梯**算出**的码，观测到的 `code_seen` 只进指纹、不参与定性。
fn exit_ok() -> bool {
    // 阶梯本身：四个码互异且严格递增
    if EXIT_RANKS != 4 {
        return false;
    }
    if EXIT_OK >= EXIT_QUIET_FAIL || EXIT_QUIET_FAIL >= EXIT_TROUBLE {
        return false;
    }
    if EXIT_TROUBLE >= EXIT_UNJUDGED {
        return false;
    }
    let mut a: usize = 0;
    while a < EXIT_SET.len() {
        let x = match EXIT_SET.get(a) {
            Some(v) => *v,
            None => return false,
        };
        let mut b: usize = a.saturating_add(1);
        while b < EXIT_SET.len() {
            let y = match EXIT_SET.get(b) {
                Some(v) => *v,
                None => return false,
            };
            if x >= y {
                return false;
            }
            b += 1;
        }
        a += 1;
    }
    if !in_table(&EXIT_SET, ANCHOR_SORT_TROUBLE) {
        return false;
    }
    if ANCHOR_SORT_TROUBLE != EXIT_TROUBLE {
        return false;
    }

    // 两族阶梯：麻烦码、用法码
    if Flavor::Gnu.trouble_exit() != EXIT_TROUBLE {
        return false;
    }
    if Flavor::Busybox.trouble_exit() != EXIT_QUIET_FAIL {
        return false;
    }
    if Flavor::Gnu.usage_exit() != EXIT_TROUBLE {
        return false;
    }
    if Flavor::Busybox.usage_exit() != EXIT_QUIET_FAIL {
        return false;
    }
    if Flavor::Gnu.quiet_check_exit() != EXIT_QUIET_FAIL {
        return false;
    }
    if Flavor::Busybox.quiet_check_exit() != EXIT_QUIET_FAIL {
        return false;
    }
    if Flavor::Gnu.usage_exit() != Flavor::Gnu.trouble_exit() {
        return false;
    }

    // 逐档：成档 0，静默乱序 1，本卡拒收族 99，其余按家族阶梯
    let mut k: usize = 0;
    while k < NOTE_COUNT {
        let n = match Note::ALL.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let g = n.exit_code(&Flavor::Gnu);
        let b = n.exit_code(&Flavor::Busybox);
        if !in_table(&EXIT_SET, g) || !in_table(&EXIT_SET, b) {
            return false;
        }
        if n.success() {
            if g != EXIT_OK || b != EXIT_OK {
                return false;
            }
        } else if n == Note::DisorderedQuiet {
            if g != EXIT_QUIET_FAIL || b != EXIT_QUIET_FAIL {
                return false;
            }
        } else if n.refused() {
            if g != EXIT_UNJUDGED || b != EXIT_UNJUDGED {
                return false;
            }
        } else {
            if g != EXIT_TROUBLE || b != EXIT_QUIET_FAIL {
                return false;
            }
        }
        if n.refused() && !n.refused_family() {
            return false;
        }
        k += 1;
    }

    // 同一个 ENOSPC、同一个落点，两族差一个码——Code 维最常被认错的一格
    if truth(3).code != EXIT_TROUBLE {
        return false;
    }
    if truth(6).code != EXIT_QUIET_FAIL {
        return false;
    }
    if SHAPE_SPILL3_ENOSPC.flavor == SHAPE_BUSY_SPILL.flavor {
        return false;
    }
    if truth(3).note != Note::SpillWriteGnu || truth(6).note != Note::SpillWriteBusy {
        return false;
    }
    // 同一个落点、同一个 28，两族结论只差在退码与文案上（其余四维逐字相等）
    let gnu_spill = define(&SHAPE_SPILL3_ENOSPC);
    let bb_spill = define(&SHAPE_BUSY_SPILL);
    if gnu_spill.code != EXIT_TROUBLE || bb_spill.code != EXIT_QUIET_FAIL {
        return false;
    }
    if gnu_spill.stage != bb_spill.stage
        || gnu_spill.value != bb_spill.value
        || gnu_spill.default != bb_spill.default
    {
        return false;
    }
    if gnu_spill.note == bb_spill.note {
        return false;
    }
    if gnu_spill.fold(FNV_OFF) == bb_spill.fold(FNV_OFF) {
        return false;
    }

    // 同一份乱序输入：`-c` 退 2 打行，`-C` 退 1 沉默
    if define(&PROBE_DISORDER).code != EXIT_TROUBLE {
        return false;
    }
    if truth(7).code != EXIT_QUIET_FAIL {
        return false;
    }
    if define(&PROBE_DISORDER).note == truth(7).note {
        return false;
    }

    // busybox 从不退 2（语料与场景里所有 busybox 记录都如此）
    let mut i: usize = 0;
    let mut busy_slots: usize = 0;
    while i < CORPUS {
        let sh = match SHAPES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let o = truth(i);
        if o.code != EXIT_OK && !in_table(&EXIT_SET, o.code) {
            return false;
        }
        if o.ok() != (o.code == EXIT_OK) {
            return false;
        }
        if sh.flavor == Flavor::Busybox {
            busy_slots = busy_slots.saturating_add(1);
            if o.code == EXIT_TROUBLE {
                return false;
            }
        }
        if o.refused() {
            if o.code != EXIT_UNJUDGED || o.default {
                return false;
            }
        }
        i += 1;
    }
    if busy_slots != 1 {
        return false;
    }

    // 场景侧：拒收/红线一律 99，异因仍按家族阶梯
    let mut s2: usize = 0;
    while s2 < SCENE_COUNT {
        let (sh, want) = match scene_at(s2) {
            Some(v) => v,
            None => return false,
        };
        let o = define(&sh);
        if o.note.refused() && o.code != EXIT_UNJUDGED {
            return false;
        }
        if o.note == Note::ForeignErr && o.code != sh.flavor.trouble_exit() {
            return false;
        }
        if want.code != EXIT_OK && !in_table(&EXIT_SET, want.code) {
            return false;
        }
        s2 += 1;
    }
    if define(&SCENE_EDQUOT_SPILL).code != EXIT_TROUBLE {
        return false;
    }
    if define(&SCENE_DIR_BAD).code != EXIT_TROUBLE {
        return false;
    }
    if define(&SCENE_PART_GONE).code != EXIT_TROUBLE {
        return false;
    }
    if define(&SCENE_BUSY_TFLAG).code != EXIT_QUIET_FAIL {
        return false;
    }
    if define(&SCENE_MEMORY_LIE).code != EXIT_UNJUDGED {
        return false;
    }
    if define(&PROBE_MEM_ENOSPC).code != EXIT_UNJUDGED {
        return false;
    }
    if define(&PROBE_MEM_ENOSPC).note != Note::MemoryLie {
        return false;
    }
    if define(&SCENE_CHECK_EMITTED).code != EXIT_UNJUDGED {
        return false;
    }

    // 观测码不参与定性：把 `code_seen` 改成 0 或 99，结论逐维不动
    let blind = Shape {
        code_seen: EXIT_OK,
        ..SHAPE_SPILL3_ENOSPC
    };
    if define(&blind) != truth(3) {
        return false;
    }
    let blind2 = Shape {
        code_seen: EXIT_UNJUDGED,
        ..SHAPE_SPILL3_ENOSPC
    };
    if define(&blind2) != define(&SHAPE_SPILL3_ENOSPC) {
        return false;
    }
    // 但观测码进指纹：篡改现场留痕在取证侧，不改判
    if blind.fold(FNV_OFF) == SHAPE_SPILL3_ENOSPC.fold(FNV_OFF) {
        return false;
    }
    if blind2.fold(FNV_OFF) == blind.fold(FNV_OFF) {
        return false;
    }

    // 申报把 GNU 现场的 2 记成 busybox 档的 1：只挂 Code 一维
    if verdict_at(10) != Some(Fault::Code) {
        return false;
    }
    let swapped = match CLAIMS.get(10) {
        Some(v) => *v,
        None => return false,
    };
    if swapped.code != EXIT_QUIET_FAIL || truth(10).code != EXIT_TROUBLE {
        return false;
    }
    if swapped.stage != truth(10).stage || swapped.note != truth(10).note {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 谓词 10：矩阵分档与渲染
// ---------------------------------------------------------------------------

/// 四档矩阵复核：档位归序、两条计数口径闭合、空行不判完美、渲染逐字。
fn matrix_ok() -> bool {
    // 空矩阵：四档就位、三计数全 0、空行永远不完美
    let m0 = Matrix::new();
    if m0.rows.len() != TIER_COUNT {
        return false;
    }
    if m0.total != 0 || m0.synced != 0 || m0.matched != 0 {
        return false;
    }
    if m0.aligned() != 0 || m0.occupied() != 0 || m0.perfect_rows() != 0 {
        return false;
    }
    if !m0.covered() {
        return false;
    }
    let mut k: usize = 0;
    while k < TIER_COUNT {
        let t = match TIER_ALL.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let r = match m0.row(t) {
            Some(v) => v,
            None => return false,
        };
        if !r.empty() || r.perfect() || r.miss() != 0 || r.tier != t {
            return false;
        }
        if m0.count_of(t) != 0 || m0.good_of(t) != 0 {
            return false;
        }
        if r.render().as_str() != empty_row_text(t) {
            return false;
        }
        k += 1;
    }
    if m0.row(Tier::InMem) != Some(Row::of(Tier::InMem)) {
        return false;
    }
    if Tier::from_index(TIER_COUNT).is_some() {
        return false;
    }

    // 一槽投空档：占位但不完美
    let mut m1 = Matrix::new();
    m1.push_actual(Tier::Degraded, false);
    if m1.covered() != true || m1.aligned() != 1 || m1.total != 1 {
        return false;
    }
    if m1.occupied() != 1 || m1.perfect_rows() != 0 {
        return false;
    }
    if m1.count_of(Tier::Degraded) != 1 || m1.good_of(Tier::Degraded) != 0 {
        return false;
    }
    if m1.matched != 0 {
        return false;
    }
    let dr = match m1.row(Tier::Degraded) {
        Some(v) => v,
        None => return false,
    };
    if dr.miss() != 1 || dr.perfect() {
        return false;
    }
    m1.push_actual(Tier::Degraded, true);
    if m1.count_of(Tier::Degraded) != 2 || m1.good_of(Tier::Degraded) != 1 {
        return false;
    }
    if m1.perfect_rows() != 0 || m1.matched != 1 {
        return false;
    }
    m1.push_actual(Tier::Degraded, true);
    if m1.perfect_rows() != 1 || m1.occupied() != 1 {
        return false;
    }

    // 两条口径必须同时闭合：只加投槽数不进档 = 账目不平
    m1.desync();
    if m1.covered() {
        return false;
    }
    if m1.aligned() != 3 || m1.total != 4 || m1.synced != 3 {
        return false;
    }

    // 全语料矩阵：14 槽 4 档全命中
    let mc = Matrix::from_cases();
    if mc.total as usize != CORPUS || mc.aligned() != CORPUS as u32 {
        return false;
    }
    if mc.synced as usize != CORPUS || mc.matched as usize != CORPUS {
        return false;
    }
    if !mc.covered() || mc.perfect_rows() as usize != TIER_COUNT || mc.occupied() as usize != TIER_COUNT
    {
        return false;
    }
    if mc.count_of(Tier::InMem) != 3 || mc.good_of(Tier::InMem) != 3 {
        return false;
    }
    if mc.count_of(Tier::Spilled) != 1 || mc.good_of(Tier::Spilled) != 1 {
        return false;
    }
    if mc.count_of(Tier::Degraded) != 9 || mc.good_of(Tier::Degraded) != 9 {
        return false;
    }
    if mc.count_of(Tier::Refused) != 1 || mc.good_of(Tier::Refused) != 1 {
        return false;
    }
    let txt = mc.render();
    if !has_sub(txt.as_str(), "in-memory:3/3") {
        return false;
    }
    if !has_sub(txt.as_str(), "spilled:1/1") {
        return false;
    }
    if !has_sub(txt.as_str(), "degraded:9/9") {
        return false;
    }
    if !has_sub(txt.as_str(), "refused:1/1") {
        return false;
    }
    if !has_sub(txt.as_str(), "aligned=14") {
        return false;
    }
    if !has_sub(txt.as_str(), "total=14") {
        return false;
    }

    // 归序规则：先看锚位，再看成败，剩下的全进拒收档
    if Tier::of(&Outcome::new(Stage::Settle, EXIT_OK, 8, Note::DoneFull, false)) != Tier::InMem {
        return false;
    }
    if Tier::of(&Outcome::new(Stage::Settle, EXIT_OK, 100, Note::DoneSpilled, false))
        != Tier::Spilled
    {
        return false;
    }
    if Tier::of(&Outcome::new(Stage::Settle, EXIT_OK, 12, Note::DoneMerge, false)) != Tier::InMem {
        return false;
    }
    if Tier::of(&Outcome::new(Stage::Settle, EXIT_OK, 0, Note::DoneCheckOk, false)) != Tier::InMem {
        return false;
    }
    // 锚位优先：成功档一旦宣称落降级锚，就归降级档（谎报的归序也照此）
    if Tier::of(&Outcome::new(Stage::Settle, EXIT_OK, 8, Note::DoneFull, true)) != Tier::Degraded {
        return false;
    }
    if Tier::of(&Outcome::new(Stage::Buffer, EXIT_UNJUDGED, 0, Note::MemoryLie, false))
        != Tier::Refused
    {
        return false;
    }
    if Tier::of(&Outcome::new(Stage::Input, EXIT_QUIET_FAIL, 0, Note::DisorderedQuiet, false))
        != Tier::Refused
    {
        return false;
    }
    if Tier::of(&Outcome::new(Stage::Merge, EXIT_TROUBLE, 0, Note::ForeignErr, false))
        != Tier::Refused
    {
        return false;
    }
    if Tier::InMem.anchors() || Tier::Spilled.anchors() || Tier::Refused.anchors() {
        return false;
    }
    if !Tier::Degraded.anchors() {
        return false;
    }

    // 槽位归档与事实面逐槽一致（第三形态不许与算法漂移）
    let mut i: usize = 0;
    while i < CORPUS {
        let want = match ACT_TIER.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if cover(i) != Some(want) {
            return false;
        }
        if Tier::of_slot(i) != Some(want) {
            return false;
        }
        if Tier::of(&truth(i)) != want {
            return false;
        }
        i += 1;
    }
    if Tier::of_slot(CORPUS).is_some() {
        return false;
    }
    if !tiers_all_covered() {
        return false;
    }

    // 降级档与锚位互推：能落锚的槽恰是那九个，其余一律不落
    let mut anchors: usize = 0;
    let mut j: usize = 0;
    while j < CORPUS {
        let o = truth(j);
        let t = match ACT_TIER.get(j) {
            Some(v) => *v,
            None => return false,
        };
        if (t == Tier::Degraded) != o.default {
            return false;
        }
        if o.default {
            anchors = anchors.saturating_add(1);
            if !o.note.space_family() {
                return false;
            }
            if o.stage != Stage::Tmpdir && o.stage != Stage::Spill && o.stage != Stage::Output {
                return false;
            }
            if o.code != EXIT_TROUBLE && o.code != EXIT_QUIET_FAIL {
                return false;
            }
        }
        j += 1;
    }
    if anchors != 9 {
        return false;
    }

    // 档位指纹各自不同（矩阵分桶的正确性依赖档与档不碰撞）
    let fa = Tier::InMem.fold(FNV_OFF);
    let fb = Tier::Spilled.fold(FNV_OFF);
    let fc = Tier::Degraded.fold(FNV_OFF);
    let fd = Tier::Refused.fold(FNV_OFF);
    if fa == fb || fa == fc || fa == fd || fb == fc || fb == fd || fc == fd {
        return false;
    }
    let mut n: usize = 0;
    while n < TIER_COUNT {
        let t = match TIER_ALL.get(n) {
            Some(v) => *v,
            None => return false,
        };
        if t.index() != n {
            return false;
        }
        if Tier::from_index(n) != Some(t) {
            return false;
        }
        if t.slug().is_empty() || t.name().is_empty() {
            return false;
        }
        n += 1;
    }
    if !strs_unique(&[
        Tier::InMem.slug(),
        Tier::Spilled.slug(),
        Tier::Degraded.slug(),
        Tier::Refused.slug(),
    ]) {
        return false;
    }
    if !strs_unique(&[
        Tier::InMem.name(),
        Tier::Spilled.name(),
        Tier::Degraded.name(),
        Tier::Refused.name(),
    ]) {
        return false;
    }
    true
}

/// 空行的逐字渲染（档位短名 + `0/0`），矩阵与报表共用。
fn empty_row_text(tier: Tier) -> String {
    let mut s = String::from(tier.slug());
    s.push(':');
    s.push_str(dec_text(0).as_str());
    s.push('/');
    s.push_str(dec_text(0).as_str());
    s
}

// ---------------------------------------------------------------------------
// 谓词 11：全绿运行与红档率
// ---------------------------------------------------------------------------

/// 全语料运行复核：一次跑完必须全绿、渲染逐字等于 `RENDER_OK`；
/// 任意一槽被改判必须正好掉到 929（红档率标定），且出口闸门判红。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if r.total != CORPUS || r.accept != ACCEPT || r.reject != REJECT {
        return false;
    }
    if r.matched != CORPUS {
        return false;
    }
    if r.rate() != RATE_FULL || !r.green() || r.gate() != Ok(()) {
        return false;
    }
    if r.tier_sum() != CORPUS {
        return false;
    }
    if r.in_mem != 3 || r.spilled != 1 || r.degraded != 9 || r.refused != 1 {
        return false;
    }
    if r.tier_count(Tier::InMem) != 3 || r.tier_count(Tier::Spilled) != 1 {
        return false;
    }
    if r.tier_count(Tier::Degraded) != 9 || r.tier_count(Tier::Refused) != 1 {
        return false;
    }
    if r.is_red_calibrated() {
        return false;
    }

    // 逐字渲染：这一串是本卡对外唯一的整体结论
    if r.render().as_str() != RENDER_OK {
        return false;
    }
    if Report::RENDER_OK != RENDER_OK {
        return false;
    }
    if !starts_with(RENDER_OK, "sort2s total=14") {
        return false;
    }
    if !has_sub(RENDER_OK, "accept=8 reject=6") {
        return false;
    }
    if !has_sub(RENDER_OK, "in-memory=3 spilled=1 degraded=9 refused=1") {
        return false;
    }
    if !has_sub(RENDER_OK, "matched=14 rate=1000 verdict=green") {
        return false;
    }
    if has_sub(RENDER_OK, "verdict=red") {
        return false;
    }

    // 运行可重入：两次跑出的报表逐字段相等
    let r2 = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if r2.render().as_str() != r.render().as_str() {
        return false;
    }

    // 注入一格期望之外的结论：命中数正好掉 1，千分率正好 929
    let one = match run_with(Some((13usize, Fault::Ok))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if one.matched + 1 != CORPUS {
        return false;
    }
    if one.rate() != RED_RATE || one.green() || !one.is_red_calibrated() {
        return false;
    }
    if one.gate() != Err(ST_ESTATE) {
        return false;
    }
    let one_txt = one.render();
    if !has_sub(one_txt.as_str(), "matched=13") {
        return false;
    }
    if !has_sub(one_txt.as_str(), "rate=929") {
        return false;
    }
    if !has_sub(one_txt.as_str(), "verdict=red") {
        return false;
    }
    if one_txt.as_str() == RENDER_OK {
        return false;
    }

    // 红档率与注入的是哪一槽、哪一维都无关：十四槽逐槽标定，一律 929
    let mut i: usize = 0;
    while i < CORPUS {
        let want = want_fault(i);
        let inj = if want == Fault::Value {
            Fault::Estate
        } else {
            Fault::Value
        };
        let rr = match run_with(Some((i, inj))) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if rr.matched + 1 != rr.total || rr.rate() != RED_RATE {
            return false;
        }
        if rr.green() || !rr.is_red_calibrated() {
            return false;
        }
        // 注入点越界必须当场拒绝（越界不是"少命中一格"，而是整份报表作废）
        if run_with(Some((CORPUS, inj))) != Err(ST_ESLOT) {
            return false;
        }
        i += 1;
    }

    // 注入成**期望**的那一维 = 什么都没改，报表照旧全绿
    let green_txt = r.render();
    if run_with(Some((0usize, Fault::Ok)))
        .map(|v| v.render())
        .ok()
        != Some(green_txt.clone())
    {
        return false;
    }
    if run_with(Some((8usize, Fault::Estate)))
        .map(|v| v.render())
        .ok()
        != Some(green_txt.clone())
    {
        return false;
    }
    if run_with(None).map(|v| v.render()).ok() != Some(green_txt) {
        return false;
    }

    // 千分率的取整口径：四舍五入到千分位（14 格 → 半格是 7）
    let half = Report {
        matched: 7,
        ..r
    };
    if half.rate() != 500 {
        return false;
    }
    let zero = Report {
        total: 0,
        matched: 0,
        ..r
    };
    if zero.rate() != 0 || zero.gate() != Err(ST_ESLOT) {
        return false;
    }

    // 出口闸门的五个分支各抓一种账目不平
    let short = Report { total: 13, ..r };
    if short.gate() != Err(ST_ESLOT) {
        return false;
    }
    let unbal = Report { reject: 5, ..r };
    if unbal.gate() != Err(ST_ECOUNT) {
        return false;
    }
    let lopsided = Report { in_mem: 2, ..r };
    if lopsided.gate() != Err(ST_EBAL) {
        return false;
    }
    let missed = Report { matched: 13, ..r };
    if missed.gate() != Err(ST_ESTATE) || missed.rate() != RED_RATE || missed.green() {
        return false;
    }
    if !has_sub(missed.render().as_str(), "verdict=red") {
        return false;
    }
    if r.gate() != Ok(()) {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 谓词 12a：语料纪律
// ---------------------------------------------------------------------------

/// 两份申报之间有几维不同（死槽的"一槽只坏一维"用它来钉）。
fn claim_diff(a: &Claim, b: &Claim) -> usize {
    let mut n: usize = 0;
    if a.success != b.success {
        n = n.saturating_add(1);
    }
    if a.stage != b.stage {
        n = n.saturating_add(1);
    }
    if a.code != b.code {
        n = n.saturating_add(1);
    }
    if a.value != b.value {
        n = n.saturating_add(1);
    }
    if a.note != b.note {
        n = n.saturating_add(1);
    }
    if a.default != b.default {
        n = n.saturating_add(1);
    }
    n
}

/// 语料纪律复核：表宽、活死配比、死槽复用活形态、一槽只坏一维、
/// 判序逐槽命中期望维、期望表与申报表两形合一。
fn corpus_ok() -> bool {
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS {
        return false;
    }
    if slot_count() != CORPUS || CORPUS != ACCEPT + REJECT {
        return false;
    }
    if WANT_TAGS.len() != CORPUS || WANT_NDX.len() != CORPUS {
        return false;
    }
    if ACT_STAGE.len() != CORPUS
        || ACT_CODE.len() != CORPUS
        || ACT_VALUE.len() != CORPUS
        || ACT_NOTE.len() != CORPUS
        || ACT_DEFAULT.len() != CORPUS
        || ACT_OK.len() != CORPUS
    {
        return false;
    }
    if ACT_LOCUS.len() != CORPUS || ACT_LINES.len() != CORPUS || ACT_MADE.len() != CORPUS {
        return false;
    }
    if ACT_SPILLS.len() != CORPUS || ACT_LEFT.len() != CORPUS || ACT_TIER.len() != CORPUS {
        return false;
    }
    if LIVE_OK.len() != ACCEPT || DEAD_FROM.len() != REJECT || REJECT_FAULTS.len() != REJECT {
        return false;
    }

    // 标签：一槽一个，不许重名，也不许越界回读
    if !strs_unique(&WANT_TAGS) {
        return false;
    }
    if tag_at(CORPUS) != "越界" || scene_name(SCENE_COUNT) != "越界" {
        return false;
    }
    if stat_meaning(ACCEPT) != "越界" {
        return false;
    }
    let mut t: usize = 0;
    while t < CORPUS {
        if tag_at(t).is_empty() || tag_at(t).len() < 4 {
            return false;
        }
        t += 1;
    }

    // 十四槽逐槽判定等于期望维；活槽全合、死槽各挂一维
    if !all_claims_land() || !want_matches_claims() || !act_matches_define() {
        return false;
    }
    let vs = verdicts();
    if vs.len() != CORPUS {
        return false;
    }
    let mut i: usize = 0;
    while i < CORPUS {
        let got = match vs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if got != want_fault(i) {
            return false;
        }
        if verdict_at(i) != Some(got) {
            return false;
        }
        if i < ACCEPT && got != Fault::Ok {
            return false;
        }
        if i >= ACCEPT && !got.is_fault() {
            return false;
        }
        i += 1;
    }

    // 六个死槽的缺欠维与 `REJECT_FAULTS`/`REJECT_CODES` 一一对位
    let mut k: usize = 0;
    while k < REJECT {
        let f = match REJECT_FAULTS.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let c = match REJECT_CODES.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if want_fault(ACCEPT + k) != f || f.code() != c {
            return false;
        }
        if f == Fault::Ok {
            return false;
        }
        let off = code_offset(c);
        if off < 1 || off > CODE_COUNT as i32 {
            return false;
        }
        if !in_table(&ALL_CODES, c) {
            return false;
        }
        let mut d2: usize = k.saturating_add(1);
        while d2 < REJECT {
            let c2 = match REJECT_CODES.get(d2) {
                Some(v) => *v,
                None => return false,
            };
            if c == c2 {
                return false;
            }
            let f2 = match REJECT_FAULTS.get(d2) {
                Some(v) => *v,
                None => return false,
            };
            if f == f2 {
                return false;
            }
            d2 += 1;
        }
        k += 1;
    }
    if REJECT_CODES.len() != REJECT {
        return false;
    }
    if !in_table(&REJECT_CODES, ST_ESTATE) || !in_table(&REJECT_CODES, ST_ESTAGE) {
        return false;
    }
    if in_table(&REJECT_CODES, ST_EINMEM) || in_table(&REJECT_CODES, ST_EINPUT) {
        return false;
    }
    if !in_table(&REJECT_CODES, ST_EANCH) || !in_table(&REJECT_CODES, ST_ENOTE) {
        return false;
    }

    // 死槽的事实面**整槽复用**活槽：坏的是申报，不是现场
    let mut d: usize = 0;
    while d < REJECT {
        let src = match DEAD_FROM.get(d) {
            Some(v) => *v,
            None => return false,
        };
        if src >= ACCEPT {
            return false;
        }
        let ds = match SHAPES.get(ACCEPT + d) {
            Some(v) => *v,
            None => return false,
        };
        let src_shape = match SHAPES.get(src) {
            Some(v) => *v,
            None => return false,
        };
        if ds != src_shape {
            return false;
        }
        d += 1;
    }
    let dead0 = match DEAD_FROM.get(0) {
        Some(v) => *v,
        None => return false,
    };
    if dead0 != ACCEPT {
        return false;
    }

    // 后五个死槽各只翻一维；第一个死槽翻的是成败位（连带整套失败面）
    let mut e: usize = 1;
    while e < REJECT {
        let idx = ACCEPT + e;
        let o = truth(idx);
        let base = Claim::from_outcome(&o, o.ok());
        let c = match CLAIMS.get(idx) {
            Some(v) => *v,
            None => return false,
        };
        if claim_diff(&base, &c) != 1 {
            return false;
        }
        if verdict_at(idx) != Some(want_fault(idx)) {
            return false;
        }
        e += 1;
    }
    let estate_truth = truth(ACCEPT);
    let estate_base = Claim::from_outcome(&estate_truth, estate_truth.ok());
    let estate_claim = match CLAIMS.get(ACCEPT) {
        Some(v) => *v,
        None => return false,
    };
    if estate_truth.ok() != true || estate_claim.success != false {
        return false;
    }
    // 申报本身是**另一份现场**的忠实结论：谎报的是"张冠李戴"，不是随手改数
    let other = define(&SHAPE_SPILL3_ENOSPC);
    if estate_claim != Claim::from_outcome(&other, false) {
        return false;
    }
    if claim_diff(&estate_base, &estate_claim) != 6 {
        return false;
    }
    if estate_base.stage != Stage::Settle || estate_base.value != 8 {
        return false;
    }

    // 申报次序的单调索引：活槽全 0，死槽 1..=6 严格递增
    let mut prev: u8 = 0;
    let mut w: usize = 0;
    while w < CORPUS {
        let ndx = match WANT_NDX.get(w) {
            Some(v) => *v,
            None => return false,
        };
        if w < ACCEPT && ndx != 0 {
            return false;
        }
        if w >= ACCEPT && ndx != (w as u8) - (ACCEPT as u8) + 1 {
            return false;
        }
        if ndx > prev {
            prev = ndx;
        } else if w >= ACCEPT {
            return false;
        }
        w += 1;
    }
    if prev != REJECT as u8 {
        return false;
    }

    // 事实面第三形态逐槽对账（行数/件数/残留都是现场量，不是结论）
    let mut m: usize = 0;
    while m < CORPUS {
        let sh = match SHAPES.get(m) {
            Some(v) => *v,
            None => return false,
        };
        let p = plan(sh);
        let want_lines = match ACT_LINES.get(m) {
            Some(v) => *v,
            None => return false,
        };
        let want_made = match ACT_MADE.get(m) {
            Some(v) => *v,
            None => return false,
        };
        let want_spills = match ACT_SPILLS.get(m) {
            Some(v) => *v,
            None => return false,
        };
        let want_left = match ACT_LEFT.get(m) {
            Some(v) => *v,
            None => return false,
        };
        let want_locus = match ACT_LOCUS.get(m) {
            Some(v) => *v,
            None => return false,
        };
        if want_lines != sh.lines || want_made != sh.made || want_left != sh.left {
            return false;
        }
        if want_spills != p.spills || want_locus != sh.locus {
            return false;
        }
        if want_lines > LINES_MAX || p.bytes > BYTES_MAX {
            return false;
        }
        m += 1;
    }

    // 成败配比：活槽 3 成 5 败；全表 4 成 10 败；该落锚 9 槽
    if count_where(&LIVE_OK, true) != 3 || count_where(&LIVE_OK, false) != 5 {
        return false;
    }
    if count_where(&ACT_OK, true) != 4 || count_where(&ACT_OK, false) != 10 {
        return false;
    }
    if count_where(&WANT_OK, true) != 3 {
        return false;
    }
    if count_where(&ACT_DEFAULT, true) != 9 || count_where(&ACT_DEFAULT, false) != 5 {
        return false;
    }
    let mut q: usize = 0;
    while q < ACCEPT {
        let live = match LIVE_OK.get(q) {
            Some(v) => *v,
            None => return false,
        };
        let declared = match WANT_OK.get(q) {
            Some(v) => *v,
            None => return false,
        };
        let stored = match CLAIMS.get(q) {
            Some(v) => v.success,
            None => return false,
        };
        if live != truth(q).ok() {
            return false;
        }
        if declared != stored {
            return false;
        }
        q += 1;
    }

    // 期望表的两个形态与申报表合一（第二形态不许与 `CLAIMS` 漂移）
    let mut z: usize = 0;
    while z < CORPUS {
        let rebuilt = match claim_from_want(z) {
            Some(v) => v,
            None => return false,
        };
        let stored = match CLAIMS.get(z) {
            Some(v) => *v,
            None => return false,
        };
        if rebuilt != stored {
            return false;
        }
        if want_value(z) != Some(stored.value) {
            return false;
        }
        if expected(z).is_none() || verdict_at(z).is_none() {
            return false;
        }
        z += 1;
    }

    // 越界一律 `None`/哨兵，不许 panic、也不许静默回绕
    if shape_at(CORPUS).is_some()
        || claim_at(CORPUS).is_some()
        || case_at(CORPUS).is_some()
        || want_value(CORPUS).is_some()
        || claim_from_want(CORPUS).is_some()
        || expected(CORPUS).is_some()
        || cover(CORPUS).is_some()
        || verdict_at(CORPUS).is_some()
        || scene_at(SCENE_COUNT).is_some()
    {
        return false;
    }
    if want_fault(CORPUS) != Fault::Ok {
        return false;
    }
    if let Some(c) = case_at(0) {
        if fault_at(&c, CORPUS, None) != Err(ST_ENOBS) {
            return false;
        }
        if fault_at(&c, 0, Some((0usize, Fault::Note))) != Ok(Fault::Note) {
            return false;
        }
        if fault_at(&c, 1, Some((0usize, Fault::Note))) != Ok(judge(&c)) {
            return false;
        }
    } else {
        return false;
    }

    // 十四槽指纹互不重复：两槽现场+申报全等就说明有槽在凑数
    let mut fs: [u64; CORPUS] = [0u64; CORPUS];
    let mut a1: usize = 0;
    while a1 < CORPUS {
        let c = match case_at(a1) {
            Some(v) => v,
            None => return false,
        };
        if let Some(cell) = fs.get_mut(a1) {
            *cell = c.fold(FNV_OFF);
        } else {
            return false;
        }
        let mut b1: usize = a1.saturating_add(1);
        while b1 < CORPUS {
            let x = match fs.get(a1) {
                Some(v) => *v,
                None => return false,
            };
            let y = match fs.get(b1) {
                Some(v) => *v,
                None => return false,
            };
            if x == y {
                return false;
            }
            b1 += 1;
        }
        a1 += 1;
    }

    // 渲染面：申报与结论各一行，短名互不串档
    let c3 = match CLAIMS.get(3) {
        Some(v) => *v,
        None => return false,
    };
    let c3r = c3.render();
    if !has_sub(c3r.as_str(), "claim ok=false") {
        return false;
    }
    if !has_sub(c3r.as_str(), "note=gnu-spill-write") {
        return false;
    }
    if !has_sub(c3r.as_str(), "anchor=true") {
        return false;
    }
    let o3 = match SHAPES.get(3) {
        Some(v) => define(v).render(),
        None => return false,
    };
    if !has_sub(o3.as_str(), "stage=spill exit=2 emitted=0") {
        return false;
    }
    if c3.exit_hint() != EXIT_TROUBLE || Claim::new(true, Stage::Args, 0, 0, Note::DoneFull, false).exit_hint()
        != EXIT_OK
    {
        return false;
    }
    let fault_slugs: [&'static str; 7] = [
        Fault::Ok.slug(),
        Fault::Estate.slug(),
        Fault::Stage.slug(),
        Fault::Code.slug(),
        Fault::Value.slug(),
        Fault::Note.slug(),
        Fault::Default.slug(),
    ];
    if !strs_unique(&fault_slugs) {
        return false;
    }
    let mut z1: usize = 0;
    while z1 < fault_slugs.len() {
        let one = match fault_slugs.get(z1) {
            Some(v) => *v,
            None => return false,
        };
        if one.is_empty() {
            return false;
        }
        if let Some(f) = Fault::ALL.get(z1) {
            if f.slug() != one {
                return false;
            }
        } else {
            return false;
        }
        z1 += 1;
    }
    let mut fb: usize = 0;
    let mut blurb_empty: usize = 0;
    while fb < Fault::ALL.len() {
        let f = match Fault::ALL.get(fb) {
            Some(v) => *v,
            None => return false,
        };
        if f.blurb().is_empty() {
            blurb_empty = blurb_empty.saturating_add(1);
        }
        if f.is_fault() == (f == Fault::Ok) {
            return false;
        }
        if f.rank() != fb as u32 {
            return false;
        }
        fb += 1;
    }
    if blurb_empty != 0 || Fault::ALL.len() != 7 {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 谓词 12b：账本双审计
// ---------------------------------------------------------------------------

/// 账本复核：登记次序、格型自洽、审计两套口径、封印与冻结、重放独立复算。
fn ledger_ok() -> bool {
    // 空账：什么都不许读，也不许重放
    let fresh = Ledger::new();
    if !fresh.is_empty() || fresh.len() != 0 || fresh.root() != 0 {
        return false;
    }
    if fresh.is_sealed() || fresh.is_frozen() || fresh.rejects() != 0 {
        return false;
    }
    if fresh.audit() != Err(ST_ESLOT) {
        return false;
    }
    if fresh.replay() != Err(ST_ESTATE) {
        return false;
    }
    if fresh.sample(0).is_some() {
        return false;
    }

    // 登记次序：槽号必须等于当前长度（跳号 = 越界）
    let mut probe = Ledger::new();
    if probe.register(Sample::stat(1, 8)) != Err(ST_ESLOT) {
        return false;
    }
    if probe.len() != 0 || probe.rejects() != 1 {
        return false;
    }
    // 自相矛盾的一格：成败位与码互不互推 → 当场拒入（连账都不上）
    if probe.register(Sample::stat(0, 0)) != Err(ST_ECOUNT) {
        return false;
    }
    if probe.register(contradictory_sample()) != Err(ST_ESLOT) {
        return false;
    }
    if probe.rejects() != 2 || probe.len() != 0 {
        return false;
    }
    if probe.register(Sample::stat(0, CORPUS as u32)) != Ok(()) {
        return false;
    }
    if probe.len() != 1 || probe.rejects() != 2 {
        return false;
    }
    // 缺陷码不在 `REJECT_CODES` 里也照样能入账——它是**审计**该抓的，不是登记该抓的
    let mut audit_only = Ledger::new();
    if audit_only.register(Sample::defect(0, ST_EINPUT)) != Ok(()) {
        return false;
    }
    if audit_only.audit() != Err(ST_ESLOT) {
        return false;
    }

    // 格型自洽性的正反两形
    if !Sample::stat(0, 1).self_consistent() || !Sample::rate(0).self_consistent() {
        return false;
    }
    if !Sample::defect(0, ST_ESTAGE).self_consistent() {
        return false;
    }
    if Sample::stat(0, 0).self_consistent() {
        return false;
    }
    if Sample::rate(0).value != RED_RATE {
        return false;
    }
    if contradictory_sample().self_consistent() {
        return false;
    }
    let ghost = Sample {
        slot: 0,
        op: 0,
        ok: true,
        code: 0,
        value: 1,
    };
    if ghost.kind() != "illegal" || ghost.self_consistent() {
        return false;
    }
    if Sample::stat(0, 1).kind() != "stat" {
        return false;
    }
    if Sample::rate(0).kind() != "rate" || Sample::defect(0, ST_ESTAGE).kind() != "defect" {
        return false;
    }
    if OP_STAT == OP_RATE || OP_STAT == OP_DEFECT || OP_RATE == OP_DEFECT {
        return false;
    }
    if OP_BITS.len() != OP_COUNT {
        return false;
    }
    let mut z: usize = 0;
    while z < OP_BITS.len() {
        let v = match OP_BITS.get(z) {
            Some(x) => *x,
            None => return false,
        };
        if v == OP_STAT || v == OP_RATE || v == OP_DEFECT {
            return false;
        }
        if in_table(&OP_BITS.map(|x| x as u32), v as u32) != true {
            return false;
        }
        z += 1;
    }

    // 满账：14 格 = 7 统计 + 1 红档 + 6 缺陷
    let mut l = build_ledger();
    if l.len() != SAMPLES || l.rejects() != 0 || l.is_sealed() {
        return false;
    }
    if l.ok_count() != ACCEPT || l.err_count() != REJECT {
        return false;
    }
    if l.count_op(OP_STAT) != STAT_CELLS {
        return false;
    }
    if l.count_op(OP_RATE) != 1 || l.count_op(OP_DEFECT) != REJECT {
        return false;
    }
    if l.sample(SAMPLES).is_some() {
        return false;
    }
    if l.audit() != Ok(()) {
        return false;
    }
    let mut i: usize = 0;
    while i < l.len() {
        let s = match l.sample(i) {
            Some(v) => v,
            None => return false,
        };
        if s.slot as usize != i || !s.self_consistent() {
            return false;
        }
        if i < STAT_CELLS {
            let want_v = match VALS.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let check_v = match VALS_CHECK.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if s.op != OP_STAT || s.value != want_v {
                return false;
            }
            if s.value != check_v || want_v != check_v {
                return false;
            }
        } else if i == STAT_CELLS {
            if s.op != OP_RATE || s.value != RED_RATE {
                return false;
            }
        } else {
            let j = i - STAT_CELLS.saturating_add(1);
            let want_c = match REJECT_CODES.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if s.op != OP_DEFECT || s.value != 0 || s.ok {
                return false;
            }
            if s.code != want_c || code_offset(s.code) < 1 {
                return false;
            }
        }
        i += 1;
    }

    // 满账之后仍然不许跳号，也不许塞进自相矛盾的一格
    if l.register(Sample::stat(SAMPLES, 1)) != Err(ST_ESLOT) {
        return false;
    }
    if l.register(contradictory_sample()) != Err(ST_ECOUNT) {
        return false;
    }
    if l.rejects() != 2 || l.len() != SAMPLES {
        return false;
    }

    // 封印与冻结：根指纹非 0、可重放、幂等
    let root = l.seal();
    if root == 0 || !l.is_sealed() || l.root() != root {
        return false;
    }
    if l.is_frozen() {
        return false;
    }
    if l.seal() != root {
        return false;
    }
    if l.replay() != Ok(root) {
        return false;
    }
    if l.freeze() != root || !l.is_frozen() {
        return false;
    }
    if l.freeze() != root {
        return false;
    }
    if l.register(Sample::rate(0)) != Err(ST_ESTATE) {
        return false;
    }
    if l.rejects() != 3 || l.len() != SAMPLES {
        return false;
    }

    // 渲染与指纹：同账同指纹，改一格就变
    let txt = l.render();
    if !has_sub(txt.as_str(), "len=14") {
        return false;
    }
    if !has_sub(txt.as_str(), "ok=8") || !has_sub(txt.as_str(), "err=6") {
        return false;
    }
    if !has_sub(txt.as_str(), "rejects=3") {
        return false;
    }
    if txt.is_empty() {
        return false;
    }
    if build_ledger().fingerprint() != build_ledger().fingerprint() {
        return false;
    }
    if build_ledger().fingerprint() == fresh.fingerprint() {
        return false;
    }
    let mut shifted = build_ledger();
    if let Some(cell) = shifted.items.get_mut(0) {
        cell.value = cell.value.saturating_add(1);
    } else {
        return false;
    }
    if shifted.fingerprint() == build_ledger().fingerprint() {
        return false;
    }
    if shifted.audit() != Err(ST_EEMIT) {
        return false;
    }

    // 两套审计口径：`VALS` 与 `VALS_CHECK` 逐格相等，任何一格漂移都判死
    if VALS.len() != ACCEPT || VALS_CHECK.len() != ACCEPT {
        return false;
    }
    let mut v: usize = 0;
    while v < ACCEPT {
        let a = match VALS.get(v) {
            Some(x) => *x,
            None => return false,
        };
        let b = match VALS_CHECK.get(v) {
            Some(x) => *x,
            None => return false,
        };
        if a != b || a == 0 {
            return false;
        }
        if v < STAT_CELLS && !in_table(&[CORPUS as u32, ACCEPT as u32, REJECT as u32,
            STAGE_COUNT, LOCUS_COUNT as u32, TIER_COUNT as u32, FLAVOR_COUNT as u32], a)
        {
            return false;
        }
        v += 1;
    }
    let last_check = match VALS_CHECK.get(ACCEPT.saturating_sub(1)) {
        Some(x) => *x,
        None => return false,
    };
    if last_check != RED_RATE {
        return false;
    }

    // 三本坏账：值错、身份错、篡改格——审计必须以不同码分开抓住
    if broken_value_ledger().audit() != Err(ST_EEMIT) {
        return false;
    }
    if broken_code_ledger().audit() != Err(ST_ECOUNT) {
        return false;
    }
    if broken_defect_ledger().audit() != Err(ST_EEMIT) {
        return false;
    }
    if broken_value_ledger().len() != SAMPLES || broken_code_ledger().len() != SAMPLES {
        return false;
    }
    if broken_defect_ledger().audit() == broken_code_ledger().audit() {
        return false;
    }
    if !broken_value_ledger().is_empty() || broken_value_ledger().is_frozen() {
        return false;
    }

    // 四注入：表路/判路/渲染路/账本路各自为真，路线名互异
    let inj = inject_four();
    if inj.len() != 4 {
        return false;
    }
    let mut n: usize = 0;
    while n < inj.len() {
        if inj.get(n) != Some(&true) {
            return false;
        }
        n += 1;
    }
    if !strs_unique(&INJECT_ROUTES) || INJECT_ROUTES.len() != inj.len() {
        return false;
    }
    let last_route = match INJECT_ROUTES.get(3) {
        Some(v) => *v,
        None => return false,
    };
    if last_route != "ledger" {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 出口：13 条判据
// ---------------------------------------------------------------------------

/// 本卡自检出口：恰好 13 条判据，一条一闸，名字逐字钉死。
///
/// 第十三条的尾名 `SO13-冻结重放四注入` 是契约要求的固定收尾名，不得改写。
pub fn run_unx11685_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11685_sort_tmpfile_enospc");
    s.add(
        "SO01-名称与段基址",
        name_ok() && anchors_agree(),
        "SEG_BASE=0x6d310，13 码连号，与 F11681/11682/11684 位距逐字成立",
    );
    s.add(
        "SO02-枚举与九闸纵线",
        enums_ok(),
        "九闸秩 1..=9、24 档文案、四档、六缺陷维全部闭合",
    );
    s.add(
        "SO03-落点与目录六态",
        locus_ok(),
        "五落点归一闸，创建档主语是目录、写出档主语是件名",
    );
    s.add(
        "SO04-形态与合法域",
        shape_ok(),
        "现场十五维全进指纹，家族越界与未知选项各自归闸",
    );
    s.add(
        "SO05-诚实闸与落盘规划",
        honesty_ok(),
        "件数 floor((行-1)/容量)，尾段留内存；没落盘就不认盘满",
    );
    s.add(
        "SO06-已吐行分账",
        emit_ok(),
        "只有输出落点可非零，校验档恒 0，残缺行数以行为量",
    );
    s.add(
        "SO07-闸门单调与红线优先",
        gate_ok(),
        "纵线只许进一格；家族>诚实>几何>输入红线>回收",
    );
    s.add(
        "SO08-文案两族逐字",
        message_ok(),
        "GNU 三式带引号主语、busybox 三式不带路径；-C 空文案",
    );
    s.add(
        "SO09-退码阶梯与家族",
        exit_ok(),
        "GNU 麻烦 2、busybox 一律 1、-C 破例 1、本卡拒收 99",
    );
    s.add(
        "SO10-矩阵分档与渲染",
        matrix_ok(),
        "四档 3/1/9/1，两条口径闭合，锚位与降级档互推",
    );
    s.add(
        "SO11-全绿运行与红档率",
        report_ok(),
        "渲染逐字 RENDER_OK，一红即 929，出口闸五分支各有其码",
    );
    s.add(
        "SO12-语料账本双审计",
        corpus_ok() && ledger_ok(),
        "8 活 6 死一槽只坏一维；14 格账本两套口径互检",
    );
    let mut l = build_ledger();
    let audited = l.audit() == Ok(());
    let root = l.freeze();
    let frozen = l.is_frozen() && root != 0;
    let replayed = l.replay() == Ok(root);
    let after_freeze = l.register(Sample::rate(STAT_CELLS)) == Err(ST_ESTATE);
    let inj = inject_four();
    let mut four = true;
    let mut q: usize = 0;
    while q < inj.len() {
        if inj.get(q) != Some(&true) {
            four = false;
        }
        q += 1;
    }
    s.add(
        "SO13-冻结重放四注入",
        audited && frozen && replayed && after_freeze && four,
        "冻结后拒登、重放复算同根，表/判/渲染/账本四路各自为真",
    );
    s
}

//<<CHUNK6C>>
