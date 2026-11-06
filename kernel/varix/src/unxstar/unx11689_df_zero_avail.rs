//! UNX-F11689 df 0 avail 呈现判据
//!
//! 这张卡管的是**"满盘"这件事在报表里长什么样**。`df` 不写一个字节、不分配一个
//! inode、不在盘上留下任何东西；它是**只读的呈现面**。于是本卡的判据不是"这次
//! 调用红不红"（那是 F11688 建件面、F11690 改名面的账），而是**四件互相顶不上
//! 就必有一假的事**：
//!
//! ```text
//! $ df /srv
//! Filesystem     1K-blocks    Used Available Use% Mounted on
//! /dev/sda1      10240000 10240000         0 100% /srv     ← avail 出 0：真满盘呈现
//! $ df /tmp
//! Filesystem     1K-blocks  Used Available Use% Mounted on
//! tmpfs                 -      -         -    - /tmp       ← 四格全横杠：不适用
//! $ df /nope
//! df: cannot stat '/nope': No such file or directory        ← 退出 1，与"满"无关
//! ```
//!
//! 第二行是本卡的心脏：**`-` 不是 `0`**。GNU `df` 在"空间不适用"的文件系统
//! （伪文件系统、不可测的挂载点）上把 Size/Used/Available/Use% **四格一起出 `-`**，
//! 而在真满盘的 ext4 上出的是 `0` 与 `100%`。把 `-` 读成 `0`，监控会在每一个
//! `/tmp`、`/proc`、`/sys` 上报警；把 `0` 读成 `-`，真正的满盘会被读成"这台机器
//! 没报表"。本卡把这两件事立成独立一维（`Note::DashCell` 对 `Note::AvailZero`），
//! 并且让 `value` 在横杠档取一个显式哨兵 `DASH_CELL`——**哨兵不是零**，任何把
//! 横杠折算成 0 的实现都会在 `judge` 第一道就被抓住。
//!
//! 更麻烦的是 **`df` 永远不会因为盘满而失败**：`ENOSPC`(28) 是写侧的 errno，
//! 呈现面一次系统调用都不分配东西，所以 `df: cannot stat 'x': No such space`
//! 这类串在真实机器上**不存在**。把 `df` 的退出码 1 归因给"盘满了"，是本卡点名
//! 判死的第一号假象（`Note::SpaceLie`）。反过来说，`df` 报 `avail 12288` 而写侧
//! 照样吃 28（inode 耗尽、ext4 journal 满、btrfs metadata 块组满）是**真的**，
//! 这一档不叫 df 说谎，叫"呈现面忠实、成因在别的轴上"——本卡给它一个显式的
//! 移交登记（`HandoffMarked`），这正是 F11688 第 5 闸打出来的移交标记的收件处。
//!
//! 还有两条只属于呈现面的纵线：
//!
//! * **向上取整**。GNU `df` 的 `Use%` 是**向上取整**的（coreutils 手册明写
//!   "the percentage is rounded up"）。于是 `avail = 1`、`used = 100` 的一行会
//!   同时印 `1` 和 `100%`。**"100% 且 avail≠0"是合法现场，不是矛盾**；而
//!   "avail=0 却印 99%"才是真的表内不自洽（本卡判死）。把 100% 当满盘报警、
//!   或把 avail≠0 当"还有救"，都读错了这张表。
//! * **保留段**。ext4 默认给 root 留 5%，`Available` 是 `f_bavail`（**已扣**保留段），
//!   `f_bfree` 还没归零。所以 `Available 0` 与"盘上一个空闲块都没有"是两句
//!   不同的话：前者是非 root 写不进去，后者物理上就不成立。本卡把这一档
//!   单独命名（`ReservedShown`），**它照样落本卡的呈现锚**——因为呈现面上
//!   它就是 `0`；但注解维度必须说清"0 是扣出来的，不是空出来的"。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 八道闸，秩从 **1** 起算；`Gate` 只接受"前进到下一秩"，倒退给 `DF_ESTAGE`，
//! `close()` 后再动给 `DF_ESTATE`。前一闸不产出事实，后一闸就没有输入，
//! 倒过来必先产生假绿。
//!
//! 1. `argv`（秩 1）——命令行本身。未知短选项、`--output` 的野字段、
//!    `--block-size` 的非法串都在这里出局，**一行表都没渲染**。这一闸排最前
//!    是为了钉死：用法错的 `df` 永远不许被记成"报表说盘满了"。GNU 的用法错
//!    退 **2**，与读侧错的 **1** 是两回事。
//! 2. `resolve`（秩 2）——把参数解析成挂载点。`ENOENT`/`EACCES`/`ENOTDIR` 在
//!    这一闸出局；此时 `df` 连 `statfs` 都没做过，任何 `avail` 数字都是无源之水。
//!    本卡六具尸位全部挂在这一闸的现场上，因为"报表根本没出来却报了一个数"
//!    是呈现面最危险的假。
//! 3. `read`（秩 3）——`statfs`/`statvfs` 取数。读侧错误（`ESTALE` 的陈旧 NFS
//!    句柄、`EOVERFLOW` 的 32 位溢出）落在这里。**这一闸没有 ENOSPC**：
//!    取数不分配，所以 28 出现在这里就是结构假。
//! 4. `table`（秩 4）——选表。`avail` 表（默认/`-P`/`-h`）与 inode 表（`-i`）
//!    是两张独立的表，各自有各自的"零"。这一闸必须在取数之后（没数就无从选表），
//!    又在零闸之前（"零"要先知道是哪张表的零才有意义）。
//! 5. `zero`（秩 5）——**本卡心脏闸**：判这一格出的是 `0` 还是 `-`。可测且
//!    `avail==0` 出 `0`；不可测出 `-`。它排在选表之后：`-i` 表的 `IFree==0`
//!    与 `avail` 表的 `Available==0` 是两个不同的锚（前者不落锚）。
//! 6. `round`（秩 6）——舍入与单位。`Use%` 向上取整、`-h`/`--si` 的二/十进制
//!    单位、`-P` 的 `Capacity` 串，全在这一闸定稿。排在零闸之后是硬的：
//!    先确定"有没有数字"，才谈得上"数字怎么印"。
//! 7. `hand`（秩 7）——移交闸。F11688 递过来的"写侧 28 而表上还有空间"在这里
//!    登记，**并且只登记、不改数**：已经渲染的 `0`/`-`/`12288` 一个字都不许被
//!    这一闸改动。排在取整之后，是因为移交是解释层的动作，不是排版层的动作。
//! 8. `exit`（秩 8）——退出码结算闸。绿 0、读侧错 1、用法错 2；`df` 的码
//!    **永远不来自空间**。锚位（本卡的满盘呈现锚）只在"表真出了 `0`、结构自洽、
//!    取数成功"三件齐时亮。
//!
//! ## 关键差异（本卡深度所在）
//!
//! * **与 F11688（touch/mkdir ENOSPC，段基址 `0x6d350`，位距 `0x10`）的边界**：
//!   F11688 判**那次 `mkdir()`/`open(O_CREAT)`** 与它留在盘上的残留；本卡判
//!   **那张表**。F11688 的第 5 闸认得"df 说还有但写不进去"，但它**不裁 df 的账**，
//!   只打 `hands_off` 标记；本卡的 `hand` 闸就是这个标记的**收件处**：收到之后
//!   登记 `HandoffMarked`，并且**明令不许改动已渲染的 avail 数字**。反向边界：
//!   `df` 显示满而 `mkdir` 成功（quota 宽限、保留段对 root 开放）归 F11688 判死，
//!   不归本卡——本卡只看表有没有如实把那个 `0` 印出来。
//! * **与 F11690（rename ENOSPC，段基址 `0x6d370`，位距 `0x10`）的边界**：
//!   F11690 判"满盘上改名到底吃不吃 28"，是写侧；本卡判"改完之后 `df` 的数
//!   变不变"——本卡**不判**写侧成功与否，只判呈现。两卡共用同一个几何真值表，
//!   但本卡的锚是绿锚（报表如实出 `0`），F11690 的锚是红锚（那次改名真的没分配
//!   东西）。谁判死谁放行：报表如实 → 本卡放行；改名报 28 → F11690 判死，
//!   本卡对它没有裁量权。
//! * **与 F11692（du 在满盘上的统计，只读统计面）的边界**：`du` 统计**已经落盘
//!   的字节**，`df` 统计**内核给的块数**；两者在一个满盘上可以对不上（延迟分配、
//!   稀疏文件、被删还开着的文件）。本卡只判 `df` 那一格印了什么，不许用 `du`
//!   的账来推翻 `df` 的表——那是 F11692 的活。
//! * **与 F11694（ENOSPC 文案逐字，跨工具一致）的边界**：F11694 管
//!   `No space left on device` 那一串本身；本卡管 **`df` 的字段名与表式样**
//!   （`Available` 对 `Capacity`、`1K-blocks` 对 `1024-blocks`、`Use%` 对 `IUse%`）
//!   和 `cannot stat %s` 的**动词**。两卡在同一串上联签，但表式样面只归本卡。
//! * **本卡的排他职责**：① `-` 与 `0` 的分档（`DASH_CELL` 哨兵，绝不折算成零）；
//!   ② `Use%` 向上取整导致的"100% 但 avail≠0"合法档，与"avail=0 但 Use%≠100"
//!   的表内不自洽判死；③ `df` 永不吃 ENOSPC（`SpaceLie` 判死）；
//!   ④ inode 表的 `0` 与 avail 表的 `0` 分锚；⑤ 保留段吃出来的 `0` 照样落锚，
//!   但注解必须说清是扣出来的；⑥ 移交登记只许记账不许改数。
//! * **谁判死谁放行**：表出了 `0` 且几何与取整自洽 → 放行并落**呈现锚**；
//!   把 `-` 读成 `0`、把 `df` 的失败归因给盘满、`avail=0` 却配一个不到 100 的
//!   `Use%`、给 inode 表的零落本卡的 avail 锚、用法错落了呈现锚 → 一律判死。
//!
//! 判据六维：结束阶段（落哪道闸）、errno（读侧码）、呈现值（那一格的数字或横杠
//! 哨兵）、注解（呈现档）、锚位（满盘 avail 0 呈现锚）、成败申报（estate）。
//! 判序固定：状态 → 阶段 → errno → 呈现值 → 注解 → 锚位。
//!
//! 语料 14 槽：8 槽申报正确（avail 真零 3、横杠 1、inode 零 1、取整 1、移交 1、
//! 用法 1），6 槽一槽只坏一维，依次挂 Estate/Stage/Code/Value/Note/Default。
//! 账本 14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码，冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11689）。
pub const SEG_BASE: u32 = 0x6d360;

/// 交叉署名：F11688「touch/mkdir ENOSPC」的段基址（本卡的直接前置）。
/// 本卡**不 import** 那个模块的任何符号，只把它的段位与位距原样声明一遍，
/// 由 `anchors_agree()` 复算对齐。
pub const ANCHOR_F11688_SEG: u32 = 0x6d350;
/// F11688 与本卡的位距（0x6d360 - 0x6d350）。
pub const ANCHOR_F11688_GAP: u32 = 0x10;

/// 交叉署名：F11690「rename ENOSPC」的段基址（本卡的直接后置）。
pub const ANCHOR_F11690_SEG: u32 = 0x6d370;
/// F11690 与本卡的位距。
pub const ANCHOR_F11690_GAP: u32 = 0x10;

/// 交叉署名：F11687「mv 跨盘部分复制回滚」的段基址。
pub const ANCHOR_F11687_SEG: u32 = 0x6d330;
/// F11687 与本卡的位距（0x6d360 - 0x6d330）。
pub const ANCHOR_F11687_GAP: u32 = 0x30;

/// 写侧空间错的 errno 真值：本卡**只引用**它，绝不允许它出现在 `df` 的码里。
pub const ANCHOR_ENOSPC: i32 = 28;
/// strerror 原文（与 F11694 联签的那一串，在本卡只出现一次，用来证否）。
pub const ANCHOR_ERRNO_TEXT: &str = "No space left on device";
/// 基准机上 `df` 读侧错的退出码。
pub const ANCHOR_EXIT_READ: u32 = 1;

/// 语料槽数：8 活 + 6 死。
pub const CORPUS: usize = 14;
/// 账本格数（与语料同尺，便于逐格对账）。
pub const SAMPLES: usize = 14;
/// 申报正确槽数。
pub const ACCEPT: usize = 8;
/// 申报错误槽数（一槽只坏一维）。
pub const REJECT: usize = 6;
/// 呈现档数。
pub const TIER_COUNT: usize = 4;
/// 闸门段数（argv/resolve/read/table/zero/round/hand/exit）。
pub const STAGE_COUNT: u32 = 8;
/// 最后一段的秩。
pub const STAGE_LAST_RANK: u32 = 8;
/// 账本里的统计格数。
pub const STAT_CELLS: usize = 7;
/// 红档率标定值：14 格里错 1 格时的千分率（13007/14 = 929）。
pub const RED_RATE: u32 = 929;
/// 全绿时的千分率。
pub const RATE_FULL: u32 = 1000;

/// 横杠哨兵：`-` 在数值轴上**没有位置**，本卡绝不允许把它折算成 0。
/// 任何把横杠读成零的实现都会让 `value` 与申报对不上，当场判死。
pub const DASH_CELL: u32 = 4_294_967_295;

/// 读侧错误：文件不存在（`df` 的 `cannot stat` 最常见的一档）。
pub const ERR_ENOENT: i32 = 2;
/// 读侧错误：权限不够（挂载点目录不可查）。
pub const ERR_EACCES: i32 = 13;
/// 读侧错误：不是目录。
pub const ERR_ENOTDIR: i32 = 20;
/// 读侧错误：路径过长。
pub const ERR_ENAMETOOLONG: i32 = 36;
/// 读侧错误：符号链接成环。
pub const ERR_ELOOP: i32 = 40;
/// 读侧错误：陈旧句柄（NFS 服务端掉线后的 `df`）。
pub const ERR_ESTALE: i32 = 116;
/// 读侧错误：数值溢出（32 位 `statfs` 装不下 16T 盘的块数）。
pub const ERR_EOVERFLOW: i32 = 75;
/// 读侧错误：无此设备（已拔掉的块设备）。
pub const ERR_ENODEV: i32 = 19;
/// 写侧空间错。**本卡的呈现面永不允许它出现**；只用来把 `SpaceLie` 钉死。
pub const ERR_ENOSPC: i32 = 28;
/// 配额超限。邻居码，本卡只在对照档里引用。
pub const ERR_EDQUOT: i32 = 122;
/// 无错误（`df` 绿）。
pub const ERR_NONE: i32 = 0;

/// 本卡认得的读侧码个数。
pub const READ_ERRNO_COUNT: usize = 8;
/// 呈现面禁用的空间码个数（ENOSPC 与 EDQUOT 两个都不许由 `df` 报出）。
pub const SPACE_ERRNO_COUNT: usize = 2;

/// 绿：报表正常出。
pub const EXIT_OK: u32 = 0;
/// 红：读侧错（stat/挂载点解析失败）。
pub const EXIT_READ: u32 = 1;
/// 用法错：GNU 的 `df` 在选项层退 2，与读侧的 1 是两条线。
pub const EXIT_USAGE: u32 = 2;
/// 不裁定：别的工具的表，本卡不给码。
pub const EXIT_UNJUDGED: u32 = 99;

/// 退出码档数。
pub const EXIT_RANKS: usize = 4;

/// `df` 表里一格的最大显示宽度（GNU 的 `Available` 列宽 10，横杠占 1）。
pub const CELL_WIDTH: u32 = 10;
/// 一行的列数（默认表 6 列：Filesystem/1K-blocks/Used/Available/Use%/Mounted on）。
pub const COLUMN_COUNT: u32 = 6;
/// inode 表与 avail 表的列名差：两张表**共用**行数，但零的含义不同。
pub const TABLE_KIND_COUNT: usize = 2;

/// GNU `df` 向上取整的分子基数（`used * 100 / (used + avail)` 再进位）。
pub const PCT_BASE: u32 = 100;
/// `-h` 的二进制进制基数（GNU coreutils 的 `-h` 用 1024）。
pub const HUMAN_BIN_UNIT: u32 = 1024;
/// `--si` 的十进制进制基数。
pub const HUMAN_SI_UNIT: u32 = 1000;
/// 单位档位个数（K/M/G/T/P）。
pub const UNIT_STEPS: usize = 5;

// ---------------------------------------------------------------------------
/// 13 个错误码：段基址 + 序号，每个钉住一维。
// ---------------------------------------------------------------------------

/// 成败互报（申报绿而实际红，或反之）→ 判序第一维。
pub const DF_ESTATE: u32 = SEG_BASE + 0x01;
/// 落闸阶段错位（呈现值说成取数前就有）→ 判序第二维。
pub const DF_ESTAGE: u32 = SEG_BASE + 0x02;
/// 读侧 errno 错位（`ENOENT` 与 `ENOSPC` 互换是本卡点名的一号假）→ 判序第三维。
pub const DF_ECODE: u32 = SEG_BASE + 0x03;
/// 呈现值错位（那一格的数字或横杠哨兵对不上）→ 判序第四维。
pub const DF_EVALUE: u32 = SEG_BASE + 0x04;
/// 呈现档错位（`-`/`0`/取整/移交四档混认）→ 判序第五维。
pub const DF_ENOTE: u32 = SEG_BASE + 0x05;
/// 呈现锚位错位（该落不落、不该落却落）→ 判序第六维。
pub const DF_EDEFAULT: u32 = SEG_BASE + 0x06;
/// 几何矛盾（块数/保留段/可测性与呈现档打架）。
pub const DF_EGEOM: u32 = SEG_BASE + 0x07;
/// 状态违例（冻结后再登记、关闸后再走）。
pub const DF_ECLOSE: u32 = SEG_BASE + 0x08;
/// 计数失衡（报表/矩阵/账本任一处不平）。
pub const DF_ECOUNT: u32 = SEG_BASE + 0x09;
/// 槽位非法（登记跳号、越界）。
pub const DF_ESLOT: u32 = SEG_BASE + 0x0a;
/// 指纹不符（冻结重放对不上）。
pub const DF_ESEAL: u32 = SEG_BASE + 0x0b;
/// 无观测（槽号越界、期望表缺格）。
pub const DF_ENOBS: u32 = SEG_BASE + 0x0c;
/// 矩阵不平（四档合计与语料格数不符）。
pub const DF_EMATRIX: u32 = SEG_BASE + 0x0d;

/// 十三码全集（`name_ok` 逐个复算段位与互异性）。
pub const ALL_CODES: [u32; 13] = [
    DF_ESTATE,
    DF_ESTAGE,
    DF_ECODE,
    DF_EVALUE,
    DF_ENOTE,
    DF_EDEFAULT,
    DF_EGEOM,
    DF_ECLOSE,
    DF_ECOUNT,
    DF_ESLOT,
    DF_ESEAL,
    DF_ENOBS,
    DF_EMATRIX,
];

// ---------------------------------------------------------------------------
// 账本操作码（本卡口径：呈现面的五种统计动作 + 两种缺陷 + 冻结重放）
// ---------------------------------------------------------------------------

/// 呈现总表。
pub const OP_TABLE: u8 = 0x01;
/// avail 格定稿。
pub const OP_AVAIL: u8 = 0x02;
/// inode 格定稿。
pub const OP_INODE: u8 = 0x03;
/// 取整与单位。
pub const OP_ROUND: u8 = 0x04;
/// 移交登记。
pub const OP_HAND: u8 = 0x05;
/// 横杠档计数。
pub const OP_DASH: u8 = 0x06;
/// 语料装配。
pub const OP_CORPUS: u8 = 0x07;
/// 红档标定。
pub const OP_RATE: u8 = 0x08;
/// 缺陷格入账。
pub const OP_DEFECT: u8 = 0x09;
/// 冻结。
pub const OP_FREEZE: u8 = 0x0a;
/// 重放。
pub const OP_REPLAY: u8 = 0x0b;

/// 操作码个数。
pub const OP_COUNT: usize = 11;

/// 选项位图：`-h`。
pub const OPT_HUMAN: u8 = 0x01;
/// 选项位图：`-i`（换 inode 表）。
pub const OPT_INODES: u8 = 0x02;
/// 选项位图：`-P`（POSIX 表式样）。
pub const OPT_POSIX: u8 = 0x04;
/// 选项位图：`--total`（多一行合计）。
pub const OPT_TOTAL: u8 = 0x08;
/// 选项位图：`-a`（含伪文件系统）。
pub const OPT_ALL: u8 = 0x10;
/// 选项位图：`--si`（十进制单位）。
pub const OPT_SI: u8 = 0x20;
/// 选项位图：`-T`（多一列 Type）。
pub const OPT_FSTYPE: u8 = 0x40;
/// 选项位图：`--output=`（自选列，表式样由字段表决定）。
pub const OPT_OUTPUT: u8 = 0x80;

/// 本卡认得的选项位全集。
pub const OPT_KNOWN: u8 = OPT_HUMAN
    | OPT_INODES
    | OPT_POSIX
    | OPT_TOTAL
    | OPT_ALL
    | OPT_SI
    | OPT_FSTYPE
    | OPT_OUTPUT;

/// 选项位数。
pub const OPT_BIT_COUNT: usize = 8;

/// 选项位表（顺序固定，`enums_ok` 逐个复算互异性）。
pub const OPT_BITS: [u8; OPT_BIT_COUNT] = [
    OPT_HUMAN,
    OPT_INODES,
    OPT_POSIX,
    OPT_TOTAL,
    OPT_ALL,
    OPT_SI,
    OPT_FSTYPE,
    OPT_OUTPUT,
];

/// 挂载点名字的最大长度（超过则 GNU 会折行；`-P` 保证不折行）。
pub const MOUNT_NAME_MAX: u32 = 255;

// ---------------------------------------------------------------------------
// FNV-1a 64 位折叠
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节：先抑或、再乘素数（FNV-1a 的顺序，写反就不是同一个指纹）。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut acc = h ^ (b as u64);
    let mut n = 0usize;
    while n < 8 {
        acc = acc.wrapping_mul(FNV_PRIME);
        n += 1;
    }
    acc
}

/// 折入一个字：按大端逐字节喂进去，保证高位上的差别也换指纹。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut acc = h;
    let mut pos = 0usize;
    while pos < 8 {
        let shift = 56u32.saturating_sub((pos as u32) * 8);
        let byte = ((w >> shift) & 0xff) as u8;
        acc = fold_byte(acc, byte);
        pos += 1;
    }
    acc
}

/// 折入一个字符串（逐字节，含结尾不含 NUL——文案比对靠整串相等，不靠指纹）。
fn fold_str(h: u64, s: &str) -> u64 {
    let raw = s.as_bytes();
    let mut acc = h;
    let mut i = 0usize;
    while i < raw.len() {
        if let Some(b) = raw.get(i) {
            acc = fold_byte(acc, *b);
        }
        i += 1;
    }
    acc
}

/// 折入一个有符号数（errno 用）。
fn fold_i32(h: u64, v: i32) -> u64 {
    fold_word(h, v as i64 as u64)
}

/// 折入一个布尔。
fn fold_bool(h: u64, v: bool) -> u64 {
    fold_byte(h, if v { 1u8 } else { 0u8 })
}

// ---------------------------------------------------------------------------
// 数字/布尔文本
// ---------------------------------------------------------------------------

/// 十进制文本（u32，循环展开、不递归、不格式化）。
fn dec_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut digits = [0u8; 10];
    let mut left = v;
    let mut tail = 10usize;
    while left > 0 && tail > 0 {
        tail -= 1;
        if let Some(slot) = digits.get_mut(tail) {
            *slot = b'0' + (left % 10) as u8;
        }
        left /= 10;
    }
    let mut out = String::new();
    let mut i = tail;
    while i < 10 {
        if let Some(b) = digits.get(i) {
            out.push(*b as char);
        }
        i += 1;
    }
    out
}

/// 定宽小写十六进制文本（段基址串用）。
fn hex_text(v: u32, digits: usize) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < digits && i < 8 {
        let shift = ((digits - 1 - i) as u32) * 4;
        let nib = ((v >> shift) & 0xf) as u8;
        out.push(if nib < 10 {
            (b'0' + nib) as char
        } else {
            (b'a' + (nib - 10)) as char
        });
        i += 1;
    }
    out
}

/// 无符号十进制文本（对外：报表里所有数字格都过这一道）。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// 有符号十进制文本（errno 用）。
pub fn i32_text(v: i32) -> String {
    if v == 0 {
        return String::from("0");
    }
    if v < 0 {
        let mut out = String::from("-");
        out.push_str(dec_text(v.unsigned_abs()).as_str());
        return out;
    }
    dec_text(v as u32)
}

/// 布尔文本（账本与明细里的 true/false）。
pub fn bool_text(v: bool) -> &'static str {
    if v {
        "yes"
    } else {
        "no"
    }
}

/// 段位文本：`0x6d36x` 式样，宽度钉死为 5 位，防止段位串位。
pub fn seg_text(v: u32) -> String {
    let mut out = String::from("0x");
    out.push_str(hex_text(v, 5).as_str());
    out
}

/// 呈现格文本：**横杠哨兵在这一道变回 `-`**，其余变回十进制。
///
/// 这一道是本卡的心脏函数：`DASH_CELL` 走左边支，任何把它当 0 的写法都过不了
/// `present_ok`。
pub fn cell_text(v: u32) -> String {
    if v == DASH_CELL {
        return String::from("-");
    }
    dec_text(v)
}

/// 百分号格文本：`None` 出 `-`（空间不适用），`Some(p)` 出 `<p>%`。
pub fn pct_text(v: Option<u32>) -> String {
    let mut out = match v {
        Some(p) => dec_text(p),
        None => String::from("-"),
    };
    if out.as_str() != "-" {
        out.push('%');
    }
    out
}

/// errno 与名字拼成一格：`2/ENOENT`。
pub fn errno_pair_text(code: i32) -> String {
    let mut out = i32_text(code);
    out.push('/');
    out.push_str(errno_name(code));
    out
}

/// 千分率文本：`929ppm` 式样（红档率标定串）。
pub fn ppm_text(v: u32) -> String {
    let mut out = dec_text(v);
    out.push_str("ppm");
    out
}

// ---------------------------------------------------------------------------
// strerror 与 errno 名
// ---------------------------------------------------------------------------

/// strerror 原文表：只收本卡读侧认得的码，**逐字**（POSIX 大写在 E 后不额外加空格）。
pub fn strerror(code: i32) -> &'static str {
    match code {
        0 => "Success",
        2 => "No such file or directory",
        13 => "Permission denied",
        19 => "No such device",
        20 => "Not a directory",
        28 => "No space left on device",
        36 => "File name too long",
        40 => "Too many levels of symbolic links",
        75 => "Value too large for defined data type",
        116 => "Stale file handle",
        122 => "Disk quota exceeded",
        _ => "Unknown error",
    }
}

/// errno 名字表。
pub fn errno_name(code: i32) -> &'static str {
    match code {
        0 => "SUCCESS",
        2 => "ENOENT",
        13 => "EACCES",
        19 => "ENODEV",
        20 => "ENOTDIR",
        28 => "ENOSPC",
        36 => "ENAMETOOLONG",
        40 => "ELOOP",
        75 => "EOVERFLOW",
        116 => "ESTALE",
        122 => "EDQUOT",
        _ => "EUNKNOWN",
    }
}

/// 本卡认得的读侧码全集（`name_ok` 复算互异与长度）。
pub const READ_ERRNOS: [i32; READ_ERRNO_COUNT] = [
    ERR_ENOENT,
    ERR_EACCES,
    ERR_ENOTDIR,
    ERR_ENAMETOOLONG,
    ERR_ELOOP,
    ERR_ESTALE,
    ERR_EOVERFLOW,
    ERR_ENODEV,
];

/// 呈现面禁用的空间码（`df` 一旦报出这两个之一，就是结构假）。
pub const FORBIDDEN_ERRNOS: [i32; SPACE_ERRNO_COUNT] = [ERR_ENOSPC, ERR_EDQUOT];

/// 码是否落在本卡认得的读侧集合里。
pub fn errno_readable(code: i32) -> bool {
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

/// 码是否是"呈现面禁用的空间码"。本卡用它把 `SpaceLie` 钉死。
pub fn errno_forbidden_on_df(code: i32) -> bool {
    let mut i = 0usize;
    while i < FORBIDDEN_ERRNOS.len() {
        if let Some(c) = FORBIDDEN_ERRNOS.get(i) {
            if *c == code {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// 文案：逐字复刻上游
// ---------------------------------------------------------------------------

/// GNU coreutils `df` 的取数失败行：`df: cannot stat '<path>': <strerror>`。
///
/// 上游格式串是 `cannot stat %s`，`%s` 过 `quote()`，C locale 下单引号；
/// 第二个 `%s` 是 `error(0, errno, ...)` 自己接的 strerror。
pub fn msg_gnu_cannot_stat(path: &str, code: i32) -> String {
    let mut out = String::from("df: cannot stat '");
    out.push_str(path);
    out.push_str("': ");
    out.push_str(strerror(code));
    out
}

/// GNU `df` 的选项错两行：`df: invalid option -- '<c>'` 换行 `Try ...`。
pub fn msg_gnu_invalid_option(c: char) -> String {
    let mut out = String::from("df: invalid option -- '");
    out.push(c);
    out.push_str("'\nTry 'df --help' for more information.");
    out
}

/// GNU `df` 的 `--output` 野字段行（上游用弯引号，逐字保留）：
/// `df: option --output: field ‘bogus’ unknown`。
pub fn msg_gnu_bad_output(field: &str) -> String {
    let mut out = String::from("df: option --output: field \u{2018}");
    out.push_str(field);
    out.push_str("\u{2019} unknown\nTry 'df --help' for more information.");
    out
}

/// GNU `df` 的块大小非法串行：`df: invalid --block-size argument 'x'`。
pub fn msg_gnu_bad_block_size(arg: &str) -> String {
    let mut out = String::from("df: invalid --block-size argument '");
    out.push_str(arg);
    out.push('\'');
    out
}

/// GNU 默认表的字段名（`Filesystem 1K-blocks Used Available Use% Mounted on`）。
pub const GNU_HEADER_FIELDS: [&str; 6] = [
    "Filesystem",
    "1K-blocks",
    "Used",
    "Available",
    "Use%",
    "Mounted on",
];

/// GNU inode 表的字段名（`Filesystem Inodes IUsed IFree IUse% Mounted on`）。
pub const GNU_INODE_FIELDS: [&str; 6] = [
    "Filesystem",
    "Inodes",
    "IUsed",
    "IFree",
    "IUse%",
    "Mounted on",
];

/// GNU `-h` 表（`Filesystem Size Used Avail Use% Mounted on`）。
pub const GNU_HUMAN_FIELDS: [&str; 6] = [
    "Filesystem",
    "Size",
    "Used",
    "Avail",
    "Use%",
    "Mounted on",
];

/// POSIX `-P` 表（`Filesystem 1024-blocks Used Available Capacity Mounted on`）。
pub const POSIX_FIELDS: [&str; 6] = [
    "Filesystem",
    "1024-blocks",
    "Used",
    "Available",
    "Capacity",
    "Mounted on",
];

/// busybox `df` 的挂载点解析失败行（`bb_error_msg_and_die` 式样）。
pub fn msg_bb_no_mount_point(path: &str) -> String {
    let mut out = String::from("df: can't find mount point of '");
    out.push_str(path);
    out.push('\'');
    out
}

/// busybox 表字段名（与 GNU 同族，但列宽与折行规则不同）。
pub const BB_HEADER_FIELDS: [&str; 6] = [
    "Filesystem",
    "1K-blocks",
    "Used",
    "Available",
    "Use%",
    "Mounted on",
];

/// `--total` 追加行的第一格（上游逐字 `total`）。
pub const TOTAL_ROW_LABEL: &str = "total";

/// 表头文本：按（家族, 表, 单位）选字段表，用两个空格连成一行。
pub fn msg_header(flavor: DfFlavor, table: TableSel, posix: bool) -> String {
    let fields: &[&str] = match flavor {
        DfFlavor::Gnu | DfFlavor::Busybox => {
            if table == TableSel::Inode {
                &GNU_INODE_FIELDS
            } else if posix {
                &POSIX_FIELDS
            } else {
                &GNU_HEADER_FIELDS
            }
        }
        DfFlavor::Posix => &POSIX_FIELDS,
        DfFlavor::Mixed => &GNU_HEADER_FIELDS,
    };
    let mut out = String::new();
    let mut i = 0usize;
    while i < fields.len() {
        if i > 0 {
            out.push_str("  ");
        }
        if let Some(f) = fields.get(i) {
            out.push_str(f);
        }
        i += 1;
    }
    out
}

/// 一行报表的文本：`<mount> <size> <used> <avail> <pct>`（本卡只呈现这四格，
/// `Mounted on` 用 `mount` 字段本身）。横杠格在这里变回 `-`。
pub fn msg_row(mount: &str, size: u32, used: u32, avail: u32, pct: Option<u32>) -> String {
    let mut out = String::from(mount);
    out.push(' ');
    out.push_str(cell_text(size).as_str());
    out.push(' ');
    out.push_str(cell_text(used).as_str());
    out.push(' ');
    out.push_str(cell_text(avail).as_str());
    out.push(' ');
    out.push_str(pct_text(pct).as_str());
    out
}

// ---------------------------------------------------------------------------
// 枚举：版式家族 / 选表 / 单位 / 文件系统种类
// ---------------------------------------------------------------------------

/// 版式家族：同一份 `statfs` 数，四个实现印出四张不同的表。
///
/// 本卡管的就是这一族差异——`Available` 与 `Capacity` 是同一格的两个名字，
/// `1K-blocks` 与 `1024-blocks` 是同一列的两个表头，而横杠档只有 GNU 与
/// busybox 都给，POSIX 明令"不适用时给 0 还是给空"由实现决定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DfFlavor {
    /// GNU coreutils（`Available` / `Use%` / `1K-blocks`）。
    Gnu,
    /// POSIX `-P` 版式（`Capacity` / `1024-blocks`，不折行）。
    Posix,
    /// busybox（字段名同 GNU，列宽与错误串不同）。
    Busybox,
    /// 混血：GNU 表头 + POSIX 字段名混排。逐字比对在这一族**不成立**，
    /// 本卡直接把它压成不自洽（谎报档），因为运维拿两边的正则去套都会中。
    Mixed,
}

impl DfFlavor {
    /// 序号。
    pub fn index(&self) -> usize {
        match self {
            DfFlavor::Gnu => 0usize,
            DfFlavor::Posix => 1usize,
            DfFlavor::Busybox => 2usize,
            DfFlavor::Mixed => 3usize,
        }
    }

    /// 反查。
    pub fn from_index(i: usize) -> Option<DfFlavor> {
        match i {
            0usize => Some(DfFlavor::Gnu),
            1usize => Some(DfFlavor::Posix),
            2usize => Some(DfFlavor::Busybox),
            3usize => Some(DfFlavor::Mixed),
            _ => None,
        }
    }

    /// 短名。
    pub fn name(&self) -> &'static str {
        match self {
            DfFlavor::Gnu => "gnu-df",
            DfFlavor::Posix => "posix-df",
            DfFlavor::Busybox => "busybox-df",
            DfFlavor::Mixed => "mixed-df",
        }
    }

    /// 是否本卡管辖（四个都管，`Mixed` 管但判死）。
    pub fn owned(&self) -> bool {
        true
    }

    /// 是否可逐字比对文案（混血不可）。
    pub fn verbatim(&self) -> bool {
        !matches!(self, DfFlavor::Mixed)
    }

    /// 是否给"不适用"出横杠（GNU/busybox 给，POSIX 版式在本卡口径下也横杠，
    /// 混血不给——因为它自己都不知道该出什么）。
    pub fn dashes(&self) -> bool {
        !matches!(self, DfFlavor::Mixed)
    }
}

/// 选表：avail 表与 inode 表各自的"零"含义不同。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TableSel {
    /// 块表（默认 / `-h` / `--si` / `-P`）。
    Block,
    /// inode 表（`-i`）。
    Inode,
}

impl TableSel {
    /// 序号。
    pub fn index(&self) -> usize {
        match self {
            TableSel::Block => 0usize,
            TableSel::Inode => 1usize,
        }
    }

    /// 反查。
    pub fn from_index(i: usize) -> Option<TableSel> {
        match i {
            0usize => Some(TableSel::Block),
            1usize => Some(TableSel::Inode),
            _ => None,
        }
    }

    /// 短名。
    pub fn name(&self) -> &'static str {
        match self {
            TableSel::Block => "block-table",
            TableSel::Inode => "inode-table",
        }
    }

    /// 该表的"可用"列叫什么（文案逐字）。
    pub fn avail_column(&self) -> &'static str {
        match self {
            TableSel::Block => "Available",
            TableSel::Inode => "IFree",
        }
    }

    /// 该表的百分比列叫什么。
    pub fn pct_column(&self) -> &'static str {
        match self {
            TableSel::Block => "Use%",
            TableSel::Inode => "IUse%",
        }
    }
}

/// 单位档：排版层的进制选择。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnitMode {
    /// 1K 块（默认）。
    Kilobytes,
    /// `-h`：二进制人类可读（1024 进制）。
    Human,
    /// `--si`：十进制人类可读（1000 进制）。
    Si,
    /// `-P`：POSIX 的 1024 块。
    PosixBlocks,
    /// `--output=`：只出选中的列，没有表头族差异。
    Chosen,
}

impl UnitMode {
    /// 序号。
    pub fn index(&self) -> usize {
        match self {
            UnitMode::Kilobytes => 0usize,
            UnitMode::Human => 1usize,
            UnitMode::Si => 2usize,
            UnitMode::PosixBlocks => 3usize,
            UnitMode::Chosen => 4usize,
        }
    }

    /// 反查。
    pub fn from_index(i: usize) -> Option<UnitMode> {
        match i {
            0usize => Some(UnitMode::Kilobytes),
            1usize => Some(UnitMode::Human),
            2usize => Some(UnitMode::Si),
            3usize => Some(UnitMode::PosixBlocks),
            4usize => Some(UnitMode::Chosen),
            _ => None,
        }
    }

    /// 短名。
    pub fn name(&self) -> &'static str {
        match self {
            UnitMode::Kilobytes => "1k",
            UnitMode::Human => "human",
            UnitMode::Si => "si",
            UnitMode::PosixBlocks => "posix-1024",
            UnitMode::Chosen => "output",
        }
    }

    /// 进制基数（原始块数在这一档要乘的字节数；`Chosen` 按 1K 走）。
    pub fn scale(&self) -> u32 {
        match self {
            UnitMode::Kilobytes | UnitMode::Chosen => HUMAN_BIN_UNIT,
            UnitMode::PosixBlocks => HUMAN_BIN_UNIT,
            UnitMode::Human => HUMAN_BIN_UNIT,
            UnitMode::Si => HUMAN_SI_UNIT,
        }
    }

    /// 是否人类可读档（会影响表头字段名）。
    pub fn human(&self) -> bool {
        matches!(self, UnitMode::Human | UnitMode::Si)
    }
}

/// 文件系统种类：决定有没有保留段、有没有 inode 表、可不可测。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FsKind {
    /// ext2（有保留段、有 inode）。
    Ext2,
    /// ext3/ext4（有保留段、有 inode）。
    Ext4,
    /// XFS（无保留段、有 inode）。
    Xfs,
    /// Btrfs（元数据块组独立，`df` 的 avail 会说谎）。
    Btrfs,
    /// tmpfs（有 inode，无保留段）。
    Tmpfs,
    /// squashfs（只读压缩，可测但永远写不进去）。
    Squashfs,
    /// NFS（迟到错误面，inode 数由服务端给）。
    Nfs,
    /// overlay（上层可写、下层只读，avail 取最下层）。
    Overlay,
    /// 伪文件系统（proc/sysfs/devtmpfs：空间不适用）。
    Pseudo,
    /// ZFS（有 inode，数据集配额与 avail 是两回事）。
    Zfs,
}

impl FsKind {
    /// 序号。
    pub fn index(&self) -> usize {
        match self {
            FsKind::Ext2 => 0usize,
            FsKind::Ext4 => 1usize,
            FsKind::Xfs => 2usize,
            FsKind::Btrfs => 3usize,
            FsKind::Tmpfs => 4usize,
            FsKind::Squashfs => 5usize,
            FsKind::Nfs => 6usize,
            FsKind::Overlay => 7usize,
            FsKind::Pseudo => 8usize,
            FsKind::Zfs => 9usize,
        }
    }

    /// 反查。
    pub fn from_index(i: usize) -> Option<FsKind> {
        match i {
            0usize => Some(FsKind::Ext2),
            1usize => Some(FsKind::Ext4),
            2usize => Some(FsKind::Xfs),
            3usize => Some(FsKind::Btrfs),
            4usize => Some(FsKind::Tmpfs),
            5usize => Some(FsKind::Squashfs),
            6usize => Some(FsKind::Nfs),
            7usize => Some(FsKind::Overlay),
            8usize => Some(FsKind::Pseudo),
            9usize => Some(FsKind::Zfs),
            _ => None,
        }
    }

    /// 种类名（`df -T` 的 Type 列逐字）。
    pub fn type_name(&self) -> &'static str {
        match self {
            FsKind::Ext2 => "ext2",
            FsKind::Ext4 => "ext4",
            FsKind::Xfs => "xfs",
            FsKind::Btrfs => "btrfs",
            FsKind::Tmpfs => "tmpfs",
            FsKind::Squashfs => "squashfs",
            FsKind::Nfs => "nfs",
            FsKind::Overlay => "overlay",
            FsKind::Pseudo => "proc",
            FsKind::Zfs => "zfs",
        }
    }

    /// 有没有 root 保留段（只有 ext 家族有）。
    pub fn has_reserved_blocks(&self) -> bool {
        matches!(self, FsKind::Ext2 | FsKind::Ext4)
    }

    /// 空间是否可测（伪文件系统不可测）。
    pub fn measurable(&self) -> bool {
        !matches!(self, FsKind::Pseudo)
    }

    /// 有没有 inode 计数（squashfs/伪文件系统没有）。
    pub fn has_inodes(&self) -> bool {
        !matches!(self, FsKind::Pseudo | FsKind::Squashfs)
    }

    /// 元数据是否与数据块分离（btrfs 的 avail 会说谎那一档）。
    pub fn metadata_detached(&self) -> bool {
        matches!(self, FsKind::Btrfs | FsKind::Zfs)
    }

    /// 只读种类（squashfs 满盘是常态，`df` 出 0 却不该报警）。
    pub fn read_only(&self) -> bool {
        matches!(self, FsKind::Squashfs)
    }
}

/// 种类全集（`geom_ok` 逐个复算）。
pub const FS_KINDS: [FsKind; 10] = [
    FsKind::Ext2,
    FsKind::Ext4,
    FsKind::Xfs,
    FsKind::Btrfs,
    FsKind::Tmpfs,
    FsKind::Squashfs,
    FsKind::Nfs,
    FsKind::Overlay,
    FsKind::Pseudo,
    FsKind::Zfs,
];

/// 目标解析档：`df` 拿参数去找挂载点的结果。
///
/// 这一档是**读侧**的：它只决定"有没有表可渲染"，与盘满没有任何关系。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TargetClass {
    /// 正常解析到一个挂载点。
    Mounted,
    /// 参数是个块设备，`df` 按设备名去找挂载点（GNU 放行，busybox 报找不到）。
    BlockDev,
    /// 解析时 `stat` 失败：文件不存在。
    NotFound,
    /// 解析时权限不够。
    Denied,
    /// 路径中段不是目录。
    NotDirectory,
    /// 路径过长。
    TooLong,
    /// 符号链接成环。
    SymlinkLoop,
    /// 挂载点存在但服务端掉了（陈旧句柄）。
    StaleMount,
    /// 设备已拔（无此设备）。
    Detached,
    /// 别的工具的表，本卡不裁定。
    Foreign,
}

impl TargetClass {
    /// 序号。
    pub fn index(&self) -> usize {
        match self {
            TargetClass::Mounted => 0usize,
            TargetClass::BlockDev => 1usize,
            TargetClass::NotFound => 2usize,
            TargetClass::Denied => 3usize,
            TargetClass::NotDirectory => 4usize,
            TargetClass::TooLong => 5usize,
            TargetClass::SymlinkLoop => 6usize,
            TargetClass::StaleMount => 7usize,
            TargetClass::Detached => 8usize,
            TargetClass::Foreign => 9usize,
        }
    }

    /// 反查。
    pub fn from_index(i: usize) -> Option<TargetClass> {
        match i {
            0usize => Some(TargetClass::Mounted),
            1usize => Some(TargetClass::BlockDev),
            2usize => Some(TargetClass::NotFound),
            3usize => Some(TargetClass::Denied),
            4usize => Some(TargetClass::NotDirectory),
            5usize => Some(TargetClass::TooLong),
            6usize => Some(TargetClass::SymlinkLoop),
            7usize => Some(TargetClass::StaleMount),
            8usize => Some(TargetClass::Detached),
            9usize => Some(TargetClass::Foreign),
            _ => None,
        }
    }

    /// 短名（语料标签里出现的那一段）。
    pub fn name(&self) -> &'static str {
        match self {
            TargetClass::Mounted => "mounted",
            TargetClass::BlockDev => "blockdev",
            TargetClass::NotFound => "notfound",
            TargetClass::Denied => "denied",
            TargetClass::NotDirectory => "notdir",
            TargetClass::TooLong => "toolong",
            TargetClass::SymlinkLoop => "eloop",
            TargetClass::StaleMount => "stale",
            TargetClass::Detached => "detached",
            TargetClass::Foreign => "foreign",
        }
    }

    /// 是否连表都没渲染（解析就死了）。
    pub fn blocks_table(&self) -> bool {
        matches!(
            self,
            TargetClass::NotFound
                | TargetClass::Denied
                | TargetClass::NotDirectory
                | TargetClass::TooLong
                | TargetClass::SymlinkLoop
                | TargetClass::Detached
        )
    }

    /// 解析失败时的邻居码；绿档给 0。
    pub fn errno(&self) -> i32 {
        match self {
            TargetClass::NotFound => ERR_ENOENT,
            TargetClass::Denied => ERR_EACCES,
            TargetClass::NotDirectory => ERR_ENOTDIR,
            TargetClass::TooLong => ERR_ENAMETOOLONG,
            TargetClass::SymlinkLoop => ERR_ELOOP,
            TargetClass::Detached => ERR_ENODEV,
            _ => ERR_NONE,
        }
    }
}

// ---------------------------------------------------------------------------
// 阶段（八道闸）
// ---------------------------------------------------------------------------

/// `df` 呈现面的八道闸，秩 1..8。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 闸 1：命令行本身。
    Argv,
    /// 闸 2：挂载点解析。
    Resolve,
    /// 闸 3：`statfs` 取数。
    Read,
    /// 闸 4：选表（块表 / inode 表）。
    Table,
    /// 闸 5：零与横杠。
    Zero,
    /// 闸 6：取整与单位。
    Round,
    /// 闸 7：移交登记。
    Hand,
    /// 闸 8：退出码结算。
    Exit,
}

impl Stage {
    /// 秩（1 起算，`Gate` 单调比较用）。
    pub fn rank(&self) -> u32 {
        match self {
            Stage::Argv => 1u32,
            Stage::Resolve => 2u32,
            Stage::Read => 3u32,
            Stage::Table => 4u32,
            Stage::Zero => 5u32,
            Stage::Round => 6u32,
            Stage::Hand => 7u32,
            Stage::Exit => 8u32,
        }
    }

    /// 反查秩。
    pub fn from_rank(r: u32) -> Option<Stage> {
        match r {
            1u32 => Some(Stage::Argv),
            2u32 => Some(Stage::Resolve),
            3u32 => Some(Stage::Read),
            4u32 => Some(Stage::Table),
            5u32 => Some(Stage::Zero),
            6u32 => Some(Stage::Round),
            7u32 => Some(Stage::Hand),
            8u32 => Some(Stage::Exit),
            _ => None,
        }
    }

    /// 八闸顺序全集。
    pub fn all() -> [Stage; 8] {
        [
            Stage::Argv,
            Stage::Resolve,
            Stage::Read,
            Stage::Table,
            Stage::Zero,
            Stage::Round,
            Stage::Hand,
            Stage::Exit,
        ]
    }

    /// 序号（0 起算，档位标签用）。
    pub fn index(&self) -> usize {
        (self.rank() - 1) as usize
    }

    /// 短名。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Argv => "argv",
            Stage::Resolve => "resolve",
            Stage::Read => "read",
            Stage::Table => "table",
            Stage::Zero => "zero",
            Stage::Round => "round",
            Stage::Hand => "hand",
            Stage::Exit => "exit",
        }
    }

    /// 这一闸之后表才存在（前三闸都没有表）。
    pub fn table_exists(&self) -> bool {
        self.rank() >= Stage::Table.rank()
    }

    /// 是否本卡呈现锚允许的落闸。
    pub fn anchorable(&self) -> bool {
        matches!(self, Stage::Zero | Stage::Round | Stage::Hand | Stage::Exit)
    }
}

// ---------------------------------------------------------------------------
// 注解（呈现档）
// ---------------------------------------------------------------------------

/// 一格报表的**呈现档**。本卡的全部深度集中在这一族枚举上：
/// `0` 与 `-` 是两个档，两个不同的运维结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// `avail` 表真出 `0`（本卡 J1 的正现场）。
    AvailZero,
    /// `avail` 表出 `0`，但那个 0 是保留段扣出来的（`f_bfree > 0`）。
    ReservedShown,
    /// `avail` 表出非零（盘还没满，或满不在这一张表上）。
    AvailNonZero,
    /// inode 表的 `IFree` 出 `0`。
    InodeZero,
    /// inode 表整行横杠（该文件系统没有 inode 计数）。
    InodeDash,
    /// 块表整行横杠（空间不适用：伪文件系统 / 不可测挂载）。
    DashCell,
    /// `Use%` 向上取整到 100，而 `avail` 还非零（合法现场）。
    PercentCeil,
    /// 呈现面忠实，但写侧已经递来一个 28（移交登记）。
    HandoffMarked,
    /// 带 `--total`：多一行合计，合计行不代表任何一块盘。
    TotalRow,
    /// 单位换算档（`-h` / `--si`）：同一个块数的两种印法。
    UnitScaled,
    /// GNU 的 `cannot stat` 行（表根本没出）。
    GnuCannotStat,
    /// busybox 的 `can't find mount point` 行。
    BbNoMount,
    /// 取数阶段就红（权限/溢出），表未出。
    ReadDenied,
    /// 陈旧句柄（NFS 掉线），表未出。
    StaleHandle,
    /// `df` 报出空间码：本卡点名判死的假象。
    SpaceLie,
    /// 混血版式：逐字比对不成立。
    MixedFamily,
    /// 用法行（表未出）。
    UsageLine,
    /// 不裁定（别的工具的表）。
    OutOfScope,
}

impl Note {
    /// 序号（`enums_ok` 复算连续性与互异）。
    pub fn index(&self) -> usize {
        match self {
            Note::AvailZero => 0usize,
            Note::ReservedShown => 1usize,
            Note::AvailNonZero => 2usize,
            Note::InodeZero => 3usize,
            Note::InodeDash => 4usize,
            Note::DashCell => 5usize,
            Note::PercentCeil => 6usize,
            Note::HandoffMarked => 7usize,
            Note::TotalRow => 8usize,
            Note::UnitScaled => 9usize,
            Note::GnuCannotStat => 10usize,
            Note::BbNoMount => 11usize,
            Note::ReadDenied => 12usize,
            Note::StaleHandle => 13usize,
            Note::SpaceLie => 14usize,
            Note::MixedFamily => 15usize,
            Note::UsageLine => 16usize,
            Note::OutOfScope => 17usize,
        }
    }

    /// 全集（顺序与 `index` 一致）。
    pub fn all() -> [Note; 18] {
        [
            Note::AvailZero,
            Note::ReservedShown,
            Note::AvailNonZero,
            Note::InodeZero,
            Note::InodeDash,
            Note::DashCell,
            Note::PercentCeil,
            Note::HandoffMarked,
            Note::TotalRow,
            Note::UnitScaled,
            Note::GnuCannotStat,
            Note::BbNoMount,
            Note::ReadDenied,
            Note::StaleHandle,
            Note::SpaceLie,
            Note::MixedFamily,
            Note::UsageLine,
            Note::OutOfScope,
        ]
    }

    /// 短名（逐字进文案与指纹）。
    pub fn name(&self) -> &'static str {
        match self {
            Note::AvailZero => "avail-zero",
            Note::ReservedShown => "avail-zero-reserved",
            Note::AvailNonZero => "avail-nonzero",
            Note::InodeZero => "ifree-zero",
            Note::InodeDash => "inode-table-dash",
            Note::DashCell => "space-not-applicable",
            Note::PercentCeil => "pct-ceil-100",
            Note::HandoffMarked => "write-handoff-noted",
            Note::TotalRow => "total-row",
            Note::UnitScaled => "unit-scaled",
            Note::GnuCannotStat => "gnu-cannot-stat",
            Note::BbNoMount => "bb-no-mount-point",
            Note::ReadDenied => "read-denied",
            Note::StaleHandle => "stale-handle",
            Note::SpaceLie => "df-claims-enospc",
            Note::MixedFamily => "mixed-format",
            Note::UsageLine => "usage-line",
            Note::OutOfScope => "out-of-scope",
        }
    }

    /// 是否用法档（呈现锚永不给这一档）。
    pub fn usage(&self) -> bool {
        matches!(self, Note::UsageLine | Note::OutOfScope)
    }

    /// 是否"表根本没渲染"档（三档未落盘）。
    pub fn untouched(&self) -> bool {
        matches!(
            self,
            Note::GnuCannotStat
                | Note::BbNoMount
                | Note::ReadDenied
                | Note::StaleHandle
                | Note::SpaceLie
        )
    }

    /// 是否 avail 表的零（本卡呈现锚的正对象）。
    pub fn avail_zero(&self) -> bool {
        matches!(self, Note::AvailZero | Note::ReservedShown)
    }

    /// 是否横杠档（`-` 而不是 `0`）。
    pub fn dashed(&self) -> bool {
        matches!(self, Note::DashCell | Note::InodeDash)
    }

    /// 是否 inode 表档。
    pub fn on_inode_table(&self) -> bool {
        matches!(self, Note::InodeZero | Note::InodeDash)
    }

    /// 是否"报表好看但写不进去"的移交档（F11688 递过来的那一类）。
    pub fn is_handoff(&self) -> bool {
        matches!(self, Note::HandoffMarked)
    }

    /// 是否需要落"df 说谎"备注（移交 + 元数据分离两档）。
    pub fn table_looks_fine(&self) -> bool {
        matches!(
            self,
            Note::AvailNonZero | Note::PercentCeil | Note::HandoffMarked | Note::UnitScaled
        )
    }
}

// ---------------------------------------------------------------------------
// 档位（呈现四档）
// ---------------------------------------------------------------------------

/// 一格报表落在哪一档。`total` 与 `good` 两维独立，见 `Row`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 一档：`avail` 表如实出 `0`——本卡 J1 的正现场。
    ZeroShown,
    /// 二档：其它如实呈现（非零、横杠、inode 零、取整、移交）。
    Faithful,
    /// 三档：表根本没渲染（用法错、解析失败、取数失败）。
    Unread,
    /// 四档：谎报（结构不自洽：混血版式、`df` 报空间码）。
    Refused,
}

impl Tier {
    /// 序号。
    pub fn index(&self) -> usize {
        match self {
            Tier::ZeroShown => 0usize,
            Tier::Faithful => 1usize,
            Tier::Unread => 2usize,
            Tier::Refused => 3usize,
        }
    }

    /// 反查。
    pub fn from_index(i: usize) -> Option<Tier> {
        match i {
            0usize => Some(Tier::ZeroShown),
            1usize => Some(Tier::Faithful),
            2usize => Some(Tier::Unread),
            3usize => Some(Tier::Refused),
            _ => None,
        }
    }

    /// 档位名（矩阵渲染里出现的那一串）。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::ZeroShown => "avail-zero",
            Tier::Faithful => "faithful",
            Tier::Unread => "no-table",
            Tier::Refused => "refused",
        }
    }

    /// 是否"报表出来了"的两档。
    pub fn rendered(&self) -> bool {
        matches!(self, Tier::ZeroShown | Tier::Faithful)
    }
}

// ---------------------------------------------------------------------------
// 判据缺陷维（judge 的返回）
// ---------------------------------------------------------------------------

/// 判序六维 + 结构性附加维。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 六维全对。
    Ok,
    /// 成败互报（申报的绿红与实际不一致）。
    Estate,
    /// 落闸错位（说成取数之前就有数）。
    Stage,
    /// errno 错位（`ENOENT` 与 `ENOSPC` 互换是本卡一号假）。
    Code,
    /// 呈现值错位（数字与横杠哨兵对不上）。
    Value,
    /// 呈现档错位（`-`/`0`/取整/移交混认）。
    Note,
    /// 锚位错位（该落不落、不该落却落）。
    Default,
    /// 几何矛盾（块数/保留段/可测性打架）。
    Geom,
    /// 状态违例（关闸后再走、冻结后再登记）。
    State,
    /// 计数失衡。
    Count,
    /// 槽位非法。
    Slot,
    /// 指纹不符。
    Seal,
    /// 无观测。
    Nob,
    /// 矩阵不平。
    Matrix,
}

impl Fault {
    /// 全集（顺序固定，与 13 个错误码一一映射）。
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

    /// 缺陷维对应的错误码（本卡 13 个非绿维各占一码，不共用）。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0u32,
            Fault::Estate => DF_ESTATE,
            Fault::Stage => DF_ESTAGE,
            Fault::Code => DF_ECODE,
            Fault::Value => DF_EVALUE,
            Fault::Note => DF_ENOTE,
            Fault::Default => DF_EDEFAULT,
            Fault::Geom => DF_EGEOM,
            Fault::State => DF_ECLOSE,
            Fault::Count => DF_ECOUNT,
            Fault::Slot => DF_ESLOT,
            Fault::Seal => DF_ESEAL,
            Fault::Nob => DF_ENOBS,
            Fault::Matrix => DF_EMATRIX,
        }
    }

    /// 是否绿。
    pub fn alive(&self) -> bool {
        matches!(self, Fault::Ok)
    }

    /// 是否本卡判序内的六维之一。
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

/// 缺陷维短名。
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

// ---------------------------------------------------------------------------
// 盘的几何（呈现面的物理证据）
// ---------------------------------------------------------------------------

/// 一次 `statfs` 拿回来的原始量。本卡**只存原始量**，一切格子都由它推。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Geom {
    /// `f_blocks`：总块数（伪文件系统上是 0）。
    pub total_blocks: u32,
    /// `f_bfree`：真空闲块，**含** root 保留段。
    pub free_blocks: u32,
    /// 保留段块数（只有 ext 家族非零）。
    pub reserved_blocks: u32,
    /// inode 总量（`df -i` 的 Inodes）。
    pub total_inodes: u32,
    /// 已用 inode（`df -i` 的 IUsed）。
    pub used_inodes: u32,
    /// 种类（决定有没有保留段、有没有 inode、可不可测）。
    pub fs: FsKind,
}

impl Geom {
    /// 空几何：全零 + ext4，只作 `const fn` 初值用。
    pub const fn blank() -> Geom {
        Geom {
            total_blocks: 0,
            free_blocks: 0,
            reserved_blocks: 0,
            total_inodes: 0,
            used_inodes: 0,
            fs: FsKind::Ext4,
        }
    }

    /// `Available` 那一格的原料：`f_bavail = f_bfree - 保留段`（不会为负）。
    pub const fn avail(&self) -> u32 {
        self.free_blocks.saturating_sub(self.reserved_blocks)
    }

    /// `Used` 那一格的原料：`f_blocks - f_bfree`。
    pub const fn used(&self) -> u32 {
        self.total_blocks.saturating_sub(self.free_blocks)
    }

    /// `IFree` 那一格的原料。
    pub const fn free_inodes(&self) -> u32 {
        self.total_inodes.saturating_sub(self.used_inodes)
    }

    /// 结构是否自洽（越界与非法保留段都在这关压死）。
    pub fn well_formed(&self) -> bool {
        if self.free_blocks > self.total_blocks {
            return false;
        }
        if self.reserved_blocks > self.free_blocks {
            return false;
        }
        if self.used_inodes > self.total_inodes {
            return false;
        }
        if self.reserved_blocks > 0 && !self.fs.has_reserved_blocks() {
            return false;
        }
        if self.total_inodes > 0 && !self.fs.has_inodes() {
            return false;
        }
        if self.fs == FsKind::Pseudo && self.total_blocks != 0 {
            return false;
        }
        true
    }

    /// 几何指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.total_blocks as u64);
        h = fold_word(h, self.free_blocks as u64);
        h = fold_word(h, self.reserved_blocks as u64);
        h = fold_word(h, self.total_inodes as u64);
        h = fold_word(h, self.used_inodes as u64);
        fold_byte(h, self.fs.index() as u8)
    }
}

/// 四张基准盘面（与基准机一致的那四个现场）。
pub fn geoms() -> [Geom; 4] {
    [
        // 0 真满盘：数据块一个都不剩。
        Geom {
            total_blocks: 1_000,
            free_blocks: 0,
            reserved_blocks: 0,
            total_inodes: 65_536,
            used_inodes: 128,
            fs: FsKind::Ext4,
        },
        // 1 保留段吃出来的 0：还有 1024 空闲块，全在保留段里。
        Geom {
            total_blocks: 10_000,
            free_blocks: 1_024,
            reserved_blocks: 1_024,
            total_inodes: 65_536,
            used_inodes: 4_096,
            fs: FsKind::Ext4,
        },
        // 2 inode 耗尽：块还有 12288，inode 用光。
        Geom {
            total_blocks: 20_000,
            free_blocks: 12_288,
            reserved_blocks: 0,
            total_inodes: 65_536,
            used_inodes: 65_536,
            fs: FsKind::Ext4,
        },
        // 3 伪文件系统：一切都不适用，四格横杠。
        Geom {
            total_blocks: 0,
            free_blocks: 0,
            reserved_blocks: 0,
            total_inodes: 0,
            used_inodes: 0,
            fs: FsKind::Pseudo,
        },
    ]
}

/// 取第 i 张基准盘面。
pub fn geom_at(i: usize) -> Option<Geom> {
    geoms().get(i).copied()
}

// ---------------------------------------------------------------------------
// Shape：一次 `df` 呈现的全部输入维度
// ---------------------------------------------------------------------------

/// 本卡输入形状。一条判据行 = 一个 Shape + 一份申报（Claim）。
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    /// 版式家族（决定字段名与错误串）。
    pub flavor: DfFlavor,
    /// 选的那张表（块表 / inode 表）。
    pub table: TableSel,
    /// 单位档（排版层）。
    pub unit: UnitMode,
    /// 参数（挂载点或路径），逐字进文案。
    pub mount: &'static str,
    /// 当时的盘面原始量。
    pub geom: Geom,
    /// 挂载点解析结果。
    pub target: TargetClass,
    /// 取数阶段实际返回的 errno（0 表示取到了）。
    pub read_errno: i32,
    /// F11688 递来的移交标记：写侧有一个 28，这张表却好看。
    pub write_handoff: bool,
    /// 选项位图。
    pub opts: u8,
    /// 操作数个数（0 个就是用法错）。
    pub operand_count: u32,
    /// 命令行本身合不合法。
    pub argv_legal: bool,
    /// 申报的退出码（exit 闸对账用）。
    pub exit_declared: u32,
}

impl Shape {
    /// 空形状：一块供给正常的 ext4 上的 GNU `df`（全绿），供 `const fn` 组装基型。
    pub const fn blank() -> Shape {
        Shape {
            flavor: DfFlavor::Gnu,
            table: TableSel::Block,
            unit: UnitMode::Kilobytes,
            mount: "/",
            geom: Geom::blank(),
            target: TargetClass::Mounted,
            read_errno: ERR_NONE,
            write_handoff: false,
            opts: 0u8,
            operand_count: 1u32,
            argv_legal: true,
            exit_declared: EXIT_OK,
        }
    }

    /// 取某位选项。
    pub const fn opt_has(&self, bit: u8) -> bool {
        self.opts & bit != 0
    }

    /// 本卡认得的位之外的野选项位。
    pub const fn unknown_opt_bits(&self) -> u8 {
        self.opts & !OPT_KNOWN
    }

    /// 是否 `-i`。
    pub const fn wants_inodes(&self) -> bool {
        self.opt_has(OPT_INODES)
    }

    /// 是否 `-P`。
    pub const fn wants_posix(&self) -> bool {
        self.opt_has(OPT_POSIX)
    }

    /// 是否 `--total`。
    pub const fn wants_total(&self) -> bool {
        self.opt_has(OPT_TOTAL)
    }

    /// 块表上空间可测吗（伪文件系统与零总块数都算不可测）。
    pub fn measured(&self) -> bool {
        self.geom.fs.measurable() && self.geom.total_blocks > 0
    }

    /// inode 表上有没有 inode 计数。
    pub fn inode_measured(&self) -> bool {
        self.geom.fs.has_inodes() && self.geom.total_inodes > 0
    }

    /// 当前表的可用格：横杠走 `DASH_CELL` 哨兵，**不折算成 0**。
    pub fn avail_cell(&self) -> u32 {
        match self.table {
            TableSel::Block => {
                if self.measured() {
                    self.geom.avail()
                } else {
                    DASH_CELL
                }
            }
            TableSel::Inode => {
                if self.inode_measured() {
                    self.geom.free_inodes()
                } else {
                    DASH_CELL
                }
            }
        }
    }

    /// 当前表的 Size/Inodes 格。
    pub fn size_cell(&self) -> u32 {
        match self.table {
            TableSel::Block => {
                if self.measured() {
                    self.geom.total_blocks
                } else {
                    DASH_CELL
                }
            }
            TableSel::Inode => {
                if self.inode_measured() {
                    self.geom.total_inodes
                } else {
                    DASH_CELL
                }
            }
        }
    }

    /// 当前表的 Used/IUsed 格。
    pub fn used_cell(&self) -> u32 {
        match self.table {
            TableSel::Block => {
                if self.measured() {
                    self.geom.used()
                } else {
                    DASH_CELL
                }
            }
            TableSel::Inode => {
                if self.inode_measured() {
                    self.geom.used_inodes
                } else {
                    DASH_CELL
                }
            }
        }
    }

    /// 百分比格（`Use%` / `IUse%`）：`None` 就是那一格出 `-`。
    ///
    /// GNU 的口径是**向上取整**：`ceil(used * 100 / (used + avail))`。
    /// 分母取 `used + avail` 而不是 `total`——这正是保留段会让百分比先冲到
    /// 100% 的原因。
    pub fn pct(&self) -> Option<u32> {
        match self.table {
            TableSel::Block => {
                if !self.measured() {
                    return None;
                }
                let used = self.geom.used() as u64;
                let avail = self.geom.avail() as u64;
                let denom = used + avail;
                if denom == 0 {
                    return None;
                }
                Some(ceil_pct(used, denom))
            }
            TableSel::Inode => {
                if !self.inode_measured() {
                    return None;
                }
                let used = self.geom.used_inodes as u64;
                let denom = self.geom.total_inodes as u64;
                if denom == 0 {
                    return None;
                }
                Some(ceil_pct(used, denom))
            }
        }
    }

    /// 用法是否不合法（命令行本身就不行）。
    pub fn usage_bad(&self) -> bool {
        if !self.argv_legal {
            return true;
        }
        if self.operand_count == 0 {
            return true;
        }
        false
    }

    /// 取数码是否合法（0、读侧集、或"呈现面禁用的空间码"——最后一种合法但自洽为假）。
    pub fn read_errno_legal(&self) -> bool {
        self.read_errno == ERR_NONE
            || errno_readable(self.read_errno)
            || errno_forbidden_on_df(self.read_errno)
    }

    /// 单位与选项是否配对（`-h` 没打却声明人类可读单位，是形状错）。
    pub fn unit_matches_opts(&self) -> bool {
        match self.unit {
            UnitMode::Kilobytes => {
                !self.opt_has(OPT_HUMAN) && !self.opt_has(OPT_SI) && !self.opt_has(OPT_OUTPUT)
            }
            UnitMode::Human => self.opt_has(OPT_HUMAN) && !self.opt_has(OPT_SI),
            UnitMode::Si => self.opt_has(OPT_SI) && !self.opt_has(OPT_HUMAN),
            UnitMode::PosixBlocks => self.opt_has(OPT_POSIX),
            UnitMode::Chosen => self.opt_has(OPT_OUTPUT),
        }
    }

    /// 选表与选项是否配对（`-i` 是唯一能换表的开关）。
    pub fn table_matches_opts(&self) -> bool {
        match self.table {
            TableSel::Block => !self.wants_inodes(),
            TableSel::Inode => self.wants_inodes(),
        }
    }

    /// 家族与 POSIX 单位是否冲突（POSIX 家族只会印 `1024-blocks`）。
    pub fn flavor_matches_unit(&self) -> bool {
        match self.flavor {
            DfFlavor::Posix => matches!(self.unit, UnitMode::PosixBlocks),
            _ => !matches!(self.unit, UnitMode::PosixBlocks) || self.wants_posix(),
        }
    }

    /// 目标与解析是否自洽：解析失败的档位必须带一个非零邻居码。
    pub fn target_consistent(&self) -> bool {
        if self.target.blocks_table() {
            return self.target.errno() != ERR_NONE;
        }
        true
    }

    /// 名字长度是否合法（超长路径应当先被 `TooLong` 接住，不该走到渲染）。
    pub fn mount_legal(&self) -> bool {
        (self.mount.chars().count() as u32) <= MOUNT_NAME_MAX
    }

    /// 形状整体是否合法。不合法的形状把 `consistent` 压成假。
    pub fn valid(&self) -> bool {
        if !self.geom.well_formed() {
            return false;
        }
        if !self.flavor.verbatim() {
            return false;
        }
        if !self.read_errno_legal() {
            return false;
        }
        if errno_forbidden_on_df(self.read_errno) {
            return false;
        }
        if !self.unit_matches_opts() {
            return false;
        }
        if !self.table_matches_opts() {
            return false;
        }
        if !self.flavor_matches_unit() {
            return false;
        }
        if !self.target_consistent() {
            return false;
        }
        if !self.mount_legal() {
            return false;
        }
        if self.unknown_opt_bits() != 0 {
            return false;
        }
        if self.usage_bad() {
            return self.target == TargetClass::Foreign;
        }
        true
    }

    /// 复制并只换盘面。
    pub fn with_geom(&self, i: usize) -> Shape {
        let mut s = *self;
        if let Some(g) = geom_at(i) {
            s.geom = g;
        }
        s
    }

    /// 复制并只换家族。
    pub fn with_flavor(&self, f: DfFlavor) -> Shape {
        let mut s = *self;
        s.flavor = f;
        s
    }

    /// 复制并只换取数码。
    pub fn clone_errno(&self, e: i32) -> Shape {
        let mut s = *self;
        s.read_errno = e;
        s
    }

    /// 复制并只换目标解析档。
    pub fn with_target(&self, t: TargetClass) -> Shape {
        let mut s = *self;
        s.target = t;
        s
    }

    /// 形状指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_byte(h, self.flavor.index() as u8);
        h = fold_byte(h, self.table.index() as u8);
        h = fold_byte(h, self.unit.index() as u8);
        h = fold_str(h, self.mount);
        h = fold_word(h, self.geom.seal());
        h = fold_str(h, self.target.name());
        h = fold_i32(h, self.read_errno);
        h = fold_bool(h, self.write_handoff);
        h = fold_word(h, self.opts as u64);
        h = fold_word(h, self.operand_count as u64);
        h = fold_bool(h, self.argv_legal);
        fold_word(h, self.exit_declared as u64)
    }
}

/// GNU 的向上取整百分比：`floor(n * 100 / d)`，只要有余数就进 1。
fn ceil_pct(n: u64, d: u64) -> u32 {
    if d == 0 {
        return 0;
    }
    let scaled = n.saturating_mul(PCT_BASE as u64);
    let mut q = scaled / d;
    if scaled % d != 0 {
        q += 1;
    }
    if q > 100 {
        return 100;
    }
    q as u32
}

// ---------------------------------------------------------------------------
// Outcome / Claim / Case
// ---------------------------------------------------------------------------

/// 一次 `df` 的**实际**呈现结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 定稿落在哪道闸。
    pub stage: Stage,
    /// 读侧 errno（0 表示这张表出来了）。
    pub code: i32,
    /// 呈现值：那一格的数字，或 `DASH_CELL` 哨兵。
    pub value: u32,
    /// 呈现档。
    pub note: Note,
    /// 本卡呈现锚。
    pub default: bool,
    /// 结构自洽（混血版式、`df` 报空间码都把它压成假）。
    pub consistent: bool,
}

impl Outcome {
    /// 全零初值。
    pub const fn blank() -> Outcome {
        Outcome {
            stage: Stage::Exit,
            code: ERR_NONE,
            value: 0u32,
            note: Note::AvailNonZero,
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

    /// 是否绿（表真出来了且结构自洽）。
    pub fn ok(&self) -> bool {
        self.code == ERR_NONE && self.consistent
    }

    /// 是否红。
    pub fn failed(&self) -> bool {
        self.code != ERR_NONE
    }

    /// 是否落了本卡锚。
    pub fn anchored(&self) -> bool {
        self.default
    }

    /// 是否谎报。
    pub fn refused(&self) -> bool {
        !self.consistent
    }

    /// 那一格是不是横杠。
    pub fn dashed(&self) -> bool {
        self.value == DASH_CELL
    }

    /// 应然退出码：绿 0、用法 2、读侧错 1、不裁定给占位 99。
    pub fn exit(&self) -> u32 {
        if self.note.usage() {
            return EXIT_USAGE;
        }
        if self.note == Note::OutOfScope {
            return EXIT_UNJUDGED;
        }
        if self.ok() {
            return EXIT_OK;
        }
        EXIT_READ
    }

    /// 该不该落锚（复算一遍，供 `default` 自检）：表出来、块表、可测、
    /// 那一格真是数字 0、结构自洽。
    pub fn should_anchor(&self) -> bool {
        if !self.consistent || self.code != ERR_NONE {
            return false;
        }
        if !self.stage.anchorable() {
            return false;
        }
        self.note.avail_zero()
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

/// 申报（外部读到的那一段账）：与 `Outcome` 同构，少了 `consistent`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    /// 申报定稿在哪道闸。
    pub stage: Stage,
    /// 申报的 errno。
    pub code: i32,
    /// 申报的呈现值。
    pub value: u32,
    /// 申报的呈现档。
    pub note: Note,
    /// 申报的锚位。
    pub default: bool,
}

impl Claim {
    /// 全零初值。
    pub const fn blank() -> Claim {
        Claim {
            stage: Stage::Exit,
            code: ERR_NONE,
            value: 0u32,
            note: Note::AvailNonZero,
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
    /// 短名（标签必须挂本卡号）。
    pub tag: &'static str,
}

impl Case {
    /// 空语料格。
    pub const fn blank() -> Case {
        Case {
            shape: Shape::blank(),
            claim: Claim::blank(),
            tag: "f11689-blank",
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

    /// 判定。
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

// CHUNK-E-PLACEHOLDER

// ---------------------------------------------------------------------------
// define / judge
// ---------------------------------------------------------------------------

/// 由一份形状算出实测结论——本模块唯一的判定入口。
///
/// 八道闸 `Argv → Resolve → Read → Table → Zero → Round → Hand → Exit` 只能
/// 向前，第一个触发的闸定局。结构自洽（混血版式、`df` 报空间码）只把
/// `consistent` 压假，不挪落点——红留在它发生的闸上，归因才不必猜。
pub fn define(s: &Shape) -> Outcome {
    let consistent = s.valid();

    // 闸 1：命令行——用法不合法，什么表都不渲染（_foreign 的表归别的工具）。
    if s.usage_bad() {
        if s.target == TargetClass::Foreign {
            return Outcome::make(
                Stage::Argv,
                ERR_NONE,
                0u32,
                Note::OutOfScope,
                false,
                consistent,
            );
        }
        return Outcome::make(Stage::Argv, ERR_NONE, 0u32, Note::UsageLine, false, consistent);
    }

    // 闸 2：挂载点解析——解析就死了，`statfs` 根本没发生。
    if s.target.blocks_table() {
        let note = match s.flavor {
            DfFlavor::Busybox => Note::BbNoMount,
            _ => Note::GnuCannotStat,
        };
        return Outcome::make(Stage::Resolve, s.target.errno(), 0u32, note, false, consistent);
    }
    if s.target == TargetClass::Foreign {
        return Outcome::make(Stage::Resolve, ERR_NONE, 0u32, Note::OutOfScope, false, consistent);
    }

    // 闸 3：取数——读侧 errno 在这里定局；df 报空间码是本卡点名的假象。
    if s.read_errno != ERR_NONE {
        let note = if errno_forbidden_on_df(s.read_errno) {
            Note::SpaceLie
        } else if s.read_errno == ERR_ESTALE {
            Note::StaleHandle
        } else {
            Note::ReadDenied
        };
        return Outcome::make(Stage::Read, s.read_errno, 0u32, note, false, consistent);
    }

    // 闸 4：选表——表/单位/家族与选项不配对就是混血版式，逐字比对不成立。
    if !s.table_matches_opts() || !s.unit_matches_opts() || !s.flavor_matches_unit() {
        return Outcome::make(Stage::Table, ERR_NONE, 0u32, Note::MixedFamily, false, consistent);
    }

    // 闸 5：零与横杠——本卡主锚的落点。横杠走 `DASH_CELL` 哨兵，绝不折算成零。
    let cell = s.avail_cell();
    if cell == DASH_CELL {
        let note = match s.table {
            TableSel::Block => Note::DashCell,
            TableSel::Inode => Note::InodeDash,
        };
        return Outcome::make(Stage::Zero, ERR_NONE, DASH_CELL, note, false, consistent);
    }
    if cell == 0u32 {
        if s.table == TableSel::Inode {
            return Outcome::make(Stage::Zero, ERR_NONE, 0u32, Note::InodeZero, false, consistent);
        }
        // 块表出 0：还有真空闲块就是保留段扣出来的那个 0，锚只落正现场。
        let note = if s.geom.free_blocks > 0u32 {
            Note::ReservedShown
        } else {
            Note::AvailZero
        };
        let anchored = note.avail_zero() && consistent;
        return Outcome::make(Stage::Zero, ERR_NONE, 0u32, note, anchored, consistent);
    }

    // 闸 6：取整与单位——`Use%` 进位到 100 而 avail 还非零是合法现场；
    // 人类可读/ SI 单位是同一条块数的另一种印法。
    if s.table == TableSel::Block {
        if let Some(p) = s.pct() {
            if p >= 100u32 && s.geom.avail() > 0u32 {
                return Outcome::make(
                    Stage::Round,
                    ERR_NONE,
                    s.geom.avail(),
                    Note::PercentCeil,
                    false,
                    consistent,
                );
            }
        }
    }
    if s.opt_has(OPT_HUMAN) || s.opt_has(OPT_SI) {
        return Outcome::make(
            Stage::Round,
            ERR_NONE,
            cell,
            Note::UnitScaled,
            false,
            consistent,
        );
    }

    // 闸 7：移交登记——呈现面忠实，但写侧已经递来一个 28（F11688）。
    if s.write_handoff {
        return Outcome::make(Stage::Hand, ERR_NONE, cell, Note::HandoffMarked, false, consistent);
    }

    // 闸 8：退出码结算——`--total` 的合计行不代表任何一块盘；其余如实非零。
    if s.opt_has(OPT_TOTAL) {
        return Outcome::make(Stage::Exit, ERR_NONE, cell, Note::TotalRow, false, consistent);
    }
    Outcome::make(Stage::Exit, ERR_NONE, cell, Note::AvailNonZero, false, consistent)
}

/// 槽级对判：申报对实测逐维比，判序固定
/// 成败 → 落点闸 → errno → 呈现值 → 呈现档 → 锚位；六维全对才给 `Ok`。
pub fn judge(c: &Case) -> Fault {
    let a = define(&c.shape);
    let k = &c.claim;
    if k.success() != a.ok() {
        return Fault::Estate;
    }
    if k.stage != a.stage {
        return Fault::Stage;
    }
    if k.code != a.code {
        return Fault::Code;
    }
    if k.value != a.value {
        return Fault::Value;
    }
    if k.note != a.note {
        return Fault::Note;
    }
    if k.default != a.default {
        return Fault::Default;
    }
    Fault::Ok
}




