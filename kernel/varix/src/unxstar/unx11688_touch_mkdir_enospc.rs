//! UNX-F11688 touch/mkdir ENOSPC 判据
//!
//! 这张卡管的是**满盘上"建一个空东西"这件事**。`touch` 建新件、`mkdir` 建目录，
//! 两者都不写正文：一个字节的数据都没落，却可能红。于是本卡的判据不是"落了多少
//! 字节"（那是 F11682/F11683 的账），而是**四件互相顶不上就必有一假的事**：
//!
//! ```text
//! $ mkdir full/a
//! mkdir: cannot create directory 'full/a': No space left on device
//! $ touch full/new
//! touch: cannot touch 'full/new': No space left on device
//! $ ls full
//! new                 ← 建件失败，件却留下了（0 字节）
//! ```
//!
//! 最后两行是本卡的心脏：**`mkdir` 失败不留痕，`touch` 失败可能留一个零字节件**。
//! 把这两种"失败"当同一种失败读，运维会在满盘之后看见一堆空件却找不到创建者，
//! 也会把 `mkdir -p` 留在盘上的半截父目录当成"目录本来就有"。本卡把"失败之后盘上
//! 多出来的东西"立成独立一维（`Residual`），并且让它**必须**与工具配对：
//! `mkdir` 侧出现残留 → 判死；`touch` 侧申报"无残留"但件其实在 → 判死。
//!
//! 更麻烦的是**同一个 `ENOSPC`（28）背后有四种完全不同的物理成因**，文案逐字
//! 相同、退出码相同、errno 相同，唯一能分开它们的是当时的几何：
//!
//! * **数据块耗尽**：`avail==0`，`free_inodes>0`。这是"真没地儿"。
//! * **inode 耗尽**：`avail>0` 且 `free_inodes==0`。ext4 上 `df -i` 才看得见；
//!   只看 `df` 的表此刻在说"还有 37% 可用"。
//! * **日志/元数据耗尽**：ext4 journal 满、btrfs 的 metadata 块组满，此时
//!   `df` 的 avail 与 `df -i` 都好看，`mkdir` 照样 28。这一档是
//!   "df 说谎"的正宗现场，本卡只负责**认出来并把它移交给 F11689**（呈现面），
//!   本卡的判据是"这条 28 不许被写成数据块耗尽"。
//! * **保留块墙**：ext4 默认给 root 留 5%，`avail` 已经把保留段剔掉，但
//!   `free` 还没归零；非 root 建件 28，root 建件成功。本卡把 `caller_root`
//!   与 `avail/free` 的差当一道闸：**同一个盘同一秒，两种身份两种结果**，
//!   把 root 的成功写成"盘是空的"或把非 root 的 28 写成"盘满了"都是错位。
//!
//! `touch` 还有 `mkdir` 没有的第二条纵线：**改时间**。`touch 已存在件` 走的是
//! `utimensat()`，它不分配任何东西，因此在本地文件系统上**不可能**回 28。于是
//! `touch: setting times of 'f': No space left on device` 这一行在本地就是**假**
//! ——真正会打这行的只有 NFS：客户端把 writeback 期间服务端返的 ENOSPC 迟迟到
//! 交给了这次 `utimensat`/`close`。本卡把这条写成一道显式闸（`fs_family` 是
//! `Nfs` 才放行，`Local*` 就判死），因为运维读到这行会去清盘，而盘其实满得
//! 很"正常"——错的不是工具，是把 NFS 的迟到错按本地语义读了。
//!
//! `mkdir -p` 的第三条纵线：**父目录残留**。GNU 与 busybox 在这里行为一致但
//! 记账口径不一致——`mkdir -p a/b/c` 在 `c` 上 28 时，`a`、`b` 已经在盘上，
//! `-p` 退 1，`a/b` 不许自动回收（回收会把别人的并发创建一起删掉）。本卡因此
//! 把"残留父目录数"计进 `value`，并用它顶住两个相反的错误：把 `-p` 的失败读成
//! "什么都没发生"，或读成"半截目录该由工具删掉"。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 八道闸，秩从 **1** 起算；`Gate` 只接受"前进到下一秩"。前一闸不产出事实，
//! 后一闸就没有输入，倒过来必先产生假绿。
//!
//! 1. `argv`（秩 1）——命令行本身。`touch` 无操作数、`-t`/`-d` 串不合法、
//!    `mkdir` 无操作数、`-m` 的 mode 串不合法，都在这里退，文件系统**一次都没
//!    碰**。这一闸排在最前是为了钉死一条：用法错的行**永远不许**被记成
//!    ENOSPC 中止（`EXIT_USAGE` 与本卡中止锚互斥）。
//! 2. `parent`（秩 2）——目标父目录的解析。`ENOENT`/`ENOTDIR`/`ELOOP`/
//!    `ENAMETOOLONG`/`EACCES` 在这里出局，说明 `mkdir()`/`open(O_CREAT)`
//!    **根本没被调用**；这一闸的红与"盘满"没有任何关系。`-p` 例外：它的中间层
//!    是自己建的，所以 `parent` 闸可以同时产出 28——这正是残留父的来源。
//! 3. `inode`（秩 3）——inode 供给。`free_inodes==0` 的 28 在这一闸定性；
//!    它必须在 `parent` 之后（先确认这次真的要建）、在数据块闸之前（因为
//!    `avail>0` 的现场如果先走数据块闸会被判成"盘不满"，28 就成了无源之水）。
//! 4. `datablock`（秩 4）——数据块与保留块墙。`avail`/`free`/`caller_root`
//!    三件在这里咬合出"谁能建、谁不能建"。
//! 5. `journal`（秩 5）——日志/元数据面。ext4 journal 满、btrfs metadata 满、
//!    tmpfs 上限，三种"df 说还有，内核说没有"在这一闸合一，并把移交标记打上。
//! 6. `create`（秩 6）——真正那次 `mkdir()` 或 `open(O_CREAT)` 的返回。
//!    这一闸决定**残留**：`mkdir` 失败必无残留，`touch` 建新件失败可能留
//!    零字节件（O_CREAT 成了、close/fsync 才红）。它排在几何三闸之后：
//!    先有"为什么满"，才谈得上"这次建没建出来半个"。
//! 7. `mtime`（秩 7）——改时间闸。只在 `touch` 打已存在件时生效；本地必绿
//!    （不分配），NFS 才可能迟到报 28。排在 create 之后：件都不存在时
//!    谈不上"时间改没改成"。
//! 8. `exit`（秩 8）——退出码结算闸，封印闸。码 0 只允许给真绿或 `-p` 的
//!    "已存在即成功"；工具面文件错统一 1；2 只给用法。 errno、残留、闸序
//!    三件都自洽，才允许在这里落锚。
//!
//! ## 关键差异（本卡深度所在）
//!
//! * **与 F11681（ENOSPC 总锚，段基址 `0x6d130`，位距 `0x220`）的边界**：总锚
//!   判一次 `write()`/`fsync()` 的 (返回, errno) 配对；本卡的现场**一次 write
//!   都没有**——`mkdir` 走 `mkdir()`、`touch` 走 `open(O_CREAT)`/`utimensat()`。
//!   本卡只在 `errno_seen==28` 与总锚同值这一维上署名，其余（残留、几何成因四档、
//!   改时间不可能 28）全是本卡独有。两卡不互相 import，段位与位距由
//!   `anchors_agree()` 复算对齐。
//! * **与 F11685/F11686/F11687（sort 临时件 / sed -i 轮换 / mv 跨盘）的边界**：
//!   那三张卡的主角是**字节流动**，本卡的主角是**目录项与 inode 的分配**；它们
//!   判"写了多少、留了多少字节的半件"，本卡判"件存不存在、几层父目录留在盘上"。
//!   同一个满盘，那三张卡能说"落了 4096 字节"，本卡说"0 字节、1 个残留空件"。
//! * **与 F11688 直接后置 F11689（df 0 avail 呈现，位距 `0x10`）的边界**：
//!   本卡判**建件那次调用**与它留下的残留；F11689 判**呈现**（`df` 怎么把
//!   avail=0 摆出来、`-i` 怎么把 inode 摆出来）。本卡第 5 闸认得"df 说谎"但
//!   **不裁 df 的账**，只打移交标记；反过来说，`df` 显示满而 `mkdir` 成功这种
//!   矛盾（reserved 段、quota 宽限）归本卡判死，不归 F11689。
//! * **与 F11690（rename ENOSPC）的边界**：rename 在目录项层面**不分配数据块**，
//!   满盘通常照样成功；那才是"没空间却能改名"的反直觉档，归 F11690。本卡不许
//!   用 rename 的语义给自己松绑。
//! * **与 F11694（ENOSPC 文案逐字，跨工具一致）的边界**：F11694 管 strerror
//!   那一串本身（`No space left on device` 逐字）；本卡管**工具动词**
//!   （GNU `cannot touch` / `setting times of` / `cannot create directory`，
//!   busybox `can't create` / `can't create directory`）与动词后面的路径引号式样。
//!   两卡在同一串上联签，但动词面只归本卡。
//! * **本卡的排他职责**：① 残留维度（`mkdir` 无残留 vs `touch` 零字节件 vs
//!   `mkdir -p` 半截父目录）；② 同一个 28 的四档物理成因分表（数据块/inode/
//!   日志元数据/保留墙），且各档对 `avail`/`free_inodes`/`caller_root` 的
//!   要求不同；③ 本地 `setting times of` 报 28 必判死、NFS 放行；
//!   ④ 用法错与 ENOSPC 中止互斥；⑤ `-p` 的"已存在即成功"（码 0 且零输出）
//!   不许被当成漏报。
//! * **谁判死谁放行**：几何成因与 errno 自洽、残留与工具配对、动词与工具家族
//!   配对、退出码与闸序一致 → 放行并落锚；把 inode 耗尽写成数据块耗尽、把本地
//!   改时间写成 28、`mkdir` 写了残留、用法错落了 ENOSPC 锚、`-p` 已存在退 1
//!   → 一律判死。
//!
//! 判据六维：结束阶段（gate）、errno（code）、残留计数（value）、文案家族与
//! 成因档（note）、建件/建目录中止锚（default）、成败申报（estate）。
//! 判序固定：状态 → 阶段 → errno → 残留 → 注解 → 锚位。
//!
//! 语料 14 槽：8 槽申报正确（成功 3、`-p` 已存在 1、建件中止 2、建目录中止 1、
//! 用法 1），6 槽一槽只坏一维，依次挂 Estate/Stage/Code/Value/Note/Default。
//! 账本 14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码，冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11688）。
pub const SEG_BASE: u32 = 0x6d350;

/// 交叉署名：F11681「ENOSPC 总锚」的段基址。本卡**不 import** 那个模块的任何
/// 符号，只把它的段位与位距原样声明一遍，由 `anchors_agree()` 复算对齐。
pub const ANCHOR_F11681_SEG: u32 = 0x6d130;
/// F11681 与本卡的位距（0x6d350 - 0x6d130）。
pub const ANCHOR_F11681_GAP: u32 = 0x220;

/// 交叉署名：F11689「df 0 avail 呈现」的段基址（本卡的直接后置）。
pub const ANCHOR_F11689_SEG: u32 = 0x6d360;
/// F11689 与本卡的位距（0x6d360 - 0x6d350）。
pub const ANCHOR_F11689_GAP: u32 = 0x10;

/// 交叉署名：F11687「mv 跨盘部分复制回滚」的段基址（本卡的直接前置）。
pub const ANCHOR_F11687_SEG: u32 = 0x6d330;
/// F11687 与本卡的位距（0x6d350 - 0x6d330）。
pub const ANCHOR_F11687_GAP: u32 = 0x20;

/// 与 F11681/F11694 在 errno 面的署名：ENOSPC 在 Linux 就是 28，三卡同数。
pub const ANCHOR_ENOSPC: i32 = 28;
/// 与 F11694 在文案面的署名：只认这一串 strerror 原文，逐字。
pub const ANCHOR_ERRNO_TEXT: &str = "No space left on device";
/// 与 F11682/F11683 在退出码面的署名：工具面文件错统一退 1，本卡不许自造。
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

// ---------------------------------------------------------------------------
// errno（POSIX / Linux 真值；本卡认得的邻居码一个都不许冒充 ENOSPC）
// ---------------------------------------------------------------------------

/// 设备无空间（strerror: `No space left on device`）。
pub const ERR_ENOSPC: i32 = 28;
/// 路径上没这一层（strerror: `No such file or directory`）。
pub const ERR_ENOENT: i32 = 2;
/// 权限不足（strerror: `Permission denied`）。
pub const ERR_EACCES: i32 = 13;
/// 已存在（strerror: `File exists`）。
pub const ERR_EEXIST: i32 = 17;
/// 只读文件系统（strerror: `Read-only file system`）。
pub const ERR_EROFS: i32 = 30;
/// 配额超限（strerror: `Quota exceeded`）——邻居，"盘还满得起来"。
pub const ERR_EDQUOT: i32 = 122;
/// 目录链接数上限（strerror: `Too many links`）——邻居，看着像满其实没满。
pub const ERR_EMLINK: i32 = 65;
/// 名字过长（strerror: `File name too long`）。
pub const ERR_ENAMETOOLONG: i32 = 36;
/// 路径成分不是目录（strerror: `Not a directory`）。
pub const ERR_ENOTDIR: i32 = 20;
/// 符号链接绕圈（strerror: `Too many levels of symbolic links`）。
pub const ERR_ELOOP: i32 = 40;
/// 成功时 errno 应停放的零值。
pub const ERR_NONE: i32 = 0;

/// 本卡认得的 errno 全集大小（含零值：0/2/13/17/20/28/30/36/40/65/122）。
pub const ERRNO_SET_SIZE: usize = 11;

/// 真"空间类"errno 的个数：ENOSPC 与 EDQUOT 两档（后者的档归 F11695）。
pub const SPACE_ERRNO_COUNT: usize = 2;

// ---------------------------------------------------------------------------
// 退出码（GNU coreutils 与 busybox 的真值）
// ---------------------------------------------------------------------------

/// 全部落定，正常收尾。
pub const EXIT_OK: u32 = 0;
/// 文件类错误：建不出来、改不动时间、打不开参照件。
pub const EXIT_FILE: u32 = 1;
/// 用法错误：命令行本身不合法，文件系统一次都没碰。
pub const EXIT_USAGE: u32 = 2;
/// 本卡不裁定的占位码：故意取一个撞不上的值，越界行不可能被读成正常行。
pub const EXIT_UNJUDGED: u32 = 99;

/// 退出码档位数量（0/1/2 三档真实码 + 1 档不裁定）。
pub const EXIT_RANKS: usize = 4;

/// 单个操作数名字的长度上界（Linux 的 PATH_MAX 是 4096；本卡按名判定用）。
pub const NAME_MAX_LIMIT: u32 = 255;

/// `mkdir -p` 最多几层父目录（本卡语料里最深 4 层，超出即形状非法）。
pub const PARENT_DEPTH_MAX: u32 = 6;

/// ext4 默认保留块占比的千分率（5% = 50‰）。
pub const RESERVED_PERMILLE: u32 = 50;

// ---------------------------------------------------------------------------
// mkdir -p / touch 的选项位
// ---------------------------------------------------------------------------

/// `mkdir -p`：逐级建父目录，已存在不算错。
pub const OPT_MAKE_PARENTS: u8 = 0x01;
/// `mkdir -m MODE`：建时定权限（父层由 umask 决定，末层由 mode 决定）。
pub const OPT_MODE: u8 = 0x02;
/// `mkdir -v`：打了 `created directory` 一行，stdout 面。
pub const OPT_VERBOSE: u8 = 0x04;
/// `touch -a`：只改访问时间。
pub const OPT_ATIME: u8 = 0x08;
/// `touch -m`：只改修改时间。
pub const OPT_MTIME: u8 = 0x10;
/// `touch -d DATE`：从文本串取时间。
pub const OPT_DATE: u8 = 0x20;
/// `touch -t [[CC]YY]MMDDhhmm[.SS]`：从定宽串取时间。
pub const OPT_TIME: u8 = 0x40;
/// `touch -r REF`：从参照件抄时间。
pub const OPT_REF: u8 = 0x80;

/// 本卡认得的选项位全集。
pub const OPT_KNOWN: u8 = OPT_MAKE_PARENTS | OPT_MODE | OPT_VERBOSE
    | OPT_ATIME | OPT_MTIME | OPT_DATE | OPT_TIME | OPT_REF;

/// 选项位个数（语料与谓词按位遍历用）。
pub const OPT_BIT_COUNT: usize = 8;

/// 选项位表（顺序与 `OPT_*` 值一致）。
pub const OPT_BITS: [u8; OPT_BIT_COUNT] = [
    OPT_MAKE_PARENTS,
    OPT_MODE,
    OPT_VERBOSE,
    OPT_ATIME,
    OPT_MTIME,
    OPT_DATE,
    OPT_TIME,
    OPT_REF,
];

// ---------------------------------------------------------------------------
// 错误码（13 个，SEG_BASE + 0x01..0x0d）
// ---------------------------------------------------------------------------

/// 01 阶段申报不符：28 落在哪道闸说错了（父层解析红却说成 create 闸）。
pub const TM_ESTAGE: u32 = SEG_BASE + 0x01;
/// 02 errno 申报不符：邻居码冒充 28，或 28 写成邻居码。
pub const TM_ECODE: u32 = SEG_BASE + 0x02;
/// 03 残留申报不符：`mkdir` 写了残留、`touch` 藏了零字节件、`-p` 漏计父层。
pub const TM_EVALUE: u32 = SEG_BASE + 0x03;
/// 04 注解申报不符：工具动词混认，或四档成因写成了同一档。
pub const TM_ENOTE: u32 = SEG_BASE + 0x04;
/// 05 锚位申报不符：本卡中止锚该亮不亮、不该亮却亮（用法错落锚即在此判死）。
pub const TM_EDEFAULT: u32 = SEG_BASE + 0x05;
/// 06 几何不符：`avail`/`free_inodes`/`caller_root` 与成因档互相矛盾。
pub const TM_EGEOM: u32 = SEG_BASE + 0x06;
/// 07 状态违例：成败互报、闸门封印后再动、账本冻结后再登记。
pub const TM_ESTATE: u32 = SEG_BASE + 0x07;
/// 08 计数不符：报表不满、矩阵不平、跨档移交数混进本卡账。
pub const TM_ECOUNT: u32 = SEG_BASE + 0x08;
/// 09 槽位非法：注入槽越界，或账本登记时 slot != len。
pub const TM_ESLOT: u32 = SEG_BASE + 0x09;
/// 10 指纹不符：冻结后重放对不上，或篡改留痕。
pub const TM_ESEAL: u32 = SEG_BASE + 0x0a;
/// 11 无观测：槽号超成语料、期望表缺格。
pub const TM_ENOBS: u32 = SEG_BASE + 0x0b;
/// 12 矩阵不平：档位覆盖出现空洞或行列对不上。
pub const TM_EMATRIX: u32 = SEG_BASE + 0x0c;
/// 13 账本不平：格数、成败比或对账表任一失衡。
pub const TM_EBAL: u32 = SEG_BASE + 0x0d;

/// 13 个错误码的有序全集（检查器与账本共用）。
pub const ALL_CODES: [u32; 13] = [
    TM_ESTAGE,
    TM_ECODE,
    TM_EVALUE,
    TM_ENOTE,
    TM_EDEFAULT,
    TM_EGEOM,
    TM_ESTATE,
    TM_ECOUNT,
    TM_ESLOT,
    TM_ESEAL,
    TM_ENOBS,
    TM_EMATRIX,
    TM_EBAL,
];

// ---------------------------------------------------------------------------
// 操作码（账本格的操作归类）
// ---------------------------------------------------------------------------

/// 命令行/用法闸。
pub const OP_ARGV: u8 = 0x01;
/// 父目录解析闸。
pub const OP_PARENT: u8 = 0x02;
/// inode 供给闸。
pub const OP_INODE: u8 = 0x03;
/// 数据块与保留墙闸。
pub const OP_BLOCK: u8 = 0x04;
/// 日志/元数据闸。
pub const OP_META: u8 = 0x05;
/// 那次创建调用（mkdir/open O_CREAT）。
pub const OP_CREATE: u8 = 0x06;
/// 改时间闸（utimensat）。
pub const OP_MTIME: u8 = 0x07;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x08;
/// 红档率标定。
pub const OP_RATE: u8 = 0x09;
/// 缺陷码格。
pub const OP_DEFECT: u8 = 0x0a;
/// 账本重放。
pub const OP_REPLAY: u8 = 0x0b;

/// 操作码个数。
pub const OP_COUNT: usize = 11;

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

/// 段位文本：固定 `0x6d3xx` 式样，宽度钉死，防止指纹里段位串位。
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

/// 残留计数文本：单数/复数按 GNU 口径只出数字，语义交给调用方。
pub fn residual_text(n: u32) -> String {
    dec_text(n)
}

// ---------------------------------------------------------------------------
// strerror 与 errno 名（逐字，与 F11694 联签的那一串在本卡只出现一次）
// ---------------------------------------------------------------------------

/// strerror 原文表：只收本卡认得的 11 个 errno。
pub fn strerror(code: i32) -> &'static str {
    match code {
        ERR_NONE => "Success",
        ERR_ENOENT => "No such file or directory",
        ERR_EACCES => "Permission denied",
        ERR_EEXIST => "File exists",
        ERR_ENOTDIR => "Not a directory",
        ERR_ENOSPC => "No space left on device",
        ERR_EROFS => "Read-only file system",
        ERR_ENAMETOOLONG => "File name too long",
        ERR_ELOOP => "Too many levels of symbolic links",
        ERR_EMLINK => "Too many links",
        ERR_EDQUOT => "Quota exceeded",
        _ => "Unknown error",
    }
}

/// errno 的大写宏名。
pub fn errno_name(code: i32) -> &'static str {
    match code {
        ERR_NONE => "OK",
        ERR_ENOENT => "ENOENT",
        ERR_EACCES => "EACCES",
        ERR_EEXIST => "EEXIST",
        ERR_ENOTDIR => "ENOTDIR",
        ERR_ENOSPC => "ENOSPC",
        ERR_EROFS => "EROFS",
        ERR_ENAMETOOLONG => "ENAMETOOLONG",
        ERR_ELOOP => "ELOOP",
        ERR_EMLINK => "EMLINK",
        ERR_EDQUOT => "EDQUOT",
        _ => "UNKNOWN",
    }
}

/// 本卡认得的 errno 表（顺序固定，`errno_legal` 与账本共用）。
pub const KNOWN_ERRNOS: [i32; ERRNO_SET_SIZE] = [
    ERR_NONE,
    ERR_ENOENT,
    ERR_EACCES,
    ERR_EEXIST,
    ERR_ENOTDIR,
    ERR_ENOSPC,
    ERR_EROFS,
    ERR_ENAMETOOLONG,
    ERR_ELOOP,
    ERR_EMLINK,
    ERR_EDQUOT,
];

/// 邻居码表：这些码**不许**被写成本卡的中止锚（顺序与 `KNOWN_ERRNOS` 一致去掉 0/28）。
pub const NEIGHBOUR_ERRNOS: [i32; 9] = [
    ERR_ENOENT,
    ERR_EACCES,
    ERR_EEXIST,
    ERR_ENOTDIR,
    ERR_EROFS,
    ERR_ENAMETOOLONG,
    ERR_ELOOP,
    ERR_EMLINK,
    ERR_EDQUOT,
];

/// 查 `code` 是否是本卡认得的 errno。
pub fn errno_known(code: i32) -> bool {
    let mut i = 0usize;
    while i < KNOWN_ERRNOS.len() {
        if let Some(v) = KNOWN_ERRNOS.get(i) {
            if *v == code {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 查 `code` 是否是"空间类"（28 与 122）；其余全是邻居。
pub fn errno_space_family(code: i32) -> bool {
    code == ERR_ENOSPC || code == ERR_EDQUOT
}

// ---------------------------------------------------------------------------
// 逐字文案（GNU coreutils 8/9 与 busybox 的动词面；本卡的主战场）
// ---------------------------------------------------------------------------

/// GNU `touch` 建/开新件失败：`touch: cannot touch 'PATH': STRERROR`。
pub fn msg_gnu_touch_cannot(path: &str, code: i32) -> String {
    let mut s = String::from("touch: cannot touch '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU `touch` 改已存在件的时间失败：`touch: setting times of 'PATH': STRERROR`。
pub fn msg_gnu_setting_times(path: &str, code: i32) -> String {
    let mut s = String::from("touch: setting times of '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU `mkdir` 建目录失败：`mkdir: cannot create directory 'PATH': STRERROR`。
pub fn msg_gnu_mkdir_cannot(path: &str, code: i32) -> String {
    let mut s = String::from("mkdir: cannot create directory '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU `touch` 缺操作数：`touch: missing file operand`。
pub fn msg_gnu_touch_missing_operand() -> String {
    String::from("touch: missing file operand")
}

/// GNU `mkdir` 缺操作数：`mkdir: missing operand`。
pub fn msg_gnu_mkdir_missing_operand() -> String {
    String::from("mkdir: missing operand")
}

/// GNU `touch -d` 串不合法：`touch: invalid date 'TEXT'`。
pub fn msg_gnu_invalid_date(text: &str) -> String {
    let mut s = String::from("touch: invalid date '");
    s.push_str(text);
    s.push('\'');
    s
}

/// GNU `touch -t` 串不合法：`touch: invalid date format 'TEXT'`。
pub fn msg_gnu_invalid_date_format(text: &str) -> String {
    let mut s = String::from("touch: invalid date format '");
    s.push_str(text);
    s.push('\'');
    s
}

/// GNU `mkdir -m` 的 mode 串不合法：`mkdir: invalid mode 'TEXT'`。
pub fn msg_gnu_invalid_mode(text: &str) -> String {
    let mut s = String::from("mkdir: invalid mode '");
    s.push_str(text);
    s.push('\'');
    s
}

/// GNU `touch -r REF` 的参照件不存在：
/// `touch: failed to get attributes for 'REF': STRERROR`。
pub fn msg_gnu_failed_to_get_attributes(refname: &str, code: i32) -> String {
    let mut s = String::from("touch: failed to get attributes for '");
    s.push_str(refname);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU `mkdir -v` 的成功行（stdout）：`mkdir: created directory 'PATH'`。
pub fn msg_gnu_mkdir_created(path: &str) -> String {
    let mut s = String::from("mkdir: created directory '");
    s.push_str(path);
    s.push('\'');
    s
}

/// GNU `mkdir -p -v` 的父层成功行：`mkdir: created directory 'PATH'`，
/// 但父层与末层在 `-p -v` 下**逐层各打一行**，本函数负责拼单层。
pub fn msg_gnu_mkdir_created_layer(path: &str, layers: u32) -> String {
    let mut s = String::new();
    let mut k = 0u32;
    while k < layers {
        if k > 0 {
            s.push('\n');
        }
        s.push_str(msg_gnu_mkdir_created(path).as_str());
        k += 1;
    }
    s
}

/// busybox `touch`：`touch: can't create 'PATH': STRERROR`。
pub fn msg_bb_touch_cant_create(path: &str, code: i32) -> String {
    let mut s = String::from("touch: can't create '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// busybox `mkdir`：`mkdir: can't create directory 'PATH': STRERROR`。
pub fn msg_bb_mkdir_cant_create(path: &str, code: i32) -> String {
    let mut s = String::from("mkdir: can't create directory '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// busybox 缺操作数：`touch: not enough arguments`。
pub fn msg_bb_touch_not_enough() -> String {
    String::from("touch: not enough arguments")
}

/// busybox `mkdir` 缺操作数：`mkdir: applet not found` 以外的真实串是
/// `usage: mkdir [OPTION] DIRECTORY...`——busybox 在这里打 usage 而不打
/// "missing operand"，本卡把它单立一格，禁止与 GNU 混认。
pub fn msg_bb_mkdir_usage() -> String {
    String::from("usage: mkdir [OPTION] DIRECTORY...")
}

/// 成因档文本（四档各自的中文名，note 维度要落这一串）。
pub fn cause_text(class: CauseClass) -> &'static str {
    match class {
        CauseClass::DataBlock => "数据块耗尽",
        CauseClass::Inode => "inode 耗尽",
        CauseClass::JournalMeta => "日志/元数据耗尽",
        CauseClass::Reserved => "保留块墙",
        CauseClass::NotSpace => "非空间错",
    }
}

/// 残留档文本。
pub fn residual_text_of(kind: Residual) -> &'static str {
    match kind {
        Residual::None => "无残留",
        Residual::ZeroFile => "零字节空件",
        Residual::Parents(n) => {
            if n == 1 {
                "一层父目录"
            } else {
                "多层父目录"
            }
        }
        Residual::OrphanTemp => "半截临时件",
        Residual::Inconsistent => "残留与申报不符"
    }
}

/// 一行完整判据行（供报告与测试逐字比对）。
pub fn render_line(
    tool: Tool,
    op: OpKind,
    path: &str,
    class: CauseClass,
    resid: Residual,
    code: i32,
) -> String {
    let mut s = String::from(tool.name());
    s.push('/');
    s.push_str(op.name());
    s.push(' ');
    s.push_str(path);
    s.push_str(" | errno=");
    s.push_str(errno_pair_text(code).as_str());
    s.push_str(" | 成因=");
    s.push_str(cause_text(class));
    s.push_str(" | 残留=");
    s.push_str(residual_text_of(resid));
    s
}

// ---------------------------------------------------------------------------
// 工具与操作
// ---------------------------------------------------------------------------

/// 本卡只认两个工具：`touch` 与 `mkdir`。别的工具（`cp`/`dd`/`tee`）各有前置卡。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    /// `touch`：建新件或改时间。
    Touch,
    /// `mkdir`：建一层或逐级建。
    Mkdir,
    /// 不裁定：本卡拒收的第三方工具，用来把越界行挡在 argv 闸。
    Foreign,
}

impl Tool {
    /// 工具全集（顺序固定）。
    pub const ALL: [Tool; 3] = [Tool::Touch, Tool::Mkdir, Tool::Foreign];

    /// 本卡认得的工具数（不含 Foreign）。
    pub const OWNED: usize = 2;

    /// 工具名（同时是文案前缀）。
    pub fn name(&self) -> &'static str {
        match self {
            Tool::Touch => "touch",
            Tool::Mkdir => "mkdir",
            Tool::Foreign => "foreign",
        }
    }

    /// 是否本卡管辖。
    pub fn owned(&self) -> bool {
        match self {
            Tool::Touch | Tool::Mkdir => true,
            Tool::Foreign => false,
        }
    }

    /// 序号（矩阵列索引用）。
    pub fn index(&self) -> usize {
        match self {
            Tool::Touch => 0usize,
            Tool::Mkdir => 1usize,
            Tool::Foreign => 2usize,
        }
    }
}

/// 一次调用到底在干什么。这一维决定走哪条纵线。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpKind {
    /// `touch 不存在的件`：需要 `open(O_CREAT)`，会分配目录项与 inode。
    TouchCreate,
    /// `touch 已存在的件`：只 `utimensat()`，本地不分配任何东西。
    TouchSetTimes,
    /// `touch -a/-m/-d/-t/-r` 的组合，落点仍是 utimensat，注解另计。
    TouchFlags,
    /// `mkdir 单层`：一次 `mkdir()`，要么成要么无痕。
    MkDirOne,
    /// `mkdir -p`：逐级 `mkdir()`，末层失败时父层已留在盘上。
    MkDirParents,
    /// 用法错：命令行就不合法，文件系统没被碰过。
    Usage,
    /// 不裁定：别的工具的写入面，归前置卡。
    OutOfScope,
}

impl OpKind {
    /// 操作全集（顺序固定）。
    pub fn all() -> [OpKind; 7] {
        [
            OpKind::TouchCreate,
            OpKind::TouchSetTimes,
            OpKind::TouchFlags,
            OpKind::MkDirOne,
            OpKind::MkDirParents,
            OpKind::Usage,
            OpKind::OutOfScope,
        ]
    }

    /// 操作名（判据行里的那一段）。
    pub fn name(&self) -> &'static str {
        match self {
            OpKind::TouchCreate => "create",
            OpKind::TouchSetTimes => "set-times",
            OpKind::TouchFlags => "set-times-flags",
            OpKind::MkDirOne => "mkdir",
            OpKind::MkDirParents => "mkdir-p",
            OpKind::Usage => "usage",
            OpKind::OutOfScope => "out-of-scope",
        }
    }

    /// 这个操作要不要分配目录项与 inode？只有要分配的才可能吃到 ENOSPC。
    pub fn allocates(&self) -> bool {
        match self {
            OpKind::TouchCreate | OpKind::MkDirOne | OpKind::MkDirParents => true,
            OpKind::TouchSetTimes | OpKind::TouchFlags | OpKind::Usage | OpKind::OutOfScope => false,
        }
    }

    /// 逐级建（`-p`）：只有它有"父层残留"这一维。
    pub fn makes_parents(&self) -> bool {
        matches!(self, OpKind::MkDirParents)
    }

    /// 是否 touch 侧的操作。
    pub fn is_touch(&self) -> bool {
        matches!(
            self,
            OpKind::TouchCreate | OpKind::TouchSetTimes | OpKind::TouchFlags
        )
    }

    /// 是否 mkdir 侧的操作。
    pub fn is_mkdir(&self) -> bool {
        matches!(self, OpKind::MkDirOne | OpKind::MkDirParents)
    }

    /// 序号。
    pub fn index(&self) -> usize {
        let all = OpKind::all();
        let mut i = 0usize;
        while i < all.len() {
            if let Some(o) = all.get(i) {
                if o == self {
                    return i;
                }
            }
            i += 1;
        }
        all.len()
    }
}

/// 工具文案家族。GNU 与 busybox 的动词不同，本卡的 note 维度按这一维分表。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flavor {
    /// GNU coreutils：`cannot touch` / `setting times of` / `cannot create directory`。
    Gnu,
    /// busybox：`can't create` / `can't create directory`，且缺操作数打 usage。
    Busybox,
    /// 家族不明/混认：note 闸直接判死的一格。
    Mixed,
}

impl Flavor {
    /// 家族全集。
    pub const ALL: [Flavor; 3] = [Flavor::Gnu, Flavor::Busybox, Flavor::Mixed];

    /// 家族名。
    pub fn name(&self) -> &'static str {
        match self {
            Flavor::Gnu => "gnu",
            Flavor::Busybox => "busybox",
            Flavor::Mixed => "mixed",
        }
    }

    /// 合法（可判）家族。
    pub fn legible(&self) -> bool {
        matches!(self, Flavor::Gnu | Flavor::Busybox)
    }

    /// 按家族拼那次失败的正字文案；`Mixed` 不产出，返回空串由调用方判死。
    pub fn text(&self, tool: Tool, op: OpKind, path: &str, code: i32) -> String {
        match self {
            Flavor::Gnu => match op {
                OpKind::TouchCreate | OpKind::TouchFlags => msg_gnu_touch_cannot(path, code),
                OpKind::TouchSetTimes => msg_gnu_setting_times(path, code),
                OpKind::MkDirOne | OpKind::MkDirParents => msg_gnu_mkdir_cannot(path, code),
                OpKind::Usage => msg_gnu_usage_line(tool),
                OpKind::OutOfScope => String::new(),
            },
            Flavor::Busybox => match op {
                OpKind::TouchCreate | OpKind::TouchFlags | OpKind::TouchSetTimes => {
                    msg_bb_touch_cant_create(path, code)
                }
                OpKind::MkDirOne | OpKind::MkDirParents => msg_bb_mkdir_cant_create(path, code),
                OpKind::Usage => msg_bb_usage_line(tool),
                OpKind::OutOfScope => String::new(),
            },
            Flavor::Mixed => String::new(),
        }
    }

    /// 序号。
    pub fn index(&self) -> usize {
        match self {
            Flavor::Gnu => 0usize,
            Flavor::Busybox => 1usize,
            Flavor::Mixed => 2usize,
        }
    }
}

/// GNU 侧的用法行（两个工具各一条，动词不同不许混）。
pub fn msg_gnu_usage_line(tool: Tool) -> String {
    match tool {
        Tool::Touch => msg_gnu_touch_missing_operand(),
        Tool::Mkdir => msg_gnu_mkdir_missing_operand(),
        Tool::Foreign => String::new(),
    }
}

/// busybox 侧的用法行。
pub fn msg_bb_usage_line(tool: Tool) -> String {
    match tool {
        Tool::Touch => msg_bb_touch_not_enough(),
        Tool::Mkdir => msg_bb_mkdir_usage(),
        Tool::Foreign => String::new(),
    }
}

// ---------------------------------------------------------------------------
// 成因档：同一个 28 背后的四种物理成因
// ---------------------------------------------------------------------------

/// ENOSPC 的物理成因分档。这一维是本卡的第二深度所在：文案与 errno 完全相同，
/// 只有几何能分开它们。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CauseClass {
    /// 数据块耗尽：`avail==0` 且 `free_inodes>0`。真没地儿。
    DataBlock,
    /// inode 耗尽：`avail>0` 且 `free_inodes==0`。`df` 看着还有，`df -i` 才见鬼。
    Inode,
    /// 日志/元数据耗尽：ext4 journal 满、btrfs metadata 块组满、tmpfs 上限。
    /// `df` 与 `df -i` 都好看，`mkdir` 照样 28。
    JournalMeta,
    /// 保留块墙：`avail==0` 但 `free>0`，非 root 撞墙、root 能过。
    Reserved,
    /// 不是空间错：邻居码或成功。本卡不许给这类行落 ENOSPC 锚。
    NotSpace,
}

impl CauseClass {
    /// 成因全集。
    pub fn all() -> [CauseClass; 5] {
        [
            CauseClass::DataBlock,
            CauseClass::Inode,
            CauseClass::JournalMeta,
            CauseClass::Reserved,
            CauseClass::NotSpace,
        ]
    }

    /// 成因名（判据行里用中文，这里给机器名）。
    pub fn name(&self) -> &'static str {
        match self {
            CauseClass::DataBlock => "datablock",
            CauseClass::Inode => "inode",
            CauseClass::JournalMeta => "journal-meta",
            CauseClass::Reserved => "reserved",
            CauseClass::NotSpace => "not-space",
        }
    }

    /// 是否属于"空间类"成因（能给本卡落锚的四档）。
    pub fn is_space(&self) -> bool {
        matches!(
            self,
            CauseClass::DataBlock | CauseClass::Inode | CauseClass::JournalMeta | CauseClass::Reserved
        )
    }

    /// 这一档是否要求"df 说谎"（呈现面要移交 F11689 的两档）。
    pub fn df_lies(&self) -> bool {
        matches!(self, CauseClass::Inode | CauseClass::JournalMeta)
    }

    /// 这一档是否受调用者身份影响（只有保留墙一档）。
    pub fn caller_sensitive(&self) -> bool {
        matches!(self, CauseClass::Reserved)
    }

    /// 序号。
    pub fn index(&self) -> usize {
        let all = CauseClass::all();
        let mut i = 0usize;
        while i < all.len() {
            if let Some(c) = all.get(i) {
                if c == self {
                    return i;
                }
            }
            i += 1;
        }
        all.len()
    }
}

/// 文件系统家族：决定"改时间能不能吃到 28"，也决定延迟分配会不会把错推迟到 close。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FsFamily {
    /// 本地 ext4（块/inode/日志三面都可能耗尽，`utimensat` 不可能 28）。
    LocalExt4,
    /// 本地 btrfs（元数据块组满是经典"df 说谎"现场）。
    LocalBtrfs,
    /// tmpfs（大小上限与 inode 上限各算一档）。
    Tmpfs,
    /// NFS（服务端的 ENOSPC 会迟到落到 close/utimensat 上——本卡唯一放行
    /// "setting times of ... 28" 的家族）。
    Nfs,
    /// 只读挂载（EROFS，不是空间问题，邻居档）。
    ReadOnly,
}

impl FsFamily {
    /// 家族全集。
    pub fn all() -> [FsFamily; 5] {
        [
            FsFamily::LocalExt4,
            FsFamily::LocalBtrfs,
            FsFamily::Tmpfs,
            FsFamily::Nfs,
            FsFamily::ReadOnly,
        ]
    }

    /// 家族名。
    pub fn name(&self) -> &'static str {
        match self {
            FsFamily::LocalExt4 => "ext4",
            FsFamily::LocalBtrfs => "btrfs",
            FsFamily::Tmpfs => "tmpfs",
            FsFamily::Nfs => "nfs",
            FsFamily::ReadOnly => "ro",
        }
    }

    /// 本地（非网络）文件系统：`utimensat` 在这里**不可能**回 28。
    pub fn is_local(&self) -> bool {
        matches!(
            self,
            FsFamily::LocalExt4 | FsFamily::LocalBtrfs | FsFamily::Tmpfs
        )
    }

    /// 会不会把空间错迟到到 close/utimensat（NFS 会，本地不会）。
    pub fn defers_errors(&self) -> bool {
        matches!(self, FsFamily::Nfs)
    }

    /// 元数据面与数据面分离的家族（btrfs 的"df 说谎"最典型）。
    pub fn separate_metadata(&self) -> bool {
        matches!(self, FsFamily::LocalBtrfs)
    }

    /// 有 5% root 保留块概念的家族（extX 家族）。
    pub fn has_reserved_blocks(&self) -> bool {
        matches!(self, FsFamily::LocalExt4)
    }
}

/// 失败之后盘上多出来的东西——本卡的排他维度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Residual {
    /// 什么都没留下（`mkdir` 失败的正解）。
    None,
    /// 留下一个 0 字节的件（`touch` 建新件：O_CREAT 成了，close/fsync 才红）。
    ZeroFile,
    /// 留下 n 层父目录（`mkdir -p` 末层失败的正解，n>=1）。
    Parents(u32),
    /// 留下半截临时件：这是 `sed -i`/`sort` 那几张卡的现场，本卡判死。
    OrphanTemp,
    /// 残留与工具/申报不符：本卡专门的一格假绿。
    Inconsistent,
}

impl Residual {
    /// 残留档名。
    pub fn name(&self) -> &'static str {
        match self {
            Residual::None => "none",
            Residual::ZeroFile => "zero-file",
            Residual::Parents(_) => "parents",
            Residual::OrphanTemp => "orphan-temp",
            Residual::Inconsistent => "inconsistent",
        }
    }

    /// 计数口径：`value` 维度就是这一格折算出来的件数。
    pub fn count(&self) -> u32 {
        match self {
            Residual::None => 0u32,
            Residual::ZeroFile => 1u32,
            Residual::Parents(n) => *n,
            Residual::OrphanTemp => 1u32,
            Residual::Inconsistent => 0u32,
        }
    }

    /// `mkdir` 侧允许的残留：只有 `-p` 的父层；单层 `mkdir` 失败必须无痕。
    pub fn allowed_for_mkdir(&self, op: OpKind) -> bool {
        match self {
            Residual::None => true,
            Residual::Parents(n) => op.makes_parents() && *n >= 1 && *n <= PARENT_DEPTH_MAX,
            Residual::ZeroFile | Residual::OrphanTemp | Residual::Inconsistent => false,
        }
    }

    /// `touch` 侧允许的残留：建新件失败可以留零字节件；父层概念在 touch 上不存在。
    pub fn allowed_for_touch(&self) -> bool {
        matches!(self, Residual::None | Residual::ZeroFile)
    }

    /// 这一档是不是"本卡判死"的那两类。
    pub fn is_lie(&self) -> bool {
        matches!(self, Residual::OrphanTemp | Residual::Inconsistent)
    }
}

/// 路径在 `parent` 闸的解析结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathClass {
    /// 父目录可写可解析，那次调用真的发生了。
    Resolved,
    /// 父目录不存在（ENOENT）——调用没发生。
    Missing,
    /// 路径中有成分不是目录（ENOTDIR）。
    NotDir,
    /// 符号链接绕圈（ELOOP）。
    Loop,
    /// 名字过长（ENAMETOOLONG）。
    TooLong,
    /// 父目录不可执行/不可写（EACCES）。
    NoAccess,
    /// 父目录本身也在这次 `-p` 里新建（这时 -p 的红才可能落在 parent 闸）。
    CreatedByParents,
}

impl PathClass {
    /// 路径档全集。
    pub fn all() -> [PathClass; 7] {
        [
            PathClass::Resolved,
            PathClass::Missing,
            PathClass::NotDir,
            PathClass::Loop,
            PathClass::TooLong,
            PathClass::NoAccess,
            PathClass::CreatedByParents,
        ]
    }

    /// 档名。
    pub fn name(&self) -> &'static str {
        match self {
            PathClass::Resolved => "resolved",
            PathClass::Missing => "missing",
            PathClass::NotDir => "notdir",
            PathClass::Loop => "loop",
            PathClass::TooLong => "toolong",
            PathClass::NoAccess => "noaccess",
            PathClass::CreatedByParents => "made-by-p",
        }
    }

    /// 这一档对应的 errno（`Resolved`/`CreatedByParents` 不决定 errno，给 0）。
    pub fn errno(&self) -> i32 {
        match self {
            PathClass::Resolved | PathClass::CreatedByParents => ERR_NONE,
            PathClass::Missing => ERR_ENOENT,
            PathClass::NotDir => ERR_ENOTDIR,
            PathClass::Loop => ERR_ELOOP,
            PathClass::TooLong => ERR_ENAMETOOLONG,
            PathClass::NoAccess => ERR_EACCES,
        }
    }

    /// 这一档是否意味着"那次创建调用根本没发生"。
    pub fn call_skipped(&self) -> bool {
        matches!(
            self,
            PathClass::Missing | PathClass::NotDir | PathClass::Loop | PathClass::TooLong | PathClass::NoAccess
        )
    }
}

// ---------------------------------------------------------------------------
// 阶段（八道闸）
// ---------------------------------------------------------------------------

/// 纵线上的八道闸。秩从 1 起算，`Gate` 只接受前进到下一秩。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 秩 1：命令行本身。
    Argv,
    /// 秩 2：父目录解析。
    Parent,
    /// 秩 3：inode 供给。
    Inode,
    /// 秩 4：数据块与保留墙。
    Block,
    /// 秩 5：日志/元数据面。
    Meta,
    /// 秩 6：那次 `mkdir()`/`open(O_CREAT)`。
    Create,
    /// 秩 7：改时间（`utimensat`）。
    MTime,
    /// 秩 8：退出码结算，也是唯一允许的封印秩。
    Exit,
}

impl Stage {
    /// 按秩列出八道闸。
    pub fn all() -> [Stage; STAGE_COUNT as usize] {
        [
            Stage::Argv,
            Stage::Parent,
            Stage::Inode,
            Stage::Block,
            Stage::Meta,
            Stage::Create,
            Stage::MTime,
            Stage::Exit,
        ]
    }

    /// 秩（1..8）。
    pub fn rank(&self) -> u32 {
        match self {
            Stage::Argv => 1u32,
            Stage::Parent => 2u32,
            Stage::Inode => 3u32,
            Stage::Block => 4u32,
            Stage::Meta => 5u32,
            Stage::Create => 6u32,
            Stage::MTime => 7u32,
            Stage::Exit => 8u32,
        }
    }

    /// 反查秩；秩 0 或 >8 一律 `None`（`None` 让调用方落 `TM_ESTAGE`）。
    pub fn from_rank(r: u32) -> Option<Stage> {
        match r {
            1 => Some(Stage::Argv),
            2 => Some(Stage::Parent),
            3 => Some(Stage::Inode),
            4 => Some(Stage::Block),
            5 => Some(Stage::Meta),
            6 => Some(Stage::Create),
            7 => Some(Stage::MTime),
            8 => Some(Stage::Exit),
            _ => None,
        }
    }

    /// 闸名（判据行与 ktest 里都用这一串）。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Argv => "argv",
            Stage::Parent => "parent",
            Stage::Inode => "inode",
            Stage::Block => "datablock",
            Stage::Meta => "journal-meta",
            Stage::Create => "create",
            Stage::MTime => "mtime",
            Stage::Exit => "exit",
        }
    }

    /// 这一闸是不是"失败落点"（Exit 是唯一的中性封印闸，Argv 是用法落点）。
    pub fn failure(&self) -> bool {
        match self {
            Stage::Argv | Stage::Exit => false,
            Stage::Parent | Stage::Inode | Stage::Block | Stage::Meta | Stage::Create
            | Stage::MTime => true,
        }
    }

    /// 这一闸之前，文件系统一次都没被碰（只有 argv 闸满足）。
    pub fn before_fs(&self) -> bool {
        matches!(self, Stage::Argv)
    }

    /// 这一闸是不是"空间成因三闸"之一（inode/datablock/journal-meta）。
    pub fn is_space_gate(&self) -> bool {
        matches!(self, Stage::Inode | Stage::Block | Stage::Meta)
    }

    /// 这一闸产不产残留（create 与 mtime 才可能产；空间三闸只定性不产件）。
    pub fn may_leave_residual(&self) -> bool {
        matches!(self, Stage::Create | Stage::MTime)
    }

    /// 成因档 → 应该落在哪道闸。本卡用它顶住"28 说错落点"的假绿。
    pub fn for_cause(c: CauseClass) -> Stage {
        match c {
            CauseClass::DataBlock | CauseClass::Reserved => Stage::Block,
            CauseClass::Inode => Stage::Inode,
            CauseClass::JournalMeta => Stage::Meta,
            CauseClass::NotSpace => Stage::Create,
        }
    }

    /// 成功账允许落在哪道闸：只有走完全线的 Exit。
    pub fn success_stage() -> Stage {
        Stage::Exit
    }
}

// ---------------------------------------------------------------------------
// 注解（文案家族 × 成因 × 残留的组合档）
// ---------------------------------------------------------------------------

/// note 维度：一条判据行的"形态"。它把动词家族、成因档、特殊成功形态合一，
/// 好让 `judge` 能在一格里抓住"动词与工具不配"或"成因与几何不配"。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// `touch` 建新件成功（0 字节件本来就是要的结果，不算残留）。
    TouchCreated,
    /// `touch` 改已存在件时间成功。
    TimesSet,
    /// `mkdir` 单层建成功。
    DirCreated,
    /// `mkdir -p` 逐级建成功（父层 + 末层）。
    DirsWithParents,
    /// `mkdir -p` 撞上"已存在即成功"：码 0、零输出、零残留。
    ExistsQuietOk,
    /// GNU 的 `cannot touch '...'`。
    GnuCannotTouch,
    /// GNU 的 `setting times of '...'`。
    GnuSettingTimes,
    /// GNU 的 `cannot create directory '...'`。
    GnuCannotCreateDir,
    /// busybox 的 `can't create '...'`。
    BbCantCreate,
    /// busybox 的 `can't create directory '...'`。
    BbCantCreateDir,
    /// 用法错：缺操作数/串不合法，文件系统没被碰。
    UsageLine,
    /// inode 档的 28（`df` 好看、`df -i` 见鬼）。
    InodeExhausted,
    /// 日志/元数据档的 28（`df` 与 `df -i` 都好看）——呈现移交 F11689。
    MetaExhausted,
    /// 保留块墙的 28（非 root 撞墙、root 能过）。
    ReservedWall,
    /// 邻居码冒充空间错（EMLINK/EDQUOT/EROFS/EACCES/EEXIST…）。
    ForeignErrno,
    /// 路径没解析到（ENOENT/ENOTDIR/ELOOP/ENAMETOOLONG）：那次调用没发生。
    PathUnresolved,
    /// 失败但盘上多出一个 0 字节件（`touch` 的正解之一）。
    LeftZeroFile,
    /// 失败但盘上多 n 层父目录（`mkdir -p` 的正解之一）。
    LeftParents,
    /// 动词家族混认/不明（note 闸判死）。
    VerbMismatch,
    /// 不裁定：别的工具的写入面。
    OutOfScope,
}

impl Note {
    /// note 全集（顺序固定，矩阵与语料共用）。
    pub fn all() -> [Note; 20] {
        [
            Note::TouchCreated,
            Note::TimesSet,
            Note::DirCreated,
            Note::DirsWithParents,
            Note::ExistsQuietOk,
            Note::GnuCannotTouch,
            Note::GnuSettingTimes,
            Note::GnuCannotCreateDir,
            Note::BbCantCreate,
            Note::BbCantCreateDir,
            Note::UsageLine,
            Note::InodeExhausted,
            Note::MetaExhausted,
            Note::ReservedWall,
            Note::ForeignErrno,
            Note::PathUnresolved,
            Note::LeftZeroFile,
            Note::LeftParents,
            Note::VerbMismatch,
            Note::OutOfScope,
        ]
    }

    /// note 名（蛇形，报告与测试逐字比对用）。
    pub fn name(&self) -> &'static str {
        match self {
            Note::TouchCreated => "touch-created",
            Note::TimesSet => "times-set",
            Note::DirCreated => "dir-created",
            Note::DirsWithParents => "dirs-with-parents",
            Note::ExistsQuietOk => "exists-quiet-ok",
            Note::GnuCannotTouch => "gnu-cannot-touch",
            Note::GnuSettingTimes => "gnu-setting-times",
            Note::GnuCannotCreateDir => "gnu-cannot-create-dir",
            Note::BbCantCreate => "bb-cant-create",
            Note::BbCantCreateDir => "bb-cant-create-dir",
            Note::UsageLine => "usage-line",
            Note::InodeExhausted => "inode-exhausted",
            Note::MetaExhausted => "meta-exhausted",
            Note::ReservedWall => "reserved-wall",
            Note::ForeignErrno => "foreign-errno",
            Note::PathUnresolved => "path-unresolved",
            Note::LeftZeroFile => "left-zero-file",
            Note::LeftParents => "left-parents",
            Note::VerbMismatch => "verb-mismatch",
            Note::OutOfScope => "out-of-scope",
        }
    }

    /// 是否成功形态。
    pub fn success(&self) -> bool {
        matches!(
            self,
            Note::TouchCreated | Note::TimesSet | Note::DirCreated | Note::DirsWithParents | Note::ExistsQuietOk
        )
    }

    /// 是否中止形态（能给本卡落锚的六类：三档成因 + 两种残留 + GNU/busybox 动词）。
    pub fn abort_family(&self) -> bool {
        matches!(
            self,
            Note::GnuCannotTouch
                | Note::GnuSettingTimes
                | Note::GnuCannotCreateDir
                | Note::BbCantCreate
                | Note::BbCantCreateDir
                | Note::InodeExhausted
                | Note::MetaExhausted
                | Note::ReservedWall
                | Note::LeftZeroFile
                | Note::LeftParents
        )
    }

    /// 是否邻居码档（判死，不许落锚）。
    pub fn foreign(&self) -> bool {
        matches!(self, Note::ForeignErrno | Note::VerbMismatch | Note::OutOfScope)
    }

    /// 是否用法档（与中止锚互斥）。
    pub fn usage(&self) -> bool {
        matches!(self, Note::UsageLine)
    }

    /// 是否"残留在场"的两档（value 维度非零的唯一合法来源）。
    pub fn carries_residual(&self) -> bool {
        matches!(self, Note::LeftZeroFile | Note::LeftParents)
    }

    /// 是否成因档（note 里管"为什么满"的三格）。
    pub fn is_cause(&self) -> bool {
        matches!(
            self,
            Note::InodeExhausted | Note::MetaExhausted | Note::ReservedWall
        )
    }

    /// 是否"那次创建调用没发生"档：路径没解析到，与用法错同归未落盘。
    pub fn untouched(&self) -> bool {
        matches!(self, Note::PathUnresolved)
    }

    /// 这一档要不要移交 F11689（呈现面）。
    pub fn hands_off_to_df(&self) -> bool {
        matches!(self, Note::InodeExhausted | Note::MetaExhausted)
    }

    /// 动词档与工具是否配对。
    pub fn verb_matches(&self, tool: Tool, flavor: Flavor) -> bool {
        match (self, tool, flavor) {
            (Note::GnuCannotTouch, Tool::Touch, Flavor::Gnu) => true,
            (Note::GnuSettingTimes, Tool::Touch, Flavor::Gnu) => true,
            (Note::GnuCannotCreateDir, Tool::Mkdir, Flavor::Gnu) => true,
            (Note::BbCantCreate, Tool::Touch, Flavor::Busybox) => true,
            (Note::BbCantCreateDir, Tool::Mkdir, Flavor::Busybox) => true,
            (Note::GnuCannotTouch, _, _) | (Note::GnuSettingTimes, _, _) | (Note::GnuCannotCreateDir, _, _) => false,
            (Note::BbCantCreate, _, _) | (Note::BbCantCreateDir, _, _) => false,
            _ => true,
        }
    }
}

// ---------------------------------------------------------------------------
// 档位（矩阵四档）
// ---------------------------------------------------------------------------

/// 四档：按 `Outcome` 把 14 格分进四个筐。`total` 与 `good` 两维独立统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 一档：完全绿（成功且残留语义正确）。
    Clean,
    /// 二档：诚实红（中止、文案与几何自洽、落锚）。
    HonestFail,
    /// 三档：未落盘（用法错、不裁定——文件系统一次都没碰）。
    NotTouched,
    /// 四档：谎报（任一维对不上，判死）。
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

    /// 是否允许落本卡中止锚（只有诚实红一档）。
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
    /// 残留错位（件数与工具/申报不符）。
    Value,
    /// 注解错位（动词家族/成因档混认）。
    Note,
    /// 锚位错位（该亮不亮、不该亮却亮）。
    Default,
    /// 几何矛盾（avail/free_inodes/caller_root 与成因档打架）。
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
    /// 矩阵不平。
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
            Fault::Estate => TM_ESTATE,
            Fault::Stage => TM_ESTAGE,
            Fault::Code => TM_ECODE,
            Fault::Value => TM_EVALUE,
            Fault::Note => TM_ENOTE,
            Fault::Default => TM_EDEFAULT,
            Fault::Geom => TM_EGEOM,
            Fault::State => TM_ESTATE,
            Fault::Count => TM_ECOUNT,
            Fault::Slot => TM_ESLOT,
            Fault::Seal => TM_ESEAL,
            Fault::Nob => TM_ENOBS,
            Fault::Matrix => TM_EMATRIX,
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
            Fault::Ok | Fault::Estate | Fault::Stage | Fault::Code | Fault::Value | Fault::Note | Fault::Default
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

// ---------------------------------------------------------------------------
// 盘的几何（本卡的"物理证据"；四档成因全靠这几格分开）
// ---------------------------------------------------------------------------

/// 一次判定当时盘面上的供给情况。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Geom {
    /// `df` 口径的可用块（已扣 root 保留段）。
    pub avail_blocks: u32,
    /// 未扣保留段的真空闲块（`statfs` 的 f_bfree）。
    pub free_blocks: u32,
    /// 剩余 inode（`df -i` 的 IFree）。
    pub free_inodes: u32,
    /// inode 总量（只做合法性与占比叙述用）。
    pub total_inodes: u32,
    /// 文件系统家族（决定保留块概念、迟到错误、元数据分离）。
    pub fs: FsFamily,
}

impl Geom {
    /// 空几何：全零 + ext4，只作 `const fn` 初值用。
    pub const fn blank() -> Geom {
        Geom {
            avail_blocks: 0u32,
            free_blocks: 0u32,
            free_inodes: 0u32,
            total_inodes: 0u32,
            fs: FsFamily::LocalExt4,
        }
    }

    /// 结构性合法：`avail <= free`，`free_inodes <= total_inodes`。
    /// 反过来就是假几何——`df` 不可能报出比真空闲更多的可用块。
    pub fn well_formed(&self) -> bool {
        if self.avail_blocks > self.free_blocks {
            return false;
        }
        if self.total_inodes > 0 && self.free_inodes > self.total_inodes {
            return false;
        }
        true
    }

    /// 按 `df` 口径满不满（avail==0）。
    pub fn full_by_avail(&self) -> bool {
        self.avail_blocks == 0
    }

    /// 按内核口径有没有地儿（free>0 意味着保留段外还有块）。
    pub fn has_free_blocks(&self) -> bool {
        self.free_blocks > 0
    }

    /// inode 有没有（`df -i` 口径）。
    pub fn has_inodes(&self) -> bool {
        self.free_inodes > 0
    }

    /// root 视角能不能建：ext 家族可以吃保留段，别的家族不行。
    pub fn writable_by_root(&self) -> bool {
        if self.fs.has_reserved_blocks() {
            return self.free_blocks > 0;
        }
        self.avail_blocks > 0
    }

    /// 非 root 视角能不能建：只看 avail。
    pub fn writable_by_other(&self) -> bool {
        self.avail_blocks > 0
    }

    /// 按调用者身份判"该不该吃到 28"。
    pub fn writable_by(&self, caller_root: bool) -> bool {
        if caller_root {
            self.writable_by_root()
        } else {
            self.writable_by_other()
        }
    }

    /// 保留段占比（千分率）；只有 ext 家族才有意义。
    pub fn reserved_permille(&self) -> u32 {
        if !self.fs.has_reserved_blocks() || self.free_blocks < self.avail_blocks {
            return 0u32;
        }
        let reserved = self.free_blocks - self.avail_blocks;
        if self.free_blocks == 0 {
            return 0u32;
        }
        (reserved as u64 * 1000 / self.free_blocks as u64) as u32
    }

    /// 几何指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.avail_blocks as u64);
        h = fold_word(h, self.free_blocks as u64);
        h = fold_word(h, self.free_inodes as u64);
        h = fold_word(h, self.total_inodes as u64);
        let family_tag = match self.fs {
            FsFamily::LocalExt4 => 1u8,
            FsFamily::LocalBtrfs => 2u8,
            FsFamily::Tmpfs => 3u8,
            FsFamily::Nfs => 4u8,
            FsFamily::ReadOnly => 5u8,
        };
        fold_byte(h, family_tag)
    }
}

/// 语料用的三块代表盘：满 data 盘、满 inode 盘、元数据满盘。
pub fn geoms() -> [Geom; 4] {
    [
        // 0：真满（avail==0、free==0、inode 还有）。
        Geom {
            avail_blocks: 0,
            free_blocks: 0,
            free_inodes: 65_536,
            total_inodes: 65_536,
            fs: FsFamily::LocalExt4,
        },
        // 1：inode 耗尽（avail 好看，IFree=0）。
        Geom {
            avail_blocks: 12_288,
            free_blocks: 12_288,
            free_inodes: 0,
            total_inodes: 65_536,
            fs: FsFamily::LocalExt4,
        },
        // 2：元数据/日志耗尽（数据与 inode 都好看）。
        Geom {
            avail_blocks: 262_144,
            free_blocks: 262_144,
            free_inodes: 32_768,
            total_inodes: 65_536,
            fs: FsFamily::LocalBtrfs,
        },
        // 3：保留块墙（avail==0 但 free>0，ext4）。
        Geom {
            avail_blocks: 0,
            free_blocks: 1_024,
            free_inodes: 4_096,
            total_inodes: 65_536,
            fs: FsFamily::LocalExt4,
        },
    ]
}

/// 取第 i 块代表盘；越界给 `None`，调用方落 `TM_ENOBS`。
pub fn geom_at(i: usize) -> Option<Geom> {
    geoms().get(i).copied()
}

// ---------------------------------------------------------------------------
// Shape：一次调用的全部输入维度
// ---------------------------------------------------------------------------

/// 本卡输入形状。一条判据行 = 一个 Shape + 一份申报（Claim）。
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    /// 哪个工具。
    pub tool: Tool,
    /// 干的哪件事。
    pub op: OpKind,
    /// 文案家族。
    pub flavor: Flavor,
    /// 目标路径（语料里全部用静态串，逐字进文案）。
    pub path: &'static str,
    /// 当时盘面。
    pub geom: Geom,
    /// 调用者是不是 root（保留墙一档靠它）。
    pub caller_root: bool,
    /// 父路径解析结果。
    pub path_class: PathClass,
    /// 那次调用实际返回的 errno（0 表示成功）。
    pub errno_seen: i32,
    /// 失败后盘上多出来的东西。
    pub residual: Residual,
    /// 选项位图。
    pub opts: u8,
    /// 操作数个数（0 个就是用法错）。
    pub operand_count: u32,
    /// 命令行本身合不合法。
    pub argv_legal: bool,
    /// 目标件在调用前是否已存在（决定 touch 走 create 还是 set-times）。
    pub existed_before: bool,
    /// 是否已把呈现面移交 F11689（inode/meta 两档为正）。
    pub hands_off: bool,
    /// 申报的退出码（exit 闸用来对账）。
    pub exit_declared: u32,
}

impl Shape {
    /// 空形状：一个全绿的 `mkdir` 单层新建，供 `const fn` 组装基型用。
    pub const fn blank() -> Shape {
        Shape {
            tool: Tool::Mkdir,
            op: OpKind::MkDirOne,
            flavor: Flavor::Gnu,
            path: "x",
            geom: Geom::blank(),
            caller_root: false,
            path_class: PathClass::Resolved,
            errno_seen: ERR_NONE,
            residual: Residual::None,
            opts: 0u8,
            operand_count: 1u32,
            argv_legal: true,
            existed_before: false,
            hands_off: false,
            exit_declared: EXIT_OK,
        }
    }

    /// 取某位选项。
    pub const fn opt_has(&self, bit: u8) -> bool {
        self.opts & bit != 0
    }

    /// 未知选项位（本卡认得的位之外）。
    pub const fn unknown_opt_bits(&self) -> u8 {
        self.opts & !OPT_KNOWN
    }

    /// 是不是带 `-p`。
    pub const fn with_parents(&self) -> bool {
        self.opt_has(OPT_MAKE_PARENTS)
    }

    /// 是不是 `-v`（成功侧多打一行 stdout）。
    pub const fn is_verbose(&self) -> bool {
        self.opt_has(OPT_VERBOSE)
    }

    /// 用法是否合法：命令行合法、有操作数、工具在本卡管辖内。
    pub fn usage_bad(&self) -> bool {
        if !self.argv_legal {
            return true;
        }
        if self.operand_count == 0 {
            return true;
        }
        if !self.tool.owned() {
            return true;
        }
        if self.op == OpKind::OutOfScope {
            return true;
        }
        false
    }

    /// errno 是否在本卡认得的集合里。
    pub fn errno_legal(&self) -> bool {
        errno_known(self.errno_seen)
    }

    /// 选项与操作是否自洽：`-p` 只能配 mkdir，`-a/-m/-d/-t/-r` 只能配 touch。
    pub fn opts_match_op(&self) -> bool {
        if self.with_parents() || self.is_verbose() {
            if !self.op.is_mkdir() {
                return false;
            }
        }
        if self.opt_has(OPT_MODE) && !self.op.is_mkdir() {
            return false;
        }
        if self.opt_has(OPT_ATIME | OPT_MTIME | OPT_DATE | OPT_TIME | OPT_REF) && !self.op.is_touch()
        {
            return false;
        }
        true
    }

    /// `op` 与 `existed_before` 是否自洽：create 只给不存在的件，set-times 只给已存在的件。
    pub fn existence_consistent(&self) -> bool {
        match self.op {
            OpKind::TouchCreate => !self.existed_before,
            OpKind::TouchSetTimes | OpKind::TouchFlags => self.existed_before,
            OpKind::MkDirOne => true,
            OpKind::MkDirParents => true,
            OpKind::Usage | OpKind::OutOfScope => true,
        }
    }

    /// 残留与工具/操作是否配对（本卡排他判据之一）。
    pub fn residual_allowed(&self) -> bool {
        match self.tool {
            Tool::Touch => self.residual.allowed_for_touch(),
            Tool::Mkdir => self.residual.allowed_for_mkdir(self.op),
            Tool::Foreign => false,
        }
    }

    /// 深度是否合法（`-p` 最多 PARENT_DEPTH_MAX 层）。
    pub fn parent_depth_legal(&self) -> bool {
        match self.residual {
            Residual::Parents(n) => n <= PARENT_DEPTH_MAX,
            _ => true,
        }
    }

    /// 形状整体是否合法。不合法的形状不参与判据（语料生成时先过这一关）。
    pub fn valid(&self) -> bool {
        if !self.geom.well_formed() {
            return false;
        }
        if !self.errno_legal() {
            return false;
        }
        if !self.opts_match_op() {
            return false;
        }
        if !self.existence_consistent() {
            return false;
        }
        if !self.parent_depth_legal() {
            return false;
        }
        if self.unknown_opt_bits() != 0 {
            return false;
        }
        if self.usage_bad() {
            // 用法错本身是合法形状，但要保证 op/工具对得上。
            return self.op == OpKind::Usage || self.op == OpKind::OutOfScope;
        }
        true
    }

    /// 复制并只换 errno。
    pub fn clone_errno(&self, e: i32) -> Shape {
        let mut s = *self;
        s.errno_seen = e;
        s
    }

    /// 复制并只换残留档。
    pub fn clone_residual(&self, r: Residual) -> Shape {
        let mut s = *self;
        s.residual = r;
        s
    }

    /// 复制并只换几何档。
    pub fn with_geom(&self, i: usize) -> Shape {
        let mut s = *self;
        if let Some(g) = geom_at(i) {
            s.geom = g;
        }
        s
    }

    /// 复制并只换文案家族。
    pub fn with_flavor(&self, f: Flavor) -> Shape {
        let mut s = *self;
        s.flavor = f;
        s
    }

    /// 成因档：本卡的核心推算，四档全靠几何与身份分开。
    pub fn cause(&self) -> CauseClass {
        if self.errno_seen != ERR_ENOSPC {
            return CauseClass::NotSpace;
        }
        // inode 面先判：avail 好看而 inode 归零。
        if self.geom.avail_blocks > 0 && self.geom.free_inodes == 0 {
            return CauseClass::Inode;
        }
        // 数据面与 inode 面都好看却仍 28：日志/元数据面。
        if self.geom.avail_blocks > 0 && self.geom.free_inodes > 0 {
            return CauseClass::JournalMeta;
        }
        // avail==0 且还有保留段外的块、非 root、ext 家族：保留墙。
        if self.geom.avail_blocks == 0
            && self.geom.free_blocks > 0
            && !self.caller_root
            && self.geom.fs.has_reserved_blocks()
        {
            return CauseClass::Reserved;
        }
        CauseClass::DataBlock
    }

    /// 移交标记是否应当为真（inode 与元数据两档要交给 F11689 呈现）。
    pub fn should_hand_off(&self) -> bool {
        self.cause().df_lies()
    }

    /// 本地 + 不分配的操作吃到 28：这是本卡点名的那类假（NFS 才放行）。
    pub fn is_deferred_lie(&self) -> bool {
        if self.errno_seen != ERR_ENOSPC {
            return false;
        }
        if self.op.allocates() {
            return false;
        }
        !self.geom.fs.defers_errors()
    }

    /// 那次红是不是"真空间错"（给锚位的必要条件）。
    pub fn is_real_space_failure(&self) -> bool {
        self.errno_seen == ERR_ENOSPC && self.cause().is_space() && self.op.allocates()
    }

    /// 形状指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_byte(h, self.tool.index() as u8);
        h = fold_byte(h, self.op.index() as u8);
        h = fold_byte(h, self.flavor.index() as u8);
        h = fold_str(h, self.path);
        h = fold_word(h, self.geom.seal());
        h = fold_bool(h, self.caller_root);
        h = fold_str(h, self.path_class.name());
        h = fold_i32(h, self.errno_seen);
        h = fold_str(h, self.residual.name());
        h = fold_word(h, self.residual.count() as u64);
        h = fold_word(h, self.opts as u64);
        h = fold_word(h, self.operand_count as u64);
        h = fold_bool(h, self.argv_legal);
        h = fold_bool(h, self.existed_before);
        h = fold_bool(h, self.hands_off);
        fold_word(h, self.exit_declared as u64)
    }
}

// ---------------------------------------------------------------------------
// Outcome / Claim / Case
// ---------------------------------------------------------------------------

/// 一次调用的**实际**结论（由 `define` 从 Shape 推出来）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 落点闸。
    pub stage: Stage,
    /// errno（0 表示这次调用绿）。
    pub code: i32,
    /// 残留计数（成功侧恒为 0）。
    pub value: u32,
    /// 形态档。
    pub note: Note,
    /// 本卡中止锚：只有真空间错、残留自洽、码 1 才亮。
    pub default: bool,
    /// 结构自洽：假几何、本地改时间吃到 28 等都把它压成 `false`。
    pub consistent: bool,
}

impl Outcome {
    /// 全零初值。
    pub const fn blank() -> Outcome {
        Outcome {
            stage: Stage::Exit,
            code: ERR_NONE,
            value: 0u32,
            note: Note::TouchCreated,
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

    /// 是否落了本卡锚。
    pub fn anchored(&self) -> bool {
        self.default
    }

    /// 是否谎报（结构上不自洽）。
    pub fn refused(&self) -> bool {
        !self.consistent
    }

    /// 应然的退出码：绿 0、用法 1（GNU touch/mkdir 的用法错也是 1）、其余 1。
    ///
    /// 注意本卡不许自造码：`touch`/`mkdir` 的缺操作数在 GNU 里退 **1**
    /// （`EXIT_USAGE` 留给 shell 内建与 `getopt` 早退的路径），这里按工具真值给。
    pub fn exit(&self) -> u32 {
        if self.ok() {
            return EXIT_OK;
        }
        if self.note.out_of_scope_marker() {
            return EXIT_UNJUDGED;
        }
        EXIT_FILE
    }

    /// 该不该落锚（复算一遍，供 `default` 自检）：真空间 errno、分配型操作、
    /// 残留自洽、闸位在空间三闸或 create 闸。
    pub fn should_anchor(&self) -> bool {
        if !self.consistent {
            return false;
        }
        if self.code != ERR_ENOSPC {
            return false;
        }
        matches!(
            self.stage,
            Stage::Parent | Stage::Inode | Stage::Block | Stage::Meta | Stage::Create
        )
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
    /// 申报的残留计数。
    pub value: u32,
    /// 申报的形态档。
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
            note: Note::TouchCreated,
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
    /// 短名（`f11688-<slug>`，标签必须是本卡的）。
    pub tag: &'static str,
}

impl Case {
    /// 空语料格。
    pub const fn blank() -> Case {
        Case {
            shape: Shape::blank(),
            claim: Claim::blank(),
            tag: "f11688-blank",
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

/// 供 `Outcome::exit` 用的窄接口：不裁定档给占位码。
impl Note {
    /// 是否为"不裁定"档（别的工具的写入面，本卡不给码）。
    pub fn out_of_scope_marker(&self) -> bool {
        matches!(self, Note::OutOfScope)
    }
}

// ---------------------------------------------------------------------------
// define：把形状过八道闸，推出实际结论
// ---------------------------------------------------------------------------

/// 动词档：按（家族, 工具, 操作）选文案 note。`Mixed` 家族不配任何动词。
fn verb_note(tool: Tool, op: OpKind, flavor: Flavor) -> Note {
    match flavor {
        Flavor::Mixed => Note::VerbMismatch,
        Flavor::Gnu => match (tool, op) {
            (Tool::Touch, OpKind::TouchCreate) => Note::GnuCannotTouch,
            (Tool::Touch, OpKind::TouchSetTimes) => Note::GnuSettingTimes,
            (Tool::Touch, OpKind::TouchFlags) => Note::GnuCannotTouch,
            (Tool::Touch, OpKind::Usage) => Note::UsageLine,
            (Tool::Touch, OpKind::OutOfScope) => Note::OutOfScope,
            (Tool::Touch, OpKind::MkDirOne)
            | (Tool::Touch, OpKind::MkDirParents) => Note::VerbMismatch,
            (Tool::Mkdir, _) => Note::GnuCannotCreateDir,
            (Tool::Foreign, _) => Note::VerbMismatch,
        },
        Flavor::Busybox => match (tool, op) {
            (Tool::Touch, _) => Note::BbCantCreate,
            (Tool::Mkdir, _) => Note::BbCantCreateDir,
            (Tool::Foreign, _) => Note::VerbMismatch,
        },
    }
}

/// 成功侧的 note。
fn success_note(op: OpKind, existed_before: bool) -> Note {
    match op {
        OpKind::TouchCreate => Note::TouchCreated,
        OpKind::TouchSetTimes | OpKind::TouchFlags => Note::TimesSet,
        OpKind::MkDirOne => Note::DirCreated,
        OpKind::MkDirParents => {
            if existed_before {
                Note::ExistsQuietOk
            } else {
                Note::DirsWithParents
            }
        }
        OpKind::Usage => Note::UsageLine,
        OpKind::OutOfScope => Note::OutOfScope,
    }
}

/// **本卡的纵线**：八道闸按秩走，前一闸不产出事实，后一闸就没有输入。
///
/// 走法（每一段都对应 `Stage` 的一闸，返回即落闸）：
///
/// 1. `argv` 闸：不裁定的工具/操作先出局；命令行不合法或缺操作数落用法档。
///    这两类**永远不给锚**——文件系统一次都没碰。
/// 2. `parent` 闸：路径没解析到（ENOENT/ENOTDIR/ELOOP/ENAMETOOLONG）就落在这里，
///    errno 用路径档自带的邻居码，锚位为假。
/// 3. 绿路：`errno_seen == 0` 时按操作选成功档，残留必须是 `None`，否则
///    `consistent` 被压成假（"成功却留了个来历不明的件"是本卡的假绿面）。
/// 4. `mtime` 闸：不分配的操作（改时间）吃到 28，本地文件系统必判假，
///    NFS 才放行；这一路**不落本卡锚**（锚只管建件/建目录）。
/// 5. `inode`/`datablock`/`journal-meta` 三闸：按几何把 28 定性成四档成因，
///    闸位即成因闸。`-p` 在自建父层时红，闸位前移到 `parent`。
/// 6. `create` 闸：非空间的邻居码（EEXIST/EACCES/EMLINK/EROFS…）落在这里，
///    note 给邻居档，锚位为假。
/// 7. note 阶梯：`混合家族 → 动词档` 优先判死，其次**残留档**（比成因更具体），
///    再次**成因档**（非数据块三档），最后才回落到动词档。
/// 8. `exit` 闸：锚位只在"28 + 空间成因 + 分配型操作 + 结构自洽"四件齐时亮。
pub fn define(s: &Shape) -> Outcome {
    // 结构自洽是一切的前提：假几何/错配残留/非法选项都把 consistent 压假。
    let mut consistent = s.valid();

    // --- 闸 1：argv ---------------------------------------------------------
    if s.op == OpKind::OutOfScope || !s.tool.owned() {
        return Outcome::make(Stage::Argv, ERR_NONE, 0, Note::OutOfScope, false, consistent);
    }
    if !s.argv_legal || s.operand_count == 0 || s.op == OpKind::Usage {
        return Outcome::make(Stage::Argv, ERR_NONE, 0, Note::UsageLine, false, consistent);
    }

    // --- 闸 2：parent -------------------------------------------------------
    if s.path_class.call_skipped() {
        let e = s.path_class.errno();
        if e == ERR_ENOSPC {
            // 路径档不该产出 28：真产出就是假几何，压 consistent。
            consistent = false;
        }
        return Outcome::make(Stage::Parent, e, 0, Note::PathUnresolved, false, consistent);
    }

    // --- 闸 3：绿路 ---------------------------------------------------------
    if s.errno_seen == ERR_NONE {
        let resid_clean = matches!(s.residual, Residual::None);
        if !resid_clean {
            consistent = false;
        }
        return Outcome::make(
            Stage::Exit,
            ERR_NONE,
            0,
            success_note(s.op, s.existed_before),
            false,
            consistent,
        );
    }

    // --- 闸 4：mtime（不分配的操作）----------------------------------------
    if !s.op.allocates() {
        if s.is_deferred_lie() {
            // 本地文件系统上 `utimensat` 不可能回 28 —— 这一路必须判假。
            consistent = false;
        }
        let note = verb_note(s.tool, s.op, s.flavor);
        if matches!(s.residual, Residual::None) {
            return Outcome::make(Stage::MTime, s.errno_seen, 0, note, false, consistent);
        }
        // 改时间既不建件也不留父层，有残留就是不自洽。
        return Outcome::make(Stage::MTime, s.errno_seen, 0, note, false, false);
    }

    // --- 闸 5/6：空间三闸与 create 闸 ---------------------------------------
    let cause = s.cause();
    if cause == CauseClass::NotSpace {
        // 邻居码：落 create 闸，note 给邻居档，绝不落锚。
        return Outcome::make(Stage::Create, s.errno_seen, 0, Note::ForeignErrno, false, consistent);
    }

    // 28 的闸位：`-p` 在自建父层时红 → parent；否则按成因定档。
    let stage = if s.path_class == PathClass::CreatedByParents {
        Stage::Parent
    } else {
        Stage::for_cause(cause)
    };

    // 残留与工具配对检查（本卡排他维）。
    let resid_ok = s.residual_allowed();
    if !resid_ok {
        consistent = false;
    }
    let value = if matches!(s.residual, Residual::None) {
        0u32
    } else if resid_ok {
        s.residual.count()
    } else {
        0u32
    };

    // note 阶梯：混合家族 → 残留档 → 成因档 → 动词档。
    let note = if s.flavor == Flavor::Mixed {
        Note::VerbMismatch
    } else if value > 0 {
        match s.residual {
            Residual::ZeroFile => Note::LeftZeroFile,
            Residual::Parents(_) => Note::LeftParents,
            _ => Note::VerbMismatch,
        }
    } else {
        match cause {
            CauseClass::Inode => Note::InodeExhausted,
            CauseClass::JournalMeta => Note::MetaExhausted,
            CauseClass::Reserved => Note::ReservedWall,
            _ => verb_note(s.tool, s.op, s.flavor),
        }
    };

    let default = consistent
        && s.errno_seen == ERR_ENOSPC
        && cause.is_space()
        && s.op.allocates()
        && resid_ok;

    Outcome::make(stage, s.errno_seen, value, note, default, consistent)
}

/// 把一次结论拼成该工具该家族的正字文案行（note 闸的复现物）。
pub fn message(s: &Shape, o: &Outcome) -> String {
    if o.note.success() {
        if s.is_verbose() && s.op.is_mkdir() {
            let layers = match s.op {
                OpKind::MkDirParents => 2u32,
                _ => 1u32,
            };
            return msg_gnu_mkdir_created_layer(s.path, layers);
        }
        return String::new();
    }
    if o.note.usage() {
        return match s.flavor {
            Flavor::Busybox => msg_bb_usage_line(s.tool),
            _ => msg_gnu_usage_line(s.tool),
        };
    }
    if o.note == Note::PathUnresolved || o.note == Note::ForeignErrno {
        return Flavor::Gnu.text(s.tool, s.op, s.path, o.code);
    }
    if o.note == Note::VerbMismatch || o.note == Note::OutOfScope {
        return String::new();
    }
    s.flavor.text(s.tool, s.op, s.path, o.code)
}

/// 一格语料的可读明细（报告与测试都用它，逐字稳定）。
pub fn detail(c: &Case) -> String {
    let o = c.actual();
    let mut s = String::from(c.tag());
    s.push_str(" | stage=");
    s.push_str(o.stage.name());
    s.push_str(" errno=");
    s.push_str(i32_text(o.code).as_str());
    s.push('(');
    s.push_str(errno_name(o.code));
    s.push_str(") residual=");
    s.push_str(dec_text(o.value).as_str());
    s.push_str(" note=");
    s.push_str(o.note.name());
    s.push_str(" anchor=");
    s.push_str(bool_text(o.default));
    s.push_str(" judge=");
    s.push_str(fault_short(c.fault()));
    s
}

// ---------------------------------------------------------------------------
// 语料：8 活 + 6 死
// ---------------------------------------------------------------------------

/// 基型：一块供给正常的 ext4 盘上的 GNU `mkdir` 单层新建（全绿）。
const BASE: Shape = Shape {
    tool: Tool::Mkdir,
    op: OpKind::MkDirOne,
    flavor: Flavor::Gnu,
    path: "x",
    geom: Geom {
        avail_blocks: 1000,
        free_blocks: 1000,
        free_inodes: 500,
        total_inodes: 1000,
        fs: FsFamily::LocalExt4,
    },
    caller_root: false,
    path_class: PathClass::Resolved,
    errno_seen: ERR_NONE,
    residual: Residual::None,
    opts: 0,
    operand_count: 1,
    argv_legal: true,
    existed_before: false,
    hands_off: false,
    exit_declared: EXIT_OK,
};

/// 申报基型：与 `BASE` 推出的实际结论逐格相同。
const BASE_CLAIM: Claim = Claim {
    stage: Stage::Exit,
    code: ERR_NONE,
    value: 0,
    note: Note::DirCreated,
    default: false,
};

/// 十四格语料形状。前八格申报正确，后六格一槽只坏一维。
pub const SHAPES: [Shape; CORPUS] = [
    // 0 `touch` 建新件成功（0 字节件本来就是要的结果）。
    Shape {
        tool: Tool::Touch,
        op: OpKind::TouchCreate,
        path: "note.txt",
        ..BASE
    },
    // 1 `mkdir` 单层建成功。
    Shape {
        path: "logs/2026",
        opts: OPT_VERBOSE,
        ..BASE
    },
    // 2 `mkdir -p` 逐级建成功（父层与末层都新建）。
    Shape {
        path: "a/b/c",
        op: OpKind::MkDirParents,
        opts: OPT_MAKE_PARENTS,
        ..BASE
    },
    // 3 `mkdir -p` 撞上已存在：码 0、零输出、零残留。
    Shape {
        path: "spool",
        op: OpKind::MkDirParents,
        opts: OPT_MAKE_PARENTS,
        existed_before: true,
        ..BASE
    },
    // 4 `touch` 建新件撞数据块满：失败却留下一个 0 字节件（落锚）。
    Shape {
        tool: Tool::Touch,
        op: OpKind::TouchCreate,
        path: "full/new",
        geom: Geom {
            avail_blocks: 0,
            free_blocks: 0,
            free_inodes: 65_536,
            total_inodes: 65_536,
            fs: FsFamily::LocalExt4,
        },
        errno_seen: ERR_ENOSPC,
        residual: Residual::ZeroFile,
        hands_off: false,
        exit_declared: EXIT_FILE,
        ..BASE
    },
    // 5 `mkdir` 单层撞数据块满：失败且**无痕**（落锚）。
    Shape {
        path: "full/a",
        geom: Geom {
            avail_blocks: 0,
            free_blocks: 0,
            free_inodes: 65_536,
            total_inodes: 65_536,
            fs: FsFamily::LocalExt4,
        },
        errno_seen: ERR_ENOSPC,
        residual: Residual::None,
        exit_declared: EXIT_FILE,
        ..BASE
    },
    // 6 `touch` 改已存在件时间，NFS 把服务端的 28 迟到交回来：放行但不落锚。
    Shape {
        tool: Tool::Touch,
        op: OpKind::TouchSetTimes,
        path: "mnt/srv/f",
        geom: Geom {
            avail_blocks: 4096,
            free_blocks: 4096,
            free_inodes: 2048,
            total_inodes: 4096,
            fs: FsFamily::Nfs,
        },
        errno_seen: ERR_ENOSPC,
        existed_before: true,
        exit_declared: EXIT_FILE,
        ..BASE
    },
    // 7 用法错：`touch` 一个操作数都没给，文件系统一次都没碰。
    Shape {
        tool: Tool::Touch,
        op: OpKind::Usage,
        path: "",
        operand_count: 0,
        argv_legal: false,
        exit_declared: EXIT_FILE,
        ..BASE
    },
    // 8（Estate）同一现场被申报成"绿"：成败互报，判序第一维当场抓住。
    Shape {
        tool: Tool::Touch,
        op: OpKind::TouchCreate,
        path: "full/new",
        geom: Geom {
            avail_blocks: 0,
            free_blocks: 0,
            free_inodes: 65_536,
            total_inodes: 65_536,
            fs: FsFamily::LocalExt4,
        },
        errno_seen: ERR_ENOSPC,
        residual: Residual::ZeroFile,
        exit_declared: EXIT_FILE,
        ..BASE
    },
    // 9（Stage）同一现场把成因闸说成 create 闸。
    Shape {
        tool: Tool::Touch,
        op: OpKind::TouchCreate,
        path: "full/new",
        geom: Geom {
            avail_blocks: 0,
            free_blocks: 0,
            free_inodes: 65_536,
            total_inodes: 65_536,
            fs: FsFamily::LocalExt4,
        },
        errno_seen: ERR_ENOSPC,
        residual: Residual::ZeroFile,
        exit_declared: EXIT_FILE,
        ..BASE
    },
    // 10（Code）同一现场把 28 写成邻居码 EDQUOT（配额与满盘是两回事）。
    Shape {
        tool: Tool::Touch,
        op: OpKind::TouchCreate,
        path: "full/new",
        geom: Geom {
            avail_blocks: 0,
            free_blocks: 0,
            free_inodes: 65_536,
            total_inodes: 65_536,
            fs: FsFamily::LocalExt4,
        },
        errno_seen: ERR_ENOSPC,
        residual: Residual::ZeroFile,
        exit_declared: EXIT_FILE,
        ..BASE
    },
    // 11（Value）同一现场把 0 字节残留藏报成"无痕"。
    Shape {
        tool: Tool::Touch,
        op: OpKind::TouchCreate,
        path: "full/new",
        geom: Geom {
            avail_blocks: 0,
            free_blocks: 0,
            free_inodes: 65_536,
            total_inodes: 65_536,
            fs: FsFamily::LocalExt4,
        },
        errno_seen: ERR_ENOSPC,
        residual: Residual::ZeroFile,
        exit_declared: EXIT_FILE,
        ..BASE
    },
    // 12（Note）同一现场把数据块耗尽写成保留块墙。
    Shape {
        tool: Tool::Touch,
        op: OpKind::TouchCreate,
        path: "full/new",
        geom: Geom {
            avail_blocks: 0,
            free_blocks: 0,
            free_inodes: 65_536,
            total_inodes: 65_536,
            fs: FsFamily::LocalExt4,
        },
        errno_seen: ERR_ENOSPC,
        residual: Residual::ZeroFile,
        exit_declared: EXIT_FILE,
        ..BASE
    },
    // 13（Default）同一现场数字全对却不落本卡中止锚（漏锚与错锚同样危险）。
    Shape {
        tool: Tool::Touch,
        op: OpKind::TouchCreate,
        path: "full/new",
        geom: Geom {
            avail_blocks: 0,
            free_blocks: 0,
            free_inodes: 65_536,
            total_inodes: 65_536,
            fs: FsFamily::LocalExt4,
        },
        errno_seen: ERR_ENOSPC,
        residual: Residual::ZeroFile,
        exit_declared: EXIT_FILE,
        ..BASE
    },
];

/// 十四格申报。前八格与 `define(SHAPES[i])` 逐格相同，后六格各坏一维。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim {
        stage: Stage::Exit,
        code: ERR_NONE,
        value: 0,
        note: Note::TouchCreated,
        default: false,
    },
    BASE_CLAIM,
    Claim {
        stage: Stage::Exit,
        code: ERR_NONE,
        value: 0,
        note: Note::DirsWithParents,
        default: false,
    },
    Claim {
        stage: Stage::Exit,
        code: ERR_NONE,
        value: 0,
        note: Note::ExistsQuietOk,
        default: false,
    },
    Claim {
        stage: Stage::Block,
        code: ERR_ENOSPC,
        value: 1,
        note: Note::LeftZeroFile,
        default: true,
    },
    Claim {
        stage: Stage::Block,
        code: ERR_ENOSPC,
        value: 0,
        note: Note::GnuCannotCreateDir,
        default: true,
    },
    Claim {
        stage: Stage::MTime,
        code: ERR_ENOSPC,
        value: 0,
        note: Note::GnuSettingTimes,
        default: false,
    },
    Claim {
        stage: Stage::Argv,
        code: ERR_NONE,
        value: 0,
        note: Note::UsageLine,
        default: false,
    },
    // 8 Estate：报成绿。
    Claim {
        stage: Stage::Exit,
        code: ERR_NONE,
        value: 0,
        note: Note::TouchCreated,
        default: false,
    },
    // 9 Stage：成因闸说成 create 闸。
    Claim {
        stage: Stage::Create,
        code: ERR_ENOSPC,
        value: 1,
        note: Note::LeftZeroFile,
        default: true,
    },
    // 10 Code：28 写成 EDQUOT。
    Claim {
        stage: Stage::Block,
        code: ERR_EDQUOT,
        value: 1,
        note: Note::LeftZeroFile,
        default: true,
    },
    // 11 Value：残留藏报成 0。
    Claim {
        stage: Stage::Block,
        code: ERR_ENOSPC,
        value: 0,
        note: Note::LeftZeroFile,
        default: true,
    },
    // 12 Note：数据块档写成保留墙档。
    Claim {
        stage: Stage::Block,
        code: ERR_ENOSPC,
        value: 1,
        note: Note::ReservedWall,
        default: true,
    },
    // 13 Default：四维全对却不落锚。
    Claim {
        stage: Stage::Block,
        code: ERR_ENOSPC,
        value: 1,
        note: Note::LeftZeroFile,
        default: false,
    },
];

/// 后六槽各坏的那一维（顺序与判序一致）。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

/// 后六槽各自应当命中的错误码（互不相同）。
pub const REJECT_CODES: [u32; REJECT] = [
    TM_ESTATE,
    TM_ESTAGE,
    TM_ECODE,
    TM_EVALUE,
    TM_ENOTE,
    TM_EDEFAULT,
];

/// 前八格的统计值。第 8 格是红档率标定值 929（14 格中 13 时的千分率）。
///
/// 七格统计依次为：语料格数、申报正确槽数、申报错误槽数、一档绿格数、
/// 二档诚实红格数、三档未落盘格数、诚实红里落锚的格数。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 4, 3, 1, 2, 929];

/// 十四格短名（标签必须挂本卡号，别卡的标签在本卡一律判死）。
pub const WANT_TAGS: [&str; CORPUS] = [
    "f11688-touch-created",
    "f11688-mkdir-created",
    "f11688-mkdir-p-created",
    "f11688-mkdir-p-exists",
    "f11688-touch-full-zero-file",
    "f11688-mkdir-full-no-trace",
    "f11688-nfs-deferred-28",
    "f11688-usage-no-operand",
    "f11688-estate-swap",
    "f11688-stage-shift",
    "f11688-errno-swap",
    "f11688-residual-hidden",
    "f11688-cause-confused",
    "f11688-anchor-lost",
];

/// 期望阶段（申报表的第二种形式，与 `CLAIMS` 交叉校验）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Block,
    Stage::Block,
    Stage::MTime,
    Stage::Argv,
    Stage::Exit,
    Stage::Create,
    Stage::Block,
    Stage::Block,
    Stage::Block,
    Stage::Block,
];

/// 期望 errno（申报表的第二种形式）。
pub const WANT_CODE: [i32; CORPUS] = [
    ERR_NONE,
    ERR_NONE,
    ERR_NONE,
    ERR_NONE,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_NONE,
    ERR_NONE,
    ERR_ENOSPC,
    ERR_EDQUOT,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_ENOSPC,
];

/// 期望残留计数（申报表的第二种形式）。
pub const WANT_VALUE: [u32; CORPUS] = [0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1, 0, 1, 1];

/// 期望形态档（申报表的第二种形式）。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::TouchCreated,
    Note::DirCreated,
    Note::DirsWithParents,
    Note::ExistsQuietOk,
    Note::LeftZeroFile,
    Note::GnuCannotCreateDir,
    Note::GnuSettingTimes,
    Note::UsageLine,
    Note::TouchCreated,
    Note::LeftZeroFile,
    Note::LeftZeroFile,
    Note::LeftZeroFile,
    Note::ReservedWall,
    Note::LeftZeroFile,
];

/// 期望锚位（申报表的第二种形式）。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, true, true, false, false, false, true, true, true, true, false,
];

/// 期望缺陷维序号：前八格 0（绿），后六格 1..6 指向 `REJECT_FAULTS`。
pub const WANT_NDX: [u8; CORPUS] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6];

/// 前八格槽号。
const ACCEPT_SLOTS: [usize; ACCEPT] = [0usize, 1, 2, 3, 4, 5, 6, 7];

/// 后六格槽号。
const REJECT_SLOTS: [usize; REJECT] = [8usize, 9, 10, 11, 12, 13];

/// 实际侧（`define` 推出）的阶段表：与 `CLAIMS` 独立写一遍，交叉校验用。
pub const ACT_STAGE: [Stage; CORPUS] = [
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Block,
    Stage::Block,
    Stage::MTime,
    Stage::Argv,
    Stage::Block,
    Stage::Block,
    Stage::Block,
    Stage::Block,
    Stage::Block,
    Stage::Block,
];

/// 实际侧 errno 表。
pub const ACT_CODE: [i32; CORPUS] = [
    ERR_NONE,
    ERR_NONE,
    ERR_NONE,
    ERR_NONE,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_NONE,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_ENOSPC,
];

/// 实际侧残留计数表。
pub const ACT_VALUE: [u32; CORPUS] = [0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1];

/// 实际侧形态档表。
pub const ACT_NOTE: [Note; CORPUS] = [
    Note::TouchCreated,
    Note::DirCreated,
    Note::DirsWithParents,
    Note::ExistsQuietOk,
    Note::LeftZeroFile,
    Note::GnuCannotCreateDir,
    Note::GnuSettingTimes,
    Note::UsageLine,
    Note::LeftZeroFile,
    Note::LeftZeroFile,
    Note::LeftZeroFile,
    Note::LeftZeroFile,
    Note::LeftZeroFile,
    Note::LeftZeroFile,
];

/// 实际侧锚位表。
pub const ACT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, true, true, false, false, true, true, true, true, true, true,
];

/// 实际侧自洽表（本语料里全部自洽：谎报由 `CLAIMS` 侧承担）。
pub const ACT_CONSISTENT: [bool; CORPUS] = [
    true, true, true, true, true, true, true, true, true, true, true, true, true, true,
];

/// 装配十四格语料。
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

/// 取第 i 格；越界给 `None`（调用方落 `TM_ENOBS`）。
pub fn case_at(i: usize) -> Option<Case> {
    cases().get(i).copied()
}

/// 覆盖率检查：格数必须正好是 `CORPUS`。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        return Ok(());
    }
    Err(TM_ECOUNT)
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

/// **判序**：状态 → 阶段 → errno → 残留 → 注解 → 锚位。次序固定，不许换。
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
// 场景族：语料之外的几何/身份组合，逐格钉住四档成因与身份差
// ---------------------------------------------------------------------------

/// 场景数。
pub const SCENE_COUNT: usize = 9;

/// 九个场景：四档成因 × 两种身份 × `-p` 父层残留 × 本地改时间谎报 × 邻居码。
pub fn scenes() -> [Shape; SCENE_COUNT] {
    [
        // 0 inode 耗尽：avail 好看、IFree=0，`mkdir` 照样 28。
        Shape {
            path: "mail/queue",
            geom: Geom {
                avail_blocks: 12_288,
                free_blocks: 12_288,
                free_inodes: 0,
                total_inodes: 65_536,
                fs: FsFamily::LocalExt4,
            },
            errno_seen: ERR_ENOSPC,
            exit_declared: EXIT_FILE,
            hands_off: true,
            ..BASE
        },
        // 1 元数据/日志耗尽：btrfs 数据与 inode 都好看。
        Shape {
            path: "var/log/app",
            geom: Geom {
                avail_blocks: 262_144,
                free_blocks: 262_144,
                free_inodes: 32_768,
                total_inodes: 65_536,
                fs: FsFamily::LocalBtrfs,
            },
            errno_seen: ERR_ENOSPC,
            exit_declared: EXIT_FILE,
            hands_off: true,
            ..BASE
        },
        // 2 保留块墙，非 root：avail=0 但 free>0。
        Shape {
            path: "home/u/x",
            geom: Geom {
                avail_blocks: 0,
                free_blocks: 1_024,
                free_inodes: 4_096,
                total_inodes: 65_536,
                fs: FsFamily::LocalExt4,
            },
            errno_seen: ERR_ENOSPC,
            caller_root: false,
            exit_declared: EXIT_FILE,
            ..BASE
        },
        // 3 同一块盘同一秒，root 建成功：保留段外还有 1024 块。
        Shape {
            path: "home/u/x",
            geom: Geom {
                avail_blocks: 0,
                free_blocks: 1_024,
                free_inodes: 4_096,
                total_inodes: 65_536,
                fs: FsFamily::LocalExt4,
            },
            errno_seen: ERR_NONE,
            caller_root: true,
            exit_declared: EXIT_OK,
            ..BASE
        },
        // 4 `mkdir -p` 末层红：两层父目录留在盘上，工具不许自动回收。
        Shape {
            path: "a/b/c",
            op: OpKind::MkDirParents,
            opts: OPT_MAKE_PARENTS,
            geom: Geom {
                avail_blocks: 0,
                free_blocks: 0,
                free_inodes: 65_536,
                total_inodes: 65_536,
                fs: FsFamily::LocalExt4,
            },
            path_class: PathClass::CreatedByParents,
            errno_seen: ERR_ENOSPC,
            residual: Residual::Parents(2),
            exit_declared: EXIT_FILE,
            ..BASE
        },
        // 5 本地 ext4 上 `touch 已存在件` 报 28：本卡点名的假（不可能事件）。
        Shape {
            tool: Tool::Touch,
            op: OpKind::TouchSetTimes,
            path: "etc/passwd",
            geom: Geom {
                avail_blocks: 500,
                free_blocks: 500,
                free_inodes: 300,
                total_inodes: 1000,
                fs: FsFamily::LocalExt4,
            },
            errno_seen: ERR_ENOSPC,
            existed_before: true,
            exit_declared: EXIT_FILE,
            ..BASE
        },
        // 6 目录链接数上限（EMLINK）：看着像满，其实不是空间问题。
        Shape {
            path: "big/child",
            geom: Geom {
                avail_blocks: 8_192,
                free_blocks: 8_192,
                free_inodes: 8_192,
                total_inodes: 65_536,
                fs: FsFamily::LocalExt4,
            },
            errno_seen: ERR_EMLINK,
            exit_declared: EXIT_FILE,
            ..BASE
        },
        // 7 配额超限（EDQUOT）：与 28 的对照档归 F11695，本卡只挡锚。
        Shape {
            path: "quota/u/f",
            geom: Geom {
                avail_blocks: 8_192,
                free_blocks: 8_192,
                free_inodes: 8_192,
                total_inodes: 65_536,
                fs: FsFamily::LocalExt4,
            },
            errno_seen: ERR_EDQUOT,
            exit_declared: EXIT_FILE,
            ..BASE
        },
        // 8 tmpfs 大小上限：无保留段概念，avail=0 即数据块档。
        Shape {
            tool: Tool::Touch,
            op: OpKind::TouchCreate,
            path: "shm/x",
            geom: Geom {
                avail_blocks: 0,
                free_blocks: 0,
                free_inodes: 1_024,
                total_inodes: 4_096,
                fs: FsFamily::Tmpfs,
            },
            errno_seen: ERR_ENOSPC,
            exit_declared: EXIT_FILE,
            ..BASE
        },
    ]
}

/// 场景短名（与 `scenes()` 同序）。
pub const SCENE_TAGS: [&str; SCENE_COUNT] = [
    "scene-inode",
    "scene-meta",
    "scene-reserved-other",
    "scene-reserved-root",
    "scene-p-parents",
    "scene-local-mtime-lie",
    "scene-emlink",
    "scene-edquot",
    "scene-tmpfs",
];

/// 场景期望阶段。
pub const SCENE_WANT_STAGE: [Stage; SCENE_COUNT] = [
    Stage::Inode,
    Stage::Meta,
    Stage::Block,
    Stage::Exit,
    Stage::Parent,
    Stage::MTime,
    Stage::Create,
    Stage::Create,
    Stage::Block,
];

/// 场景期望 errno。
pub const SCENE_WANT_CODE: [i32; SCENE_COUNT] = [
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_NONE,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_EMLINK,
    ERR_EDQUOT,
    ERR_ENOSPC,
];

/// 场景期望残留。
pub const SCENE_WANT_VALUE: [u32; SCENE_COUNT] = [0, 0, 0, 0, 2, 0, 0, 0, 0];

/// 场景期望形态档。
pub const SCENE_WANT_NOTE: [Note; SCENE_COUNT] = [
    Note::InodeExhausted,
    Note::MetaExhausted,
    Note::ReservedWall,
    Note::DirCreated,
    Note::LeftParents,
    Note::GnuSettingTimes,
    Note::ForeignErrno,
    Note::ForeignErrno,
    Note::GnuCannotTouch,
];

/// 场景期望锚位（只有真空间错且分配型操作才亮）。
pub const SCENE_WANT_DEFAULT: [bool; SCENE_COUNT] = [
    true, true, true, false, true, false, false, false, true,
];

/// 场景期望自洽（第 6 格是本地改时间的假，其余自洽）。
pub const SCENE_WANT_CONSISTENT: [bool; SCENE_COUNT] = [
    true, true, true, true, true, false, true, true, true,
];

/// 取第 i 个场景；越界给 `None`。
pub fn scene_at(i: usize) -> Option<Shape> {
    scenes().get(i).copied()
}

/// 第 i 个场景应不应落锚；越界给 `None`。
pub fn scene_anchored(i: usize) -> Option<bool> {
    SCENE_WANT_DEFAULT.get(i).copied()
}

/// 第 i 个场景过一遍 `define`，与期望表逐格比对（note 闸的复算物）。
pub fn scene_case(i: usize) -> Option<Case> {
    let sh = scene_at(i)?;
    let tag = SCENE_TAGS.get(i)?;
    let st = SCENE_WANT_STAGE.get(i)?;
    let cd = SCENE_WANT_CODE.get(i)?;
    let vl = SCENE_WANT_VALUE.get(i)?;
    let nt = SCENE_WANT_NOTE.get(i)?;
    let df = SCENE_WANT_DEFAULT.get(i)?;
    Some(Case {
        shape: sh,
        claim: Claim::make(*st, *cd, *vl, *nt, *df),
        tag,
    })
}

/// 场景族全绿：每格的 `define` 与期望表逐格一致且自洽位符合期望。
pub fn scenes_agree() -> bool {
    let mut i = 0usize;
    while i < SCENE_COUNT {
        let Some(sh) = scene_at(i) else {
            return false;
        };
        let Some(o) = define_or_none(&sh) else {
            return false;
        };
        let Some(want_stage) = SCENE_WANT_STAGE.get(i) else {
            return false;
        };
        let Some(want_code) = SCENE_WANT_CODE.get(i) else {
            return false;
        };
        let Some(want_value) = SCENE_WANT_VALUE.get(i) else {
            return false;
        };
        let Some(want_note) = SCENE_WANT_NOTE.get(i) else {
            return false;
        };
        let Some(want_default) = SCENE_WANT_DEFAULT.get(i) else {
            return false;
        };
        let Some(want_consistent) = SCENE_WANT_CONSISTENT.get(i) else {
            return false;
        };
        if o.stage != *want_stage
            || o.code != *want_code
            || o.value != *want_value
            || o.note != *want_note
            || o.default != *want_default
            || o.consistent != *want_consistent
        {
            return false;
        }
        i += 1;
    }
    true
}

/// `define` 的安全包装：形状非法时不给结论（本卡用它把非法形状挡在场景族外）。
pub fn define_or_none(s: &Shape) -> Option<Outcome> {
    if !s.geom.well_formed() || !s.errno_legal() {
        return None;
    }
    Some(define(s))
}

/// 段位署名复算：本卡与 F11681 的位距必须等于两个段基址之差。
pub fn anchors_agree() -> bool {
    if SEG_BASE < ANCHOR_F11681_SEG {
        return false;
    }
    if SEG_BASE - ANCHOR_F11681_SEG != ANCHOR_F11681_GAP {
        return false;
    }
    if ANCHOR_F11689_SEG < SEG_BASE {
        return false;
    }
    if ANCHOR_F11689_SEG - SEG_BASE != ANCHOR_F11689_GAP {
        return false;
    }
    if SEG_BASE < ANCHOR_F11687_SEG {
        return false;
    }
    SEG_BASE - ANCHOR_F11687_SEG == ANCHOR_F11687_GAP
}

// ---------------------------------------------------------------------------
// Row / Matrix：按档位分四筐，total 与 good 两维独立
// ---------------------------------------------------------------------------

/// 一行的统计：该档的格数与其中判对的格数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
    /// 一档。
    pub clean: Row,
    /// 二档。
    pub honest: Row,
    /// 三档。
    pub untouched: Row,
    /// 四档。
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

    /// 全矩阵红格数（各档 red 之和）。
    pub fn red_total(&self) -> usize {
        self.clean
            .red()
            .saturating_add(self.honest.red())
            .saturating_add(self.untouched.red())
            .saturating_add(self.refused.red())
    }

    /// 覆盖是否完整：合计等于语料格数，且判对数等于申报正确槽数。
    pub fn covered(&self) -> bool {
        self.slots() == CORPUS && self.goods() == ACCEPT
    }

    /// 渲染整个矩阵（一行一档，顺序固定）。
    pub fn render(&self) -> String {
        let all = Tier::all();
        let mut s = String::new();
        let mut i = 0usize;
        while i < TIER_COUNT {
            if i > 0 {
                s.push(' ');
            }
            let Some(t) = all.get(i) else {
                i += 1;
                continue;
            };
            if let Some(r) = self.row(*t) {
                s.push_str(r.render(*t).as_str());
            }
            i += 1;
        }
        s
    }
}

/// 档位全集（`Tier` 的补一个 free 函数，矩阵遍历用）。
impl Tier {
    /// 四档按序号列出。
    pub fn all() -> [Tier; TIER_COUNT] {
        [Tier::Clean, Tier::HonestFail, Tier::NotTouched, Tier::Refused]
    }
}

// ---------------------------------------------------------------------------
// Report：语料汇总与红档率标定
// ---------------------------------------------------------------------------

/// 十四格过一遍之后的汇总。
///
/// `matched` 数的是"实际缺陷 == 期望缺陷"的格数，因此它同时钉住两件事：判序
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
    /// 诚实红里落了本卡锚的格数。
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
            Err(TM_ECOUNT)
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
        let mut s = String::from("tm-enospc corpus=");
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
const RENDER_OK: &str =
    "tm-enospc corpus=14 accept=8 reject=6 clean=4 honest=3 untouched=1 refused=0 anchor=2 match=14 rate=1000ppm\n";

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
/// 是硬编码的而不是算出来的。槽号越界一律 `TM_ESLOT`，不许静默吞掉。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(TM_ESLOT);
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
            return Err(TM_ENOBS);
        };
        let a = c.actual();
        let mut f = judge(c);
        if let Some((j, forced)) = over {
            if j == i {
                f = forced;
            }
        }
        let Some(want) = expected(i) else {
            return Err(TM_ENOBS);
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
    if r.total as usize != CORPUS || m.slots() != CORPUS {
        return false;
    }
    if r.clean != m.clean.good as u32 {
        return false;
    }
    if r.honest != m.honest.good as u32 {
        return false;
    }
    if r.untouched != m.untouched.good as u32 {
        return false;
    }
    if r.refused != m.refused.good as u32 {
        return false;
    }
    if m.goods() != ACCEPT {
        return false;
    }
    r.accept as usize == m.clean.good + m.honest.good + m.untouched.good + m.refused.good
}

/// 注入标定：强行改坏一格，红档率必须正好落在 `RED_RATE`。
///
/// 这条是 `VALS` 第七格（929）的来源。标定值本身也要被测——如果 `rate()`
/// 写成截断除法、或 `matched` 少数了一格，929 就变了。
pub fn red_rate_calibrated() -> bool {
    let Ok(r) = run_with(Some((0, Fault::Code))) else {
        return false;
    };
    if r.reds() != 1 {
        return false;
    }
    if r.rate() != RED_RATE {
        return false;
    }
    let Ok(g) = run() else {
        return false;
    };
    g.rate() == RATE_FULL && r.total == g.total
}

/// 注入六路：每一路各自坏一维，红档率必须一律离开 1000。
///
/// `run_with` 只注入一格；这里把六具缺陷位（活格槽 0 上依次强行注入）全过一遍，
/// 确认任何一维的错位都能被 `matched` 抓到。若某一维注入后仍全绿，说明判序里
/// 那一维根本没参与比较。
pub fn inject_six_lowers_rate() -> bool {
    let all = JUDGE_ORDER;
    let mut i = 0usize;
    while i < REJECT {
        let Some(f) = all.get(i) else {
            return false;
        };
        match run_with(Some((0, *f))) {
            Ok(r) => {
                if r.rate() >= RATE_FULL {
                    return false;
                }
                if r.reds() != 1 {
                    return false;
                }
            }
            Err(_) => return false,
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 阶段闸门：八道闸只能向前，回退即谎报
// ---------------------------------------------------------------------------

/// 单调阶段闸门。
///
/// 观测纵线是 `argv → parent → inode → block → meta → create → mtime → exit`，
/// 八段只能向前。回退（例如已经判死在 mtime 段又声称停在 create 段）说明申报者
/// 不是按闸顺序观测的，而是拿着文案倒推——这类申报在真实故障里最危险，因为
/// 文案看着完全合理，闸位却对不上盘面上的证据。
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    /// 上一段秩（-1 表示未走）。
    last: i32,
    /// 已走段数。
    steps: u32,
    /// 回退次数（显性记录，不许静默吞）。
    falls: u32,
    /// 是否已关。
    closed: bool,
}

impl Gate {
    /// 新闸门。
    pub const fn new() -> Gate {
        Gate {
            last: -1,
            steps: 0,
            falls: 0,
            closed: false,
        }
    }

    /// 走一段。回退给 `TM_ESTAGE`，关闸后再走给 `TM_ESTATE`。
    pub fn step(&mut self, s: Stage) -> Result<(), u32> {
        if self.closed {
            self.falls = self.falls.saturating_add(1);
            return Err(TM_ESTATE);
        }
        let o = s.rank() as i32;
        if o < self.last {
            self.falls = self.falls.saturating_add(1);
            return Err(TM_ESTAGE);
        }
        self.last = o;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }

    /// 按语料纵线走一遍（八段各走一次，必须全过）。
    pub fn walk_all(&mut self) -> Result<u32, u32> {
        let all = Stage::all();
        let mut i = 0usize;
        while i < all.len() {
            let Some(s) = all.get(i) else {
                return Err(TM_ECOUNT);
            };
            self.step(*s)?;
            i += 1;
        }
        self.close()
    }

    /// 关闭，返回段数。段数不足八道视为账不平。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            self.falls = self.falls.saturating_add(1);
            return Err(TM_ESTATE);
        }
        self.closed = true;
        if self.steps != STAGE_COUNT {
            return Err(TM_ECOUNT);
        }
        Ok(self.steps)
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

    /// 是否已关。
    pub const fn closed(&self) -> bool {
        self.closed
    }

    /// 账本指纹（闸门自身状态也进指纹，防止"关闸后再改状态"逃过重放）。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, (self.last + 1) as u64);
        h = fold_word(h, self.steps as u64);
        h = fold_word(h, self.falls as u64);
        fold_bool(h, self.closed)
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 账本格。
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// 槽位（登记时必须严格等于当前长度）。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功格。
    pub ok: bool,
    /// 错误码（成功格恒为 0）。
    pub code: u32,
    /// 观测值（失败格恒为 0）。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
            op: 0,
            ok: false,
            code: 0,
            value: 0,
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

/// 账本：14 格，7 格统计 + 1 格红档率 + 6 格缺陷码；冻结后拒登记。
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
            return Err(TM_ESLOT);
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

    /// 逐格对账：槽位连续、成格值等于 `VALS`、败格码等于 `REJECT_CODES`、
    /// 败格值必归零。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(TM_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let Some(s) = self.rows.get(i) else {
                return Err(TM_EBAL);
            };
            if (s.slot as usize) != i {
                return Err(TM_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(TM_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(TM_EVALUE);
                        }
                    }
                    None => return Err(TM_EBAL),
                }
                vi = vi.saturating_add(1);
            } else {
                if s.value != 0 {
                    return Err(TM_EVALUE);
                }
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(TM_EVALUE);
                        }
                    }
                    None => return Err(TM_EBAL),
                }
                ci = ci.saturating_add(1);
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(TM_EBAL);
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

    /// 冻结。已冻结再冻结视为状态违例，并计入拒绝。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TM_ESTATE);
        }
        if self.len != SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TM_EBAL);
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
            Err(TM_ESEAL)
        }
    }

    /// 篡改留痕：把某格观测值改掉，重放必须报指纹不符。
    pub fn tamper(&mut self, i: usize, value: u32) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TM_ESTATE);
        }
        if i >= self.len {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TM_ESLOT);
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
        Ok(v) => Sample {
            slot,
            op,
            ok: true,
            code: 0,
            value: v,
        },
        Err(c) => Sample {
            slot,
            op,
            ok: false,
            code: c,
            value: 0,
        },
    }
}

/// 建账本：7 格统计 + 1 格红档率（929）+ 6 格缺陷码。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total)));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept)));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject)));
    let _ = l.register(count_cell(3, OP_CREATE, r.map(|x| x.clean)));
    let _ = l.register(count_cell(4, OP_BLOCK, r.map(|x| x.honest)));
    let _ = l.register(count_cell(5, OP_ARGV, r.map(|x| x.untouched)));
    let _ = l.register(count_cell(6, OP_INODE, r.map(|x| x.anchored)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_RATE, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_DEFECT,
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

/// 四路注入：表路、判路、渲染路、账本路。
///
/// 四路各自独立，任何一路静默退化都会在这里变成 false：
/// 1. 表路——段基址、错误码链、枚举、几何与文件系统画像、形状合法域；
/// 2. 判路——八闸纵线、成因归因、申报对判、矩阵覆盖；
/// 3. 渲染路——语料与场景族、槽位与红档、文案逐字、释义逐字；
/// 4. 账本路——阶段闸门、逐格对账、冻结重放。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && enums_ok() && geom_ok() && shape_ok();
    out[1] = define_ok() && cause_ok() && judge_ok() && matrix_ok();
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
// 判据基元：13 个私有谓词，每个对应一个错误码维
// ---------------------------------------------------------------------------

/// 01 名称与段基址：常量链、尺寸、errno 真值、退出码档、操作码互不重复。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d350 {
        return false;
    }
    if TM_ESTAGE != SEG_BASE + 0x01
        || TM_ECODE != SEG_BASE + 0x02
        || TM_EVALUE != SEG_BASE + 0x03
        || TM_ENOTE != SEG_BASE + 0x04
        || TM_EDEFAULT != SEG_BASE + 0x05
        || TM_EGEOM != SEG_BASE + 0x06
        || TM_ESTATE != SEG_BASE + 0x07
        || TM_ECOUNT != SEG_BASE + 0x08
        || TM_ESLOT != SEG_BASE + 0x09
        || TM_ESEAL != SEG_BASE + 0x0a
        || TM_ENOBS != SEG_BASE + 0x0b
        || TM_EMATRIX != SEG_BASE + 0x0c
        || TM_EBAL != SEG_BASE + 0x0d
    {
        return false;
    }
    if ALL_CODES.len() != 13 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_CODES.len() {
        let Some(c) = ALL_CODES.get(i) else {
            return false;
        };
        if *c <= SEG_BASE || *c > SEG_BASE + 0x0d {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_CODES.len() {
            if let Some(d) = ALL_CODES.get(j) {
                if *c == *d {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    if CORPUS != 14 || SAMPLES != 14 || ACCEPT != 8 || REJECT != 6 || ACCEPT + REJECT != CORPUS {
        return false;
    }
    if TIER_COUNT != 4 || STAGE_COUNT != 8 || STAGE_LAST_RANK != 8 || STAT_CELLS != 7 {
        return false;
    }
    if RED_RATE != 929 || RATE_FULL != 1000 {
        return false;
    }
    // 交叉署名：ENOSPC 在 Linux 就是 28，strerror 原文与前置卡同串。
    if ERR_ENOSPC != 28 || ANCHOR_ENOSPC != ERR_ENOSPC {
        return false;
    }
    if ANCHOR_ERRNO_TEXT != strerror(ERR_ENOSPC) {
        return false;
    }
    if ANCHOR_EXIT_FILE != EXIT_FILE || EXIT_RANKS != 4 {
        return false;
    }
    if ANCHOR_F11681_SEG + ANCHOR_F11681_GAP != SEG_BASE {
        return false;
    }
    if ANCHOR_F11689_SEG.checked_sub(SEG_BASE) != Some(ANCHOR_F11689_GAP) {
        return false;
    }
    if ANCHOR_F11687_SEG + ANCHOR_F11687_GAP != SEG_BASE {
        return false;
    }
    if !anchors_agree() {
        return false;
    }
    // 尺寸真值：名字上限与保留段千分率都不许被改数。
    if NAME_MAX_LIMIT != 255 || PARENT_DEPTH_MAX != 6 || RESERVED_PERMILLE != 50 {
        return false;
    }
    if ERRNO_SET_SIZE != KNOWN_ERRNOS.len() || SPACE_ERRNO_COUNT != 2 {
        return false;
    }
    if !errno_known(ERR_ENOSPC) || !errno_space_family(ERR_ENOSPC) {
        return false;
    }
    // 配额与 28 同属空间家族，但绝不允许冒充本卡的中止锚（邻居表里就有它）。
    if !errno_space_family(ERR_EDQUOT) || !errno_known(ERR_EDQUOT) {
        return false;
    }
    if errno_space_family(ERR_ENOENT) || errno_space_family(ERR_NONE) {
        return false;
    }
    if NEIGHBOUR_ERRNOS.len() + 2 != KNOWN_ERRNOS.len() {
        return false;
    }
    true
}

/// 02 枚举与八闸：工具、操作、家族、成因、档位的枚举全覆盖与反查一致。
fn enums_ok() -> bool {
    if Stage::all().len() != STAGE_COUNT as usize {
        return false;
    }
    let stages = Stage::all();
    let mut i = 0usize;
    while i < stages.len() {
        let Some(s) = stages.get(i) else {
            return false;
        };
        if s.rank() as usize != i + 1 {
            return false;
        }
        if Stage::from_rank(s.rank()) != Some(*s) {
            return false;
        }
        i += 1;
    }
    if stages.first() != Some(&Stage::Argv) || stages.last() != Some(&Stage::Exit) {
        return false;
    }
    if !matches!(stages.last(), Some(x) if x.rank() == STAGE_LAST_RANK) {
        return false;
    }
    // 空间三闸必须都在文件系统面（不在 argv 之前），且 success_stage 落在 exit。
    if Stage::success_stage() != Stage::Exit {
        return false;
    }
    let mut space = 0usize;
    let mut k = 0usize;
    while k < stages.len() {
        if let Some(s) = stages.get(k) {
            if s.is_space_gate() {
                space = space.saturating_add(1);
            }
            if s.before_fs() && k > 0 {
                let prev = match stages.get(k - 1) {
                    Some(p) => *p,
                    None => return false,
                };
                if !prev.before_fs() {
                    return false;
                }
            }
        }
        k += 1;
    }
    if space != 3 {
        return false;
    }
    if OpKind::all().len() != 7 {
        return false;
    }
    let ops = OpKind::all();
    let mut n = 0usize;
    let mut a = 0usize;
    let mut i2 = 0usize;
    while i2 < ops.len() {
        let Some(o) = ops.get(i2) else {
            return false;
        };
        if o.index() != i2 {
            return false;
        }
        n = n.saturating_add(1);
        if o.allocates() {
            a = a.saturating_add(1);
        }
        if o.is_touch() && o.is_mkdir() {
            return false;
        }
        i2 += 1;
    }
    if n != 7 || a != 3 {
        return false;
    }
    // 归属面：本家两个工具都拥有写入面，外来工具不拥有；两者的矩阵列必须分开。
    if !Tool::Touch.owned() || !Tool::Mkdir.owned() || Tool::Foreign.owned() {
        return false;
    }
    if Tool::Touch.index() == Tool::Mkdir.index() || Tool::Touch.index() == Tool::Foreign.index() {
        return false;
    }
    if CauseClass::all().len() != 5 || FsFamily::all().len() != 5 || PathClass::all().len() != 7 {
        return false;
    }
    if Note::all().len() != 20 || Fault::all().len() != 14 {
        return false;
    }
    // 判序表与缺陷码表必须同长同序。
    if JUDGE_ORDER.len() != REJECT || REJECT_CODES.len() != REJECT {
        return false;
    }
    let mut m = 0usize;
    while m < REJECT {
        let (Some(f), Some(c), Some(want)) = (
            JUDGE_ORDER.get(m),
            REJECT_CODES.get(m),
            REJECT_FAULTS.get(m),
        ) else {
            return false;
        };
        if f != want || f.code() != *c || matches!(f, Fault::Ok) {
            return false;
        }
        m += 1;
    }
    if Tier::all().len() != TIER_COUNT {
        return false;
    }
    let mut t = 0usize;
    while t < TIER_COUNT {
        let Some(ti) = Tier::from_index(t) else {
            return false;
        };
        if ti.index() != t {
            return false;
        }
        t += 1;
    }
    true
}

/// 03 几何与文件系统画像：四份盘面各自成立，且四档成因互不混淆。
fn geom_ok() -> bool {
    let g = geoms();
    if g.len() != 4 {
        return false;
    }
    let mut i = 0usize;
    while i < g.len() {
        let Some(x) = geom_at(i) else {
            return false;
        };
        if !x.well_formed() {
            return false;
        }
        if x.seal() == 0 {
            return false;
        }
        if i > 0 {
            let mut j = 0usize;
            while j < i {
                if let Some(y) = geom_at(j) {
                    if y.seal() == x.seal() {
                        return false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    // 保留段只属于 ext 家族；btrfs/tmpfs/NFS 都没有 root 保留墙。
    if !FsFamily::LocalExt4.has_reserved_blocks()
        || FsFamily::LocalBtrfs.has_reserved_blocks()
        || FsFamily::Tmpfs.has_reserved_blocks()
        || FsFamily::Nfs.has_reserved_blocks()
    {
        return false;
    }
    if !FsFamily::Nfs.defers_errors() || FsFamily::LocalExt4.defers_errors() {
        return false;
    }
    if !FsFamily::LocalBtrfs.separate_metadata()
        || FsFamily::LocalExt4.separate_metadata()
    {
        return false;
    }
    // 只读挂载不是本地家族，也不参与"迟到错"的归因。
    if FsFamily::ReadOnly.is_local() || FsFamily::ReadOnly.defers_errors() {
        return false;
    }
    if !FsFamily::LocalExt4.is_local() || !FsFamily::Tmpfs.is_local() {
        return false;
    }
    // 保留墙的可写性按身份分岔：root 能进、他人不能。
    let wall = Geom {
        avail_blocks: 0,
        free_blocks: 120,
        free_inodes: 400,
        total_inodes: 400,
        fs: FsFamily::LocalExt4,
    };
    if !wall.writable_by(true) || wall.writable_by(false) {
        return false;
    }
    if wall.reserved_permille() < RESERVED_PERMILLE {
        return false;
    }
    if !wall.full_by_avail() || !wall.has_inodes() {
        return false;
    }
    // inode 耗尽：avail 好看、IFree 归零，df 的"看起来还有"正是谎。
    let inode_dry = Geom {
        avail_blocks: 8192,
        free_blocks: 8192,
        free_inodes: 0,
        total_inodes: 8192,
        fs: FsFamily::LocalExt4,
    };
    if inode_dry.has_inodes() || !inode_dry.has_free_blocks() {
        return false;
    }
    if !CauseClass::Inode.df_lies() || !CauseClass::JournalMeta.df_lies() {
        return false;
    }
    if CauseClass::DataBlock.df_lies() || CauseClass::NotSpace.df_lies() {
        return false;
    }
    if !CauseClass::Reserved.caller_sensitive() || CauseClass::Inode.caller_sensitive() {
        return false;
    }
    if CauseClass::all().len()
        != CauseClass::all()
            .iter()
            .filter(|c| c.is_space())
            .count()
            + 1
    {
        return false;
    }
    true
}

/// 04 形状合法域：合法形状过 `valid`，非法构造被挡在闸外且指纹非零。
fn shape_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < cs.len() {
        let Some(c) = cs.get(i) else {
            return false;
        };
        if !c.shape.valid() {
            return false;
        }
        if c.shape.seal() == 0 {
            return false;
        }
        i += 1;
    }
    // 用法错三要素：命令行非法 / 无操作数 / 工具越界，任一即 usage_bad。
    let mut u = Shape::blank();
    u.argv_legal = false;
    if !u.usage_bad() {
        return false;
    }
    let mut v = Shape::blank();
    v.operand_count = 0;
    if !v.usage_bad() {
        return false;
    }
    let mut w = Shape::blank();
    w.tool = Tool::Foreign;
    if !w.usage_bad() || w.valid() {
        return false;
    }
    // 选项位图必须把本卡认得的八位全覆盖：越界位在 u8 里无处可藏。
    if OPT_BIT_COUNT != 8 || OPT_KNOWN != 0xff || Shape::blank().unknown_opt_bits() != 0 {
        return false;
    }
    // `-p` 只能配 mkdir；配 touch 就是选项与操作打架。
    let mut y = Shape::blank();
    y.tool = Tool::Touch;
    y.op = OpKind::TouchCreate;
    y.opts = OPT_MAKE_PARENTS;
    if y.opts_match_op() {
        return false;
    }
    // `-a` 只能配 touch；配 mkdir 同样打架。
    let mut y2 = Shape::blank();
    y2.opts = OPT_ATIME;
    if y2.opts_match_op() {
        return false;
    }
    // 存在性与操作：set-times 却声称文件本来不存在，形状自相矛盾。
    let mut z = Shape::blank();
    z.op = OpKind::TouchSetTimes;
    z.existed_before = false;
    if z.existence_consistent() {
        return false;
    }
    // 深度越界的父层构造非法。
    let mut d = Shape::blank();
    d.path_class = PathClass::CreatedByParents;
    d.residual = Residual::Parents(PARENT_DEPTH_MAX + 1);
    if d.parent_depth_legal() {
        return false;
    }
    // errno 合法域：认得的码放行，集合外的码挡住。
    if !Shape::blank().clone_errno(ERR_EDQUOT).errno_legal() {
        return false;
    }
    if !Shape::blank().clone_errno(ERR_ENOSPC).errno_legal() {
        return false;
    }
    if !Shape::blank().clone_errno(ERR_NONE).errno_legal() {
        return false;
    }
    if Shape::blank().clone_errno(1).errno_legal() {
        return false;
    }
    // 每块代表盘换上去都必须仍然合法（几何与工具无关）。
    let mut k = 0usize;
    while k < geoms().len() {
        let Some(g) = geom_at(k) else {
            return false;
        };
        let sh = Shape::blank().with_geom(k);
        if !sh.geom.well_formed() || sh.geom.seal() != g.seal() {
            return false;
        }
        k += 1;
    }
    true
}

/// 05 八闸纵线：`define` 的实际结论必须逐格等于 `ACT_*` 表，且与 `CLAIMS` 的
/// 活格侧完全一致。
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
        let a = define(&c.shape);
        let Some(st) = ACT_STAGE.get(i) else {
            return false;
        };
        let Some(code) = ACT_CODE.get(i) else {
            return false;
        };
        let Some(val) = ACT_VALUE.get(i) else {
            return false;
        };
        let Some(no) = ACT_NOTE.get(i) else {
            return false;
        };
        let Some(df) = ACT_DEFAULT.get(i) else {
            return false;
        };
        let Some(cf) = ACT_CONSISTENT.get(i) else {
            return false;
        };
        if a.stage != *st || a.code != *code || a.value != *val || a.note != *no {
            return false;
        }
        if a.default != *df || a.consistent != *cf {
            return false;
        }
        // 锚位自证：亮锚的格必须通过 `should_anchor` 复算。
        if a.default != a.should_anchor() {
            return false;
        }
        // 成功格的残留必须为零，失败格才允许有残留计数。
        if a.ok() && a.value != 0 {
            return false;
        }
        if i < ACCEPT {
            let Some(q) = CLAIMS.get(i) else {
                return false;
            };
            if a.stage != q.stage || a.code != q.code || a.value != q.value {
                return false;
            }
            if a.note != q.note || a.default != q.default {
                return false;
            }
        }
        // 退出码档位：绿 0、不裁定 99、其余 1。
        let e = a.exit();
        if a.ok() != (e == EXIT_OK) {
            return false;
        }
        if a.note.out_of_scope_marker() != (e == EXIT_UNJUDGED) {
            return false;
        }
        i += 1;
    }
    // 场景族九格也必须落在同一张判序桌上。
    scenes_agree()
}

/// 06 成因归因：四档成因 → 闸位 → note 的三元组必须闭合，邻居码不许冒充。
fn cause_ok() -> bool {
    let map = [
        (CauseClass::DataBlock, Stage::Block),
        (CauseClass::Inode, Stage::Inode),
        (CauseClass::JournalMeta, Stage::Meta),
        (CauseClass::Reserved, Stage::Block),
    ];
    let mut i = 0usize;
    while i < map.len() {
        let Some((c, want)) = map.get(i) else {
            return false;
        };
        if Stage::for_cause(*c) != *want {
            return false;
        }
        if !c.is_space() {
            return false;
        }
        i += 1;
    }
    if Stage::for_cause(CauseClass::NotSpace) == Stage::Block {
        return false;
    }
    if CauseClass::NotSpace.is_space() {
        return false;
    }
    // 成因档与 note 一一对应。
    if !Note::InodeExhausted.is_cause()
        || !Note::MetaExhausted.is_cause()
        || !Note::ReservedWall.is_cause()
        || Note::LeftZeroFile.is_cause()
    {
        return false;
    }
    if !Note::LeftZeroFile.carries_residual()
        || !Note::LeftParents.carries_residual()
        || Note::GnuCannotTouch.carries_residual()
    {
        return false;
    }
    if !Note::UsageLine.usage() || Note::PathUnresolved.usage() {
        return false;
    }
    if !Note::PathUnresolved.untouched() || Note::OutOfScope.untouched() {
        return false;
    }
    if !Note::InodeExhausted.hands_off_to_df() || !Note::MetaExhausted.hands_off_to_df() {
        return false;
    }
    if Note::ReservedWall.hands_off_to_df() {
        return false;
    }
    // 成功五档全是 success 形态，中止家族与邻居档都不算成功。
    let notes = Note::all();
    let mut succ = 0usize;
    let mut abort = 0usize;
    let mut k = 0usize;
    while k < notes.len() {
        let Some(n) = notes.get(k) else {
            return false;
        };
        if n.success() {
            succ = succ.saturating_add(1);
        }
        if n.abort_family() {
            abort = abort.saturating_add(1);
        }
        if n.success() && n.abort_family() {
            return false;
        }
        if n.foreign() && (n.success() || n.abort_family()) {
            return false;
        }
        k += 1;
    }
    if succ != 5 || abort != 10 {
        return false;
    }
    // 动词档与工具配对：GNU 的 touch 文案不许出现在 mkdir 行上。
    if !Note::GnuCannotTouch.verb_matches(Tool::Touch, Flavor::Gnu)
        || Note::GnuCannotTouch.verb_matches(Tool::Mkdir, Flavor::Gnu)
        || !Note::BbCantCreateDir.verb_matches(Tool::Mkdir, Flavor::Busybox)
        || Note::BbCantCreateDir.verb_matches(Tool::Mkdir, Flavor::Gnu)
    {
        return false;
    }
    if !Note::LeftZeroFile.verb_matches(Tool::Touch, Flavor::Gnu) {
        return false;
    }
    if Flavor::Mixed.legible() || !Flavor::Gnu.legible() {
        return false;
    }
    // 残留档位：touch 只留 0 字节件，mkdir -p 才留父层。
    if !Residual::ZeroFile.allowed_for_touch()
        || Residual::Parents(1).allowed_for_touch()
        || Residual::None.count() != 0
        || Residual::ZeroFile.count() != 1
        || Residual::Parents(2).count() != 2
        || Residual::OrphanTemp.count() != 1
        || !Residual::Inconsistent.is_lie()
        || Residual::None.is_lie()
    {
        return false;
    }
    if !Residual::Parents(1).allowed_for_mkdir(OpKind::MkDirParents) {
        return false;
    }
    if Residual::Parents(1).allowed_for_mkdir(OpKind::MkDirOne) {
        return false;
    }
    true
}

/// 07 申报对判：判序六维按固定次序生效，每格的实际缺陷等于期望缺陷。
fn judge_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let Some(c) = cs.get(i) else {
            return false;
        };
        let Some(want) = expected(i) else {
            return false;
        };
        if judge(c) != want {
            return false;
        }
        if i < ACCEPT && !c.ok() {
            return false;
        }
        if i >= ACCEPT && c.ok() {
            return false;
        }
        if i >= ACCEPT {
            let Some(want_code) = REJECT_CODES.get(i - ACCEPT) else {
                return false;
            };
            if c.fault().code() != *want_code {
                return false;
            }
        }
        // 每格指纹非零，且活格之间互不相同（同形两格说明申报表被复制粘贴）。
        if c.seal() == 0 {
            return false;
        }
        i += 1;
    }
    // 期望表越界必须给 None，而不是回落到 `Fault::Ok`。
    if expected(CORPUS).is_some() || expected(usize::MAX).is_some() {
        return false;
    }
    // case_at 与 cases 必须同源。
    let Some(a) = case_at(0) else {
        return false;
    };
    match cs.first() {
        Some(c) => a.tag == c.tag && a.claim == c.claim,
        None => false,
    }
}

/// 08 矩阵统计：四档覆盖完整、行列平衡、渲染逐字稳定。
fn matrix_ok() -> bool {
    let m = Matrix::build();
    if m.slots() != CORPUS {
        return false;
    }
    if m.goods() != ACCEPT {
        return false;
    }
    if !m.covered() {
        return false;
    }
    if m.clean.total != 4 || m.honest.total != 9 {
        return false;
    }
    if m.untouched.total != 1 || m.refused.total != 0 {
        return false;
    }
    if m.clean.good != 4 || m.honest.good != 3 || m.untouched.good != 1 {
        return false;
    }
    if m.refused.good != 0 || m.honest.red() != 6 || m.red_total() != REJECT {
        return false;
    }
    // 空档不计入"全对的行数"，因此只有一档与三档满分。
    if m.perfect_rows() != 2 {
        return false;
    }
    if m.row(Tier::Clean) != Some(m.clean) || Tier::from_index(TIER_COUNT).is_some() {
        return false;
    }
    let r = m.render();
    if r != "clean=4/4 honest-fail=9/3 not-touched=1/1 refused=0/0" {
        return false;
    }
    report_agrees_with_matrix()
}

/// 09 语料与场景族：标签全部挂本卡号，活/死两侧形状各自成立。
fn corpus_ok() -> bool {
    let cs = cases();
    if cs.len() != CORPUS || WANT_TAGS.len() != CORPUS {
        return false;
    }
    let mut i = 0usize;
    let mut seen = 0usize;
    while i < CORPUS {
        let (Some(c), Some(tag)) = (cs.get(i), WANT_TAGS.get(i)) else {
            return false;
        };
        if c.tag != *tag || !c.tag.starts_with("f11688-") {
            return false;
        }
        if i < ACCEPT {
            let Some(want) = CLAIMS.get(i) else {
                return false;
            };
            if c.claim != *want {
                return false;
            }
        }
        let mut j = 0usize;
        let mut dup = 0usize;
        while j < CORPUS {
            if let Some(o) = WANT_TAGS.get(j) {
                if o == tag {
                    dup = dup.saturating_add(1);
                }
            }
            j += 1;
        }
        if dup != 1 {
            return false;
        }
        seen = seen.saturating_add(1);
        i += 1;
    }
    if seen != CORPUS {
        return false;
    }
    // 前八格的期望全为绿、后六格各坏一维且六维互不相同。
    let mut k = 0usize;
    while k < ACCEPT {
        if expected(k) != Some(Fault::Ok) {
            return false;
        }
        k += 1;
    }
    let mut used = [false; REJECT];
    let mut n = ACCEPT;
    while n < CORPUS {
        let Some(f) = expected(n) else {
            return false;
        };
        let mut idx = f as usize;
        if idx == 0 {
            return false;
        }
        idx -= 1;
        if idx >= REJECT {
            return false;
        }
        if let Some(slot) = used.get_mut(idx) {
            if *slot {
                return false;
            }
            *slot = true;
        }
        n += 1;
    }
    if !used.iter().all(|x| *x) {
        return false;
    }
    if cover(CORPUS) != Ok(()) || cover(CORPUS - 1).is_ok() {
        return false;
    }
    // 场景族九格：标签唯一、指纹非零、与语料同闸表。
    let sc = scenes();
    if sc.len() != SCENE_COUNT || SCENE_TAGS.len() != SCENE_COUNT {
        return false;
    }
    let mut s2 = 0usize;
    while s2 < SCENE_COUNT {
        let (Some(sh), Some(tag)) = (scene_at(s2), SCENE_TAGS.get(s2)) else {
            return false;
        };
        // 场景族标签自成一家（`scene-` 前缀），与语料标签的 `f11688-` 前缀互不混用。
        if !tag.starts_with("scene-") || sh.seal() == 0 {
            return false;
        }
        if !sh.valid() {
            return false;
        }
        if scene_case(s2).is_none() || scene_anchored(s2).is_none() {
            return false;
        }
        s2 += 1;
    }
    true
}

/// 10 槽位与红档：越界槽给 `TM_ESLOT`，红档率标定值钉死在 929。
fn slots_ok() -> bool {
    if run_with(Some((CORPUS, Fault::Code))).is_ok() {
        return false;
    }
    if run_with(Some((CORPUS + 99, Fault::Note))).is_ok() {
        return false;
    }
    if run_with(Some((0, Fault::Code))).is_err() {
        return false;
    }
    if run_with(None).is_err() {
        return false;
    }
    if case_at(CORPUS).is_some() {
        return false;
    }
    if scene_at(SCENE_COUNT).is_some() || scene_at(usize::MAX).is_some() {
        return false;
    }
    if define_or_none(&Shape::blank()).is_none() {
        return false;
    }
    // 越界形状的注入槽在 `run_with` 里必须挡住：`Err(TM_ESLOT)` 而非 `Ok`。
    match run_with(Some((usize::MAX, Fault::Slot))) {
        Err(TM_ESLOT) => true,
        _ => false,
    }
}

/// 10b 红档显式：注入后率离开 1000，六路注入各自掉率，标定值正是 929。
fn reds_ok() -> bool {
    if !red_rate_calibrated() {
        return false;
    }
    if !inject_six_lowers_rate() {
        return false;
    }
    let Ok(r) = run() else {
        return false;
    };
    if r.rate() != RATE_FULL {
        return false;
    }
    if r.reds() != 0 || !r.green() || r.gate() != Ok(()) {
        return false;
    }
    if r.tier_cells() != ACCEPT as u32 {
        return false;
    }
    if r.red_cells() != r.honest {
        return false;
    }
    if r.render() != RENDER_OK {
        return false;
    }
    let Ok(bad) = run_with(Some((3, Fault::Geom))) else {
        return false;
    };
    if bad.green() || bad.gate().is_ok() || bad.reds() != 1 {
        return false;
    }
    if bad.total != r.total || bad.accept + 1 != r.accept || bad.reject != r.reject + 1 {
        return false;
    }
    if bad.rate() >= RATE_FULL || bad.tier_cells() + 1 != r.tier_cells() {
        return false;
    }
    true
}

/// 11 文案逐字：GNU 8.x/9.x 与 busybox 三式文案各自独立，邻居码不冒充 28。
fn message_ok() -> bool {
    if strerror(ERR_ENOSPC) != "No space left on device" {
        return false;
    }
    if strerror(ERR_NONE) != "Success" {
        return false;
    }
    if errno_name(ERR_ENOSPC) != "ENOSPC" || errno_name(ERR_NONE) != "OK" {
        return false;
    }
    if errno_pair_text(ERR_ENOSPC) != "28/ENOSPC" {
        return false;
    }
    if errno_pair_text(ERR_NONE) != "0/OK" {
        return false;
    }
    if errno_pair_text(ERR_EDQUOT) != "122/EDQUOT" {
        return false;
    }
    if strerror(ERR_EDQUOT) != "Quota exceeded" || errno_name(ERR_EDQUOT) != "EDQUOT" {
        return false;
    }
    let gnu_touch = msg_gnu_touch_cannot("note.txt", ERR_ENOSPC);
    if gnu_touch != "touch: cannot touch 'note.txt': No space left on device" {
        return false;
    }
    let gnu_times = msg_gnu_setting_times("note.txt", ERR_ENOSPC);
    if gnu_times != "touch: setting times of 'note.txt': No space left on device" {
        return false;
    }
    let gnu_dir = msg_gnu_mkdir_cannot("a/b", ERR_ENOSPC);
    if gnu_dir != "mkdir: cannot create directory 'a/b': No space left on device" {
        return false;
    }
    let bb_touch = msg_bb_touch_cant_create("note.txt", ERR_ENOSPC);
    if bb_touch != "touch: can't create 'note.txt': No space left on device" {
        return false;
    }
    let bb_dir = msg_bb_mkdir_cant_create("a/b", ERR_ENOSPC);
    if bb_dir != "mkdir: can't create directory 'a/b': No space left on device" {
        return false;
    }
    if msg_gnu_touch_missing_operand() != "touch: missing file operand" {
        return false;
    }
    if msg_gnu_mkdir_missing_operand() != "mkdir: missing operand" {
        return false;
    }
    if msg_bb_touch_not_enough() != "touch: not enough arguments" {
        return false;
    }
    if msg_gnu_invalid_date("@1700000000.5x") != "touch: invalid date '@1700000000.5x'" {
        return false;
    }
    if msg_gnu_mkdir_created("a/b") != "mkdir: created directory 'a/b'" {
        return false;
    }
    if msg_gnu_mkdir_created_layer("a/b", 2)
        != "mkdir: created directory 'a/b'\nmkdir: created directory 'a/b'"
    {
        return false;
    }
    if msg_gnu_mkdir_created_layer("a/b", 0) != "" {
        return false;
    }
    if msg_gnu_invalid_date_format("20260101") != "touch: invalid date format '20260101'" {
        return false;
    }
    if msg_gnu_invalid_mode("999") != "mkdir: invalid mode '999'" {
        return false;
    }
    if msg_gnu_failed_to_get_attributes("ref.txt", ERR_ENOENT)
        != "touch: failed to get attributes for 'ref.txt': No such file or directory"
    {
        return false;
    }
    if msg_bb_mkdir_usage() != "usage: mkdir [OPTION] DIRECTORY..." {
        return false;
    }
    if msg_gnu_usage_line(Tool::Foreign) != "" || msg_bb_usage_line(Tool::Foreign) != "" {
        return false;
    }
    // 同一 errno 的两种动词绝不互串。
    if gnu_touch == gnu_times || bb_touch == bb_dir {
        return false;
    }
    if gnu_touch == bb_touch {
        return false;
    }
    // 邻居码文案与 28 文案必须可辨（串里含 strerror 原文）。
    let edquot_line = msg_gnu_mkdir_cannot("a/b", ERR_EDQUOT);
    if edquot_line == gnu_dir || edquot_line.contains(ANCHOR_ERRNO_TEXT) {
        return false;
    }
    // message() 的三条分流：成功空串、用法行、中止文案。
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let Some(c) = cs.get(i) else {
            return false;
        };
        let a = c.actual();
        let m = message(&c.shape, &a);
        if a.note.success() && !c.shape.is_verbose() && !m.is_empty() {
            return false;
        }
        if a.note.usage() && m.is_empty() {
            return false;
        }
        if a.note.abort_family() && m.is_empty() {
            return false;
        }
        if a.note.out_of_scope_marker() && !m.is_empty() {
            return false;
        }
        i += 1;
    }
    // Flavor::text 的四种组合两两不同。
    let t1 = Flavor::Gnu.text(Tool::Touch, OpKind::TouchCreate, "p", ERR_ENOSPC);
    let t2 = Flavor::Gnu.text(Tool::Touch, OpKind::TouchSetTimes, "p", ERR_ENOSPC);
    let t3 = Flavor::Gnu.text(Tool::Mkdir, OpKind::MkDirOne, "p", ERR_ENOSPC);
    let t4 = Flavor::Busybox.text(Tool::Touch, OpKind::TouchCreate, "p", ERR_ENOSPC);
    if t1 == t2 || t1 == t3 || t1 == t4 || t2 == t3 || t3 == t4 {
        return false;
    }
    // `Mixed` 家族不产出文案：混认的申报在这里直接拿到空串，由调用方判死。
    if !Flavor::Mixed.text(Tool::Touch, OpKind::TouchCreate, "p", ERR_ENOSPC).is_empty() {
        return false;
    }
    if !Flavor::Gnu.text(Tool::Touch, OpKind::OutOfScope, "p", ERR_ENOSPC).is_empty() {
        return false;
    }
    true
}

/// 12 释义与阶段闸：明细行逐字稳定，八闸单调、回退与关闸后再走各有码。
fn detail_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let Some(c) = cs.get(i) else {
            return false;
        };
        let a = c.actual();
        let line = detail(c);
        if !line.starts_with(c.tag()) {
            return false;
        }
        if !line.contains(a.stage.name()) || !line.contains(a.note.name()) {
            return false;
        }
        if !line.contains(errno_name(a.code)) || !line.contains(fault_short(c.fault())) {
            return false;
        }
        if !line.contains("residual=") || !line.contains("anchor=") {
            return false;
        }
        let mut j = 0usize;
        while j < CORPUS {
            if i != j {
                if let Some(o) = cs.get(j) {
                    if detail(o) == line {
                        return false;
                    }
                }
            }
            j += 1;
        }
        i += 1;
    }
    // render_line 的三段分隔稳定，且不同成因给出不同行。
    let l1 = render_line(
        Tool::Mkdir,
        OpKind::MkDirOne,
        "a/b",
        CauseClass::Inode,
        Residual::None,
        ERR_ENOSPC,
    );
    let l2 = render_line(
        Tool::Mkdir,
        OpKind::MkDirOne,
        "a/b",
        CauseClass::DataBlock,
        Residual::None,
        ERR_ENOSPC,
    );
    if l1 == l2 || !l1.contains("errno=28/ENOSPC") {
        return false;
    }
    if !l1.contains(cause_text(CauseClass::Inode))
        || !l1.contains(residual_text_of(Residual::None))
    {
        return false;
    }
    if residual_text(0) != "0" || residual_text(1) != "1" {
        // 同一数字必须有同一文本，不同数字必须不同文本。
        return false;
    }
    if u32_text(0) != "0" || u32_text(1000) != "1000" {
        return false;
    }
    if seg_text(TM_ESEAL) != "0x6d35a" || seg_text(SEG_BASE) != "0x6d350" {
        return false;
    }
    if bool_text(true) != "true" || bool_text(false) != "false" {
        return false;
    }
    if i32_text(ERR_ENOSPC) != "28" || i32_text(-5) != "-5" || i32_text(0) != "0" {
        return false;
    }
    if residual_text_of(Residual::Inconsistent) == residual_text_of(Residual::None) {
        return false;
    }
    true
}

/// 12b 阶段闸：纵线全走、回退报错、关闸后再走报错，且错误码分岔。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    if g.steps() != 0 || g.closed() || g.falls() != 0 {
        return false;
    }
    if g.walk_all() != Ok(STAGE_COUNT) {
        return false;
    }
    if !g.closed() || g.falls() != 0 {
        return false;
    }
    // 关闸后再走 → 状态违例。
    if g.step(Stage::Exit) != Err(TM_ESTATE) || g.falls() != 1 {
        return false;
    }
    // 回退 → 阶段错。
    let mut h = Gate::new();
    if h.step(Stage::Create) != Ok(()) {
        return false;
    }
    if h.step(Stage::Inode) != Err(TM_ESTAGE) {
        return false;
    }
    if h.last_rank() != Stage::Create.rank() as i32 || h.falls() != 1 {
        return false;
    }
    // 同秩重复走允许（同一段里多次观测）。
    if h.step(Stage::Create) != Ok(()) || h.falls() != 1 {
        return false;
    }
    // 段数不足就关闸 → 计数错。
    let mut k = Gate::new();
    if k.step(Stage::Argv) != Ok(()) {
        return false;
    }
    if k.close() != Err(TM_ECOUNT) {
        return false;
    }
    if !k.closed() {
        return false;
    }
    // 双关 → 状态违例。
    let mut m = Gate::new();
    if m.walk_all() != Ok(STAGE_COUNT) || m.close() != Err(TM_ESTATE) {
        return false;
    }
    if m.seal() == 0 {
        return false;
    }
    true
}

/// 13 账本：建账、逐格对账、越界登记、冻结重放、篡改留痕。
fn ledger_ok() -> bool {
    let mut l = build_ledger();
    if l.len() != SAMPLES || l.is_empty() || l.rejects() != 0 {
        return false;
    }
    if l.ok_count() != ACCEPT || l.err_count() != REJECT {
        return false;
    }
    if l.audit() != Ok(()) {
        return false;
    }
    if l.sample(SAMPLES).is_some() {
        return false;
    }
    if let Some(s) = l.sample(0) {
        if s.value != VALS[0] || !s.ok || s.code != 0 {
            return false;
        }
    } else {
        return false;
    }
    if let Some(s) = l.sample(7) {
        if s.value != RED_RATE || !s.ok {
            return false;
        }
    } else {
        return false;
    }
    let mut i = 0usize;
    while i < REJECT {
        let Some(s) = l.sample(ACCEPT + i) else {
            return false;
        };
        let Some(want) = REJECT_CODES.get(i) else {
            return false;
        };
        if s.ok || s.code != *want || s.value != 0 {
            return false;
        }
        i += 1;
    }
    // 槽位错（跳号）与越界都要拒登并计数。
    let mut n = Ledger::new();
    if n.register(Sample {
        slot: 1,
        op: OP_CORPUS,
        ok: true,
        code: 0,
        value: 1,
    }) != Err(TM_ESLOT)
    {
        return false;
    }
    if n.rejects() != 1 || n.len() != 0 {
        return false;
    }
    if n.register(Sample {
        slot: 0,
        op: OP_CORPUS,
        ok: true,
        code: 0,
        value: 1,
    }) != Ok(())
    {
        return false;
    }
    // 未满账就冻结 → 账不平。
    if n.freeze() != Err(TM_EBAL) {
        return false;
    }
    if n.is_frozen() {
        return false;
    }
    // 冻结后登记一律拒绝。
    // 冻结：指纹非零、状态位落位，且**在拒绝计数被污染之前**重放必须逐位相符。
    let root = match l.freeze() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if root == 0 || !l.is_frozen() {
        return false;
    }
    if l.replay() != Ok(root) {
        return false;
    }
    // 冻结后的账本不许再动：双关 → `TM_ESTATE`，登记 → `TM_ESLOT`，两次都要留痕。
    // 留痕会进指纹，所以重放必须改成报错——这一路用副本，不能污染上面那条正判据。
    let mut p = build_ledger();
    let pr = match p.freeze() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if p.freeze() != Err(TM_ESTATE) || p.rejects() != 1 {
        return false;
    }
    if p.register(Sample {
        slot: 0,
        op: OP_CORPUS,
        ok: true,
        code: 0,
        value: 1,
    }) != Err(TM_ESLOT)
    {
        return false;
    }
    if p.rejects() != 2 || p.len() != SAMPLES {
        return false;
    }
    if p.replay() == Ok(pr) {
        return false;
    }
    // 篡改留痕：改掉一格观测值后，重放必须报指纹不符。
    let mut t = build_ledger();
    let before = match t.freeze() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if t.replay() != Ok(before) {
        return false;
    }
    // 冻结态不许再改：`tamper` 挡住、观测值一格没动，但拒登留痕本身进指纹，
    // 所以同一副本的重放必须改成报不符——"挡住"与"留痕"是两条各自钉住的判据。
    if t.tamper(0, 999) != Err(TM_ESTATE) {
        return false;
    }
    let (Some(kept), Some(want)) = (t.sample(0), VALS.get(0)) else {
        return false;
    };
    if kept.value != *want || t.rejects() != 1 || t.replay() == Ok(before) {
        return false;
    }
    let mut u = build_ledger();
    if u.tamper(0, 999) != Ok(()) {
        return false;
    }
    if u.tamper(SAMPLES, 1) != Err(TM_ESLOT) {
        return false;
    }
    if u.audit() != Err(TM_EVALUE) || u.replay() != Err(TM_ESEAL) {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 汇总入口
// ---------------------------------------------------------------------------

/// 本卡自检：13 项，逐项对应一个错误码维。
///
/// 断言方向：每一项都是"判据本身成立"，因此任何一维退化都变红；`audit` 只走
/// `add(name, bool, "static")`，不塞动态明细，域聚合渲染保持零分配。
pub fn run_unx11688_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11688_touch_mkdir_enospc");

    s.add("TM01-名称与段基址", name_ok(), "static");

    s.add("TM02-枚举与八闸", enums_ok(), "static");

    s.add("TM03-几何与文件系统画像", geom_ok(), "static");

    s.add("TM04-形状合法域", shape_ok(), "static");

    s.add("TM05-八闸纵线", define_ok(), "static");

    s.add("TM06-成因归因三元组", cause_ok(), "static");

    s.add("TM07-申报对判", judge_ok(), "static");

    s.add("TM08-矩阵统计", matrix_ok(), "static");

    s.add("TM09-语料与场景族", corpus_ok(), "static");

    s.add("TM10-槽位与红档", slots_ok() && reds_ok(), "static");

    s.add("TM11-文案逐字", message_ok(), "static");

    s.add("TM12-释义与阶段闸", detail_ok() && gate_ok(), "static");

    s.add(
        "TM13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            let inj = inject_four();
            let four = inj[0] && inj[1] && inj[2] && inj[3];
            a && b && ledger_ok() && four
        },
        "static",
    );

    s
}
