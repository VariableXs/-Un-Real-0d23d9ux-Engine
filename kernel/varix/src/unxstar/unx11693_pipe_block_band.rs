//! UNX-F11693 管道写满 ENOSPC/阻塞分档判据
//!
//! 这张卡管的是**管道写满那一瞬间到底发生了什么**。判据原文（`_vx5_cards.md`
//! `[UNX-F11693]`，验收 ②）是：
//!
//! ```text
//! UNX-F11693-J1 管道满阻塞档判据绿（非 ENOSPC 实况），与基准机一致
//! ```
//!
//! 括弧里那五个字是本卡的全部重量：**管道满不是 ENOSPC 实况**。管道的"满"是
//! 流控，不是空间——写端睡到读端回收为止，一个字节都不会丢，也一个块都不占用
//! 文件系统。把它读成 28 的运维会去清盘，而盘其实空得很：
//!
//! ```text
//! $ dd if=/dev/zero bs=1M count=1024 | slow-reader   # 写端卡住，不是报错
//! $ cat big.txt | head -1                            # 读端先退
//! cat: write error: Broken pipe                      # 32，不是 28
//! $ echo $?                                          # 141 = 128 + 13(SIGPIPE)
//! ```
//!
//! 于是本卡的域模型里，**空间维（`free_blocks`）对管道类目标是恒不适用量**：
//! 它必须写 0。谁在管道行上填了正的 `free_blocks`，谁就在宣称"这有块可分配"，
//! 而管道根本没有块——这条被 `Capacity::well_formed()` 当场压成谎报档。反过来，
//! 真 28 只允许出现在 `Target::RegularFile` 且 `free_blocks == 0` 的行上，也只有
//! 那种行才允许落本卡的实况锚（`Outcome::default`）。J1 说的"非 ENOSPC 实况"就是
//! 这一条闸：**锚位与目标类型互锁**。
//!
//! 第二条纵线是**阻塞的四档分野**，四档全部由同一份几何（排队字节 / 容量 / 读者
//! 引用数 / 是否 `O_NONBLOCK`）算出来，不许靠申报：
//!
//! * **阻塞档**（`Blocked`）：读者在、`O_NONBLOCK` 未置、`want > room`。写端睡，
//!   醒来继续写，最终 `written == want`、errno 0、退出 0。**这一档判据绿**——
//!   它就是 J1 的正解锚。
//! * **缓塞档**（`WouldBlock`）：同一份几何，但置了 `O_NONBLOCK`。内核不睡，
//!   收下 `room` 字节后回 `EAGAIN`（11，`Resource temporarily unavailable`），
//!   尾巴 `want - room` 留在用户态缓冲里——这就是本卡的 `value` 维（残写字节数）。
//! * **断链档**（`BrokenPipe` / `SigpipeDeath`）：`readers == 0`。errno 是
//!   `EPIPE`（32）；差别只在信号——外部命令没屏蔽 SIGPIPE → 死于 13 号信号、
//!   状态 141；shell 内建（`printf`/`echo`）把 SIGPIPE 置了 `SIG_IGN` → 进程活着、
//!   拿到 errno、退 1。**同一 errno、同一几何，两种退出码**，本卡把它俩拆成两个
//!   note 档，因为运维按 141 找"被管道杀掉的进程"，按 1 找"报了写错的内建"。
//! * **无读者开档**（`OpenNoReader`）：`O_NONBLOCK | O_WRONLY` 打开没人开的 FIFO
//!   连 errno 都不给 EPIPE，直接 `ENXIO`（6，`No such device or address`），一字节
//!   未写、残写等于申报量。这一档必须排在引用计数闸之前——它是 `open()` 的红，
//!   不是 `write()` 的红。
//!
//! 第三条纵线是**原子性**：`want > PIPE_BUF(4096)` 的阻塞写不保证原子，内核可以
//! 只收半截再继续；这一行依然全绿，但落点闸在 `Write` 而不是 `Block`，note 是
//! `AtomicShort`。把它写成"短写=失败"就把绿行读成红了。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 八道闸，秩从 **1** 起算；`Gate` 只接受"前进到下一秩"。前一闸不产出事实，
//! 后一闸就没有输入，倒过来必先产生假绿。
//!
//! 1. `argv`（秩 1）——命令行与操作数。缺操作数、不认识的工具、非法格式串都在
//!    这里退，**管道一次都没碰**。它排最前是钉死：用法错永远不许被记成管道满，
//!    也永远不许落实况锚。
//! 2. `open`（秩 2）——开 FIFO/管道这一手。`O_NONBLOCK` 打开无读者 FIFO 给
//!    `ENXIO`；阻塞打开无读者 FIFO 则**卡在 open 里不返回**（不是错）。这一闸
//!    必须在引用计数闸之前：读者数为 0 的事实在这里第一次露头，而且它给的码
//!    与 `write()` 侧的 `EPIPE` 不同。
//! 3. `refcount`（秩 3）——读者/写者引用计数。`readers == 0` 才是断链档的入口。
//!    它排在 open 之后：件都没开成，谈不上"写到一半读者跑了"。
//! 4. `capacity`（秩 4）——容量画像（排队字节 ≤ 管道容量）。这一闸只裁**几何**：
//!    `buffered > capacity` 是自相矛盾的申报，直接压成谎报档，不进后续裁决。
//! 5. `space`（秩 5）——空间闸。管道类目标在这里**必须空转**（没有块可谈），
//!    只有 `RegularFile` 才在 `free_blocks == 0` 时产出 28。本卡最深的一道闸：
//!    它在"没有空间"这件事上给管道放行、给文件判死，次序放在容量之后、阻塞之前，
//!    因为阻塞档的 `room` 要先算得出，才知道这次到底有没有"空间"这回事。
//! 6. `block`（秩 6）——阻塞裁决闸。`room`、`O_NONBLOCK`、`want` 三件在这里咬合
//!    出阻塞档 / 缓塞档。它必须在 space 之后：先定性"这是流控不是空间"，再谈睡不睡。
//! 7. `write`（秩 7）——`write()` 的返回值与残写记账，以及超 `PIPE_BUF` 的非原子
//!    短写。落点闸在这里的绿行是"短过、但写完"。
//! 8. `exit`（秩 8）——退出码与信号结算闸。0 只给真绿；141 只给"没屏蔽 SIGPIPE
//!    且拿到 EPIPE"；1 给其余红与用法错。锚位（`default`）也在这里落：只有真 28
//!    的文件行才允许亮，管道行一律不许亮。
//!
//! ## 关键差异（本卡深度所在）
//!
//! * **与 F11692（du 满盘统计，段基址 `0x6d3a0`，位距 `0x10`）的边界**：F11692 是
//!   只读统计面，判"满盘上 `du` 报的数对不对"；本卡判"写端在管道里的遭遇"。
//!   同一条 `No space left on device`，F11692 不许把它当自己的现场（统计面不写
//!   字节），本卡不许把它安到管道行上。两卡不互相 import，段位由本卡原样声明。
//! * **与 F11694（ENOSPC 文案逐字，段基址 `0x6d3d0`，本卡的直接后置）的边界**：
//!   F11694 管那一串英文**本身**逐字对不对；本卡管**哪一个 errno 该出现在那一串
//!   的位置上**。本卡只钉三串的存在（`Broken pipe` / `Resource temporarily
//!   unavailable` / `No such device or address`）与"28 不许进管道行"，串的拼写
//!   细则归 F11694。
//! * **与 F11688（touch/mkdir ENOSPC，段基址 `0x6d350`）的边界**：那张卡的主角是
//!   **目录项与 inode 分配**（0 字节却红、残留件、`-p` 半截父目录）；本卡的目标
//!   连目录项都不分配——写管道满时**没有任何残留落在盘上**，残写只留在用户态。
//!   所以本卡的 `value` 维是"未交付字节数"，与 F11688 的"残留件数"同名不同义。
//! * **与 F11696（释放后重试恢复，段基址 `0x6d3f0`）的边界**：那张卡判"腾出空间
//!   再写就成"，是恢复面锚；本卡的阻塞档在形状上极像恢复（睡一觉就写完了），
//!   但**恢复是空间、阻塞是流控**：本卡不许把阻塞档写成恢复档，也不许因为
//!   "最后写完了"就给它落 28 锚。
//! * **本卡的排他职责**：① 阻塞四档归因（阻塞/缓塞/断链/无读者开），全部由几何
//!   复算、不接受申报；② `EPIPE` 侧 `SIG_IGN` 把 141 与 1 拆成两档；③ 管道行
//!   的 `free_blocks` 恒 0 与实况锚互锁；④ 超 `PIPE_BUF` 的非原子短写仍是绿行；
//!   ⑤ 把 28 安到管道上（`SpaceLie`）在物理上不可发生，出现即谎报。
//! * **谁判死谁放行**：几何自洽 + 分档由复算得出 + errno 与目标类型配对 + 退出码
//!   与信号配对 + 锚位只在真 28 文件行亮 → 放行；管道行申报正的 `free_blocks`、
//!   管道行落实况锚、无读者开写成 EPIPE、缓塞档申报零残写、断链档没屏蔽信号却退
//!   1 → 一律判死。
//!
//! 判据六维：结束阶段（gate）、errno（code）、残写字节（value）、分档与文案家族
//! （note）、ENOSPC 实况锚（default）、成败申报（estate）。
//! 判序固定：状态 → 阶段 → errno → 残写 → 注解 → 锚位。
//!
//! 语料 14 槽：8 槽申报正确（全绿 3、诚实红 4、用法 1），6 槽一槽只坏一维，
//! 依次挂 Estate/Stage/Code/Value/Note/Default，标签形如 `estate-swap`。
//! 账本 14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码，冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11693，分配表原值，不许改）。
pub const SEG_BASE: u32 = 0x6d3b0;

/// 交叉署名：F11692「du 在满盘上的统计判据」的段基址（本卡直接前置）。
/// 本卡**不 import** 那个模块的任何符号，只把段位原样声明一遍。
pub const ANCHOR_F11692_SEG: u32 = 0x6d3a0;
/// F11692 与本卡的位距（0x6d3b0 - 0x6d3a0）。
pub const ANCHOR_F11692_GAP: u32 = 0x10;

/// 交叉署名：F11694「ENOSPC 文案逐字」的段基址（本卡直接后置）。
pub const ANCHOR_F11694_SEG: u32 = 0x6d3d0;
/// F11694 与本卡的位距（0x6d3d0 - 0x6d3b0）。
pub const ANCHOR_F11694_GAP: u32 = 0x20;

/// 交叉署名：F11695「ENOSPC 与 EDQUOT 配额对照档」的段基址。
pub const ANCHOR_F11695_SEG: u32 = 0x6d3e0;
/// F11695 与本卡的位距。
pub const ANCHOR_F11695_GAP: u32 = 0x30;

/// 满盘锚：`ENOSPC`。**本卡只承认它属于文件目标**，管道行出现即谎报。
pub const ANCHOR_ENOSPC: i32 = 28;
/// 断链码：`EPIPE`——管道写端的正宗码，不是 28。
pub const ANCHOR_EPIPE: i32 = 32;
/// 缓塞码：`EAGAIN`——`O_NONBLOCK` 写满管道的码。
pub const ANCHOR_EAGAIN: i32 = 11;
/// 无读者开玛：`ENXIO`——`O_NONBLOCK | O_WRONLY` 开空 FIFO。
pub const ANCHOR_ENXIO: i32 = 6;
/// 交叉署名：`No space left on device`（本卡只在"不许进管道行"这一维上署名）。
pub const ANCHOR_ERRNO_TEXT: &str = "No space left on device";
/// 交叉署名：`Broken pipe`（本卡的正宗文案）。
pub const ANCHOR_PIPE_TEXT: &str = "Broken pipe";

/// 语料格数：14 槽 = 8 活 + 6 死。
pub const CORPUS: usize = 14;
/// 账本格数（7 统计 + 1 红档 + 6 缺陷码）。
pub const SAMPLES: usize = 14;
/// 申报正确槽数。
pub const ACCEPT: usize = 8;
/// 申报错误槽数。
pub const REJECT: usize = 6;
/// 矩阵档位数（阻塞分档四档）。
pub const TIER_COUNT: usize = 4;
/// 闸门道数。
pub const STAGE_COUNT: u32 = 8;
/// 最后一道闸的秩。
pub const STAGE_LAST_RANK: u32 = 8;
/// 统计格数（账本前 7 格）。
pub const STAT_CELLS: usize = 7;
/// 红档率标定值：注入一槽缺陷后 13/14 的千分率。
pub const RED_RATE: u32 = 929;
/// 全绿千分率。
pub const RATE_FULL: u32 = 1000;

/// Linux 管道默认容量（`F_SETPIPE_SZ` 的初值，本卡的分母）。
pub const PIPE_CAP_DEFAULT: u32 = 65_536;
/// 老式管道容量（一页，基准机上仍可见）。
pub const PIPE_CAP_LEGACY: u32 = 4096;
/// POSIX `PIPE_BUF`：≤ 此值且阻塞的写保证原子。
pub const PIPE_BUF_ATOMIC: u32 = 4096;
/// 单块读写常用的块大小（语料里的 `bs`）。
pub const BLOCK_BYTES: u32 = 512;
/// SIGPIPE 信号号。
pub const SIGPIPE_NO: u8 = 13;
/// 信号致死的状态偏移（128 + signo）。
pub const EXIT_SIG_BASE: u32 = 128;

/// `ENOSPC`：设备上已无空间可分配。
pub const ERR_ENOSPC: i32 = 28;
/// `EPIPE`：管道已断（读者全退）。
pub const ERR_EPIPE: i32 = 32;
/// `EAGAIN`：资源暂时不可用（非阻塞写满）。
pub const ERR_EAGAIN: i32 = 11;
/// `EWOULDBLOCK`：POSIX 同义别名，与 `EAGAIN` 同值。
pub const ERR_EWOULDBLOCK: i32 = 11;
/// `ENXIO`：无此设备或地址（无读者开 FIFO）。
pub const ERR_ENXIO: i32 = 6;
/// `EBADF`：文件描述符非法（写端已关）。
pub const ERR_EBADF: i32 = 9;
/// `EINTR`：调用被信号打断。
pub const ERR_EINTR: i32 = 4;
/// `EINVAL`：参数非法（对管道做 seek 等）。
pub const ERR_EINVAL: i32 = 22;
/// `ENOENT`：目标不存在。
pub const ERR_ENOENT: i32 = 2;
/// `EDQUOT`：配额耗尽——**另一个"满"，不是本卡的满**。
pub const ERR_EDQUOT: i32 = 122;
/// `ENOBUFS`：协议缓冲不足——与管道满形近的邻居码。
pub const ERR_ENOBUFS: i32 = 105;
/// `EFBIG`：文件过大——写不下时的第三种说法。
pub const ERR_EFBIG: i32 = 27;
/// 无错误。
pub const ERR_NONE: i32 = 0;

/// 本卡认得的 errno 个数。
pub const ERRNO_SET_SIZE: usize = 12;
/// 与"满"相关的 errno 个数（ENOSPC / EDQUOT / ENOBUFS 三档里的两档在本卡管辖内）。
pub const SPACE_ERRNO_COUNT: usize = 3;
/// 管道侧 errno 个数（EPIPE / EAGAIN / ENXIO）。
pub const PIPE_ERRNO_COUNT: usize = 3;

/// 成功退出码。
pub const EXIT_OK: u32 = 0;
/// 文件/IO 错退出码（coreutils 的 missing-operand 也走 1）。
pub const EXIT_FILE: u32 = 1;
/// 用法退出码（POSIX 语法错那一档）。
pub const EXIT_USAGE: u32 = 2;
/// 被 SIGPIPE 打死的状态。
pub const EXIT_SIGPIPE: u32 = 141;
/// 本卡不裁定的行（越界申报）。
pub const EXIT_UNJUDGED: u32 = 99;

/// 死管道的读者引用数（判据里"读者全退"的算式写法）。
pub const READERS_NONE: u32 = 0;
/// 单读者管道的读者引用数。
pub const READERS_ONE: u32 = 1;
/// 合法读者上界（语料里最大出现两次：`tee` 分流）。
pub const READERS_MAX: u32 = 4;

/// 选项位图：`O_NONBLOCK` 写端。
pub const FLAG_NONBLOCK: u8 = 0x01;
/// 选项位图：`SIGPIPE` 置了 `SIG_IGN`（shell 内建那一档）。
pub const FLAG_IGNPIPE: u8 = 0x02;
/// 选项位图：这一手是 `open()` 而非 `write()`。
pub const FLAG_OPEN_ONLY: u8 = 0x04;
/// 选项位图：`dd` 的 `conv=fsync`/`oflag=direct` 一类落盘要求。
pub const FLAG_SYNC_OUT: u8 = 0x08;
/// 选项位图：管道是整条流水线的末件。
pub const FLAG_LAST_OF_PIPE: u8 = 0x10;
/// 选项位图：追加打开（`oflag=append`）。
pub const FLAG_APPEND: u8 = 0x20;
/// 选项位图：单次申报量超 `PIPE_BUF`。
pub const FLAG_BIG_WANT: u8 = 0x40;
/// 选项位图：读端是交互式（慢读者，会回收）。
pub const FLAG_SLOW_READER: u8 = 0x80;

/// 本卡认得的全部选项位。
pub const FLAG_KNOWN: u8 = FLAG_NONBLOCK
    | FLAG_IGNPIPE
    | FLAG_OPEN_ONLY
    | FLAG_SYNC_OUT
    | FLAG_LAST_OF_PIPE
    | FLAG_APPEND
    | FLAG_BIG_WANT
    | FLAG_SLOW_READER;

/// 选项位数。
pub const FLAG_BIT_COUNT: usize = 8;

/// 选项位表（顺序固定，`FLAG_BITS[i] == 1 << i`）。
pub const FLAG_BITS: [u8; FLAG_BIT_COUNT] = [
    FLAG_NONBLOCK,
    FLAG_IGNPIPE,
    FLAG_OPEN_ONLY,
    FLAG_SYNC_OUT,
    FLAG_LAST_OF_PIPE,
    FLAG_APPEND,
    FLAG_BIG_WANT,
    FLAG_SLOW_READER,
];

/// 选项位的名字（逐字符）。
pub const FLAG_NAMES: [&str; FLAG_BIT_COUNT] = [
    "nonblock",
    "ignpipe",
    "open-only",
    "sync-out",
    "last-of-pipe",
    "append",
    "big-want",
    "slow-reader",
];

// ---------------------------------------------------------------------------
// 13 个错误码（前缀 PF_，值 = SEG_BASE + 0x01 .. +0x0d）
// ---------------------------------------------------------------------------

/// 阶段错位 / 闸门回退。
pub const PF_ESTAGE: u32 = SEG_BASE + 0x01;
/// errno 维错位。
pub const PF_ECODE: u32 = SEG_BASE + 0x02;
/// 残写字节维错位。
pub const PF_EVALUE: u32 = SEG_BASE + 0x03;
/// 注解（分档/文案家族）错位。
pub const PF_ENOTE: u32 = SEG_BASE + 0x04;
/// 锚位错位。
pub const PF_EDEFAULT: u32 = SEG_BASE + 0x05;
/// 容量画像矛盾（排队超容量、读者越上界、管道行填了块数）。
pub const PF_EBAND: u32 = SEG_BASE + 0x06;
/// 状态违例（关闸后再走、冻结后再登记）。
pub const PF_ESTATE: u32 = SEG_BASE + 0x07;
/// 计数失衡（报表/矩阵/账本）。
pub const PF_ECOUNT: u32 = SEG_BASE + 0x08;
/// 槽位非法。
pub const PF_ESLOT: u32 = SEG_BASE + 0x09;
/// 指纹不符。
pub const PF_ESEAL: u32 = SEG_BASE + 0x0a;
/// 无观测（槽号越界、期望表缺格）。
pub const PF_EOBS: u32 = SEG_BASE + 0x0b;
/// 矩阵不平。
pub const PF_EMATRIX: u32 = SEG_BASE + 0x0c;
/// 账本不平（格数、成败比、值链任一不合）。
pub const PF_EBAL: u32 = SEG_BASE + 0x0d;

/// 13 个错误码的表（顺序 = 常量声明顺序，`name_ok()` 逐格核对）。
pub const ALL_CODES: [u32; 13] = [
    PF_ESTAGE,
    PF_ECODE,
    PF_EVALUE,
    PF_ENOTE,
    PF_EDEFAULT,
    PF_EBAND,
    PF_ESTATE,
    PF_ECOUNT,
    PF_ESLOT,
    PF_ESEAL,
    PF_EOBS,
    PF_EMATRIX,
    PF_EBAL,
];

/// 账本操作码：语料格数。
pub const OP_CAPACITY: u8 = 0x01;
/// 账本操作码：断链/引用计数。
pub const OP_REFCOUNT: u8 = 0x02;
/// 账本操作码：缓塞裁决。
pub const OP_BLOCK: u8 = 0x03;
/// 账本操作码：开闸。
pub const OP_OPEN: u8 = 0x04;
/// 账本操作码：空间闸。
pub const OP_SPACE: u8 = 0x05;
/// 账本操作码：写返回值。
pub const OP_WRITE: u8 = 0x06;
/// 账本操作码：用法闸。
pub const OP_ARGV: u8 = 0x07;
/// 账本操作码：语料统计格。
pub const OP_CORPUS: u8 = 0x08;
/// 账本操作码：红档率格。
pub const OP_RATE: u8 = 0x09;
/// 账本操作码：缺陷码格。
pub const OP_DEFECT: u8 = 0x0a;
/// 账本操作码：冻结重放格。
pub const OP_REPLAY: u8 = 0x0b;

/// 操作码个数。
pub const OP_COUNT: usize = 11;

/// 操作码表（`enums_ok()` 核对互不重复）。
pub const OP_CODES: [u8; OP_COUNT] = [
    OP_CAPACITY,
    OP_REFCOUNT,
    OP_BLOCK,
    OP_OPEN,
    OP_SPACE,
    OP_WRITE,
    OP_ARGV,
    OP_CORPUS,
    OP_RATE,
    OP_DEFECT,
    OP_REPLAY,
];

// ---------------------------------------------------------------------------
// 指纹与文本基元
// ---------------------------------------------------------------------------

/// FNV-1a 初始偏移量。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折叠一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    let x = h ^ (b as u64);
    x.wrapping_mul(FNV_PRIME)
}

/// 折叠一个宽值（逐字节，低位先走）。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut acc = h;
    let mut v = w;
    let mut i = 0usize;
    while i < 8usize {
        acc = fold_byte(acc, (v & 0xFF) as u8);
        v >>= 8;
        i += 1;
    }
    acc
}

/// 折叠一个字符串（逐字节，含长度前缀，防"拼接同形"逃过指纹）。
fn fold_str(h: u64, s: &str) -> u64 {
    let mut acc = fold_word(h, s.len() as u64);
    for b in s.as_bytes() {
        acc = fold_byte(acc, *b);
    }
    acc
}

/// 折叠一个有符号 errno。
fn fold_i32(h: u64, v: i32) -> u64 {
    fold_word(h, v as u32 as u64)
}

/// 折叠一个布尔。
fn fold_bool(h: u64, v: bool) -> u64 {
    fold_byte(h, if v { 1u8 } else { 0u8 })
}

/// u32 → 十进制文本（不递归，逐位除 10）。
fn dec_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut digits = [0u8; 10];
    let mut w = 0usize;
    let mut n = v;
    while n > 0 && w < digits.len() {
        if let Some(slot) = digits.get_mut(w) {
            *slot = b'0' + (n % 10) as u8;
        }
        n /= 10;
        w += 1;
    }
    let mut out = String::new();
    while w > 0 {
        w -= 1;
        if let Some(b) = digits.get(w) {
            out.push(*b as char);
        }
    }
    out
}

/// u32 → 十六进制文本（`digits` 位，小写，不含 `0x`）。
fn hex_text(v: u32, digits: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let n = if digits == 0 { 8usize } else { digits };
    let mut out = String::new();
    let mut i = 0usize;
    while i < n {
        let shift = ((n - i - 1) * 4) as u32;
        let idx = ((v >> shift) & 0xF) as usize;
        match HEX.get(idx) {
            Some(b) => out.push(*b as char),
            None => out.push('0'),
        }
        i += 1;
    }
    out
}

/// u32 → 文本（对外口径，等价 `dec_text`）。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// i32（errno）→ 带符号文本。
fn i32_text(v: i32) -> String {
    if v < 0 {
        let mut s = String::from("-");
        s.push_str(dec_text((v as i64).unsigned_abs() as u32).as_str());
        s
    } else {
        dec_text(v as u32)
    }
}

/// 布尔 → `1` / `0`（账本与释义里用）。
fn bool_text(v: bool) -> &'static str {
    if v {
        "1"
    } else {
        "0"
    }
}

/// 段基址文本（`0x6d3b0` 式样，逐字钉）。
pub fn seg_text(v: u32) -> String {
    let mut s = String::from("0x");
    s.push_str(hex_text(v, 5).as_str());
    s
}

/// errno 的 `(数字, 名字, 文案)` 三元组文本。
pub fn errno_triple(code: i32) -> String {
    let mut s = i32_text(code);
    s.push('(');
    s.push_str(errno_name(code));
    s.push_str(")=");
    s.push_str(strerror(code));
    s
}

/// `strerror(3)` 在 C locale 下的逐字英文（与 F11577 的表同值）。
pub fn strerror(code: i32) -> &'static str {
    match code {
        0 => "Success",
        2 => "No such file or directory",
        4 => "Interrupted system call",
        6 => "No such device or address",
        9 => "Bad file descriptor",
        11 => "Resource temporarily unavailable",
        13 => "Permission denied",
        22 => "Invalid argument",
        27 => "File too large",
        28 => "No space left on device",
        32 => "Broken pipe",
        105 => "No buffer space available",
        122 => "Disk quota exceeded",
        _ => "Unknown error",
    }
}

/// errno 的名字（大写字母串）。
pub fn errno_name(code: i32) -> &'static str {
    match code {
        0 => "ESUCCESS",
        2 => "ENOENT",
        4 => "EINTR",
        6 => "ENXIO",
        9 => "EBADF",
        11 => "EAGAIN",
        13 => "EACCES",
        22 => "EINVAL",
        27 => "EFBIG",
        28 => "ENOSPC",
        32 => "EPIPE",
        105 => "ENOBUFS",
        122 => "EDQUOT",
        _ => "ERANGE",
    }
}

/// 本卡认得的 errno 集合。
pub const KNOWN_ERRNOS: [i32; ERRNO_SET_SIZE] = [
    ERR_NONE,
    ERR_ENOENT,
    ERR_EINTR,
    ERR_ENXIO,
    ERR_EBADF,
    ERR_EAGAIN,
    ERR_EINVAL,
    ERR_EFBIG,
    ERR_ENOSPC,
    ERR_EPIPE,
    ERR_ENOBUFS,
    ERR_EDQUOT,
];

/// 形近邻居码（"也是满/也是缓冲"但不归本卡裁的）。
pub const NEIGHBOUR_ERRNOS: [i32; 5] = [ERR_EDQUOT, ERR_ENOBUFS, ERR_EFBIG, ERR_EBADF, ERR_EINTR];

/// errno 是否在本卡认得的集合里。
pub fn errno_known(code: i32) -> bool {
    for c in KNOWN_ERRNOS.iter() {
        if *c == code {
            return true;
        }
    }
    false
}

/// 是否"满族"码（三个"满"：空间、配额、缓冲）。
pub fn errno_space_family(code: i32) -> bool {
    code == ERR_ENOSPC || code == ERR_EDQUOT || code == ERR_ENOBUFS
}

/// 是否管道侧专属码（只会出现在管道/FIFO 上）。
pub fn errno_pipe_family(code: i32) -> bool {
    code == ERR_EPIPE || code == ERR_EAGAIN || code == ERR_ENXIO
}

/// 是否本卡可能申报的码（管道侧 + 空间族 + 无错）。
pub fn errno_allowed(code: i32) -> bool {
    code == ERR_NONE || errno_pipe_family(code) || errno_space_family(code)
}

// ---------------------------------------------------------------------------
// 文案：逐字复刻上游真实报错措辞
// ---------------------------------------------------------------------------

/// GNU `dd` 的写错文案：`dd: error writing '<目标>': <strerror>`。
pub fn msg_dd_error_writing(target: &str, code: i32) -> String {
    let mut s = String::from("dd: error writing '");
    s.push_str(target);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU `dd` 的开错文案：`dd: failed to open '<路径>': <strerror>`。
pub fn msg_dd_failed_open(path: &str, code: i32) -> String {
    let mut s = String::from("dd: failed to open '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// GNU coreutils `cat` 的写错文案：`cat: write error: <strerror>`。
pub fn msg_cat_write_error(code: i32) -> String {
    let mut s = String::from("cat: write error: ");
    s.push_str(strerror(code));
    s
}

/// busybox `cat` 的写错文案（同一串措辞，跨工具一致）。
pub fn msg_busybox_cat_write_error(code: i32) -> String {
    let mut s = String::from("cat: write error: ");
    s.push_str(strerror(code));
    s
}

/// bash 内建 `printf` 的写错文案：`bash: printf: write error: <strerror>`。
/// 内建把 SIGPIPE 置了 SIG_IGN，所以这行**不会**变成 141。
pub fn msg_bash_printf_write_error(code: i32) -> String {
    let mut s = String::from("bash: printf: write error: ");
    s.push_str(strerror(code));
    s
}

/// GNU `tee` 对末件写的文案：`tee: standard output: <strerror>`。
pub fn msg_tee_standard_output(code: i32) -> String {
    let mut s = String::from("tee: standard output: ");
    s.push_str(strerror(code));
    s
}

/// GNU `tee` 缺操作数：`tee: missing file operand`（coreutils 真值走 1）。
pub fn msg_tee_missing_operand() -> String {
    String::from("tee: missing file operand")
}

/// GNU `dd` 的非法数字：`dd: invalid number '<文本>'`。
pub fn msg_dd_invalid_number(text: &str) -> String {
    let mut s = String::from("dd: invalid number '");
    s.push_str(text);
    s.push('\'');
    s
}

/// 管道行**不许**出现的那一句（本卡的反锚文案，逐字）。
pub fn msg_pipe_enospc_lie(target: &str) -> String {
    let mut s = String::from("dd: error writing '");
    s.push_str(target);
    s.push_str("': No space left on device");
    s
}

/// 断链文案的规范串（`cat: write error: Broken pipe`）。
pub fn msg_broken_pipe(tool: Writer) -> String {
    match tool {
        Writer::Cat => msg_cat_write_error(ERR_EPIPE),
        Writer::Busybox => msg_busybox_cat_write_error(ERR_EPIPE),
        Writer::BashPrintf => msg_bash_printf_write_error(ERR_EPIPE),
        Writer::Tee => msg_tee_standard_output(ERR_EPIPE),
        Writer::Dd | Writer::Yes => msg_dd_error_writing("standard output", ERR_EPIPE),
        Writer::OutOfScope => String::from("out of scope"),
    }
}

/// 缓塞文案的规范串（`dd: error writing 'standard output': Resource temporarily unavailable`）。
pub fn msg_would_block(tool: Writer) -> String {
    match tool {
        Writer::BashPrintf => msg_bash_printf_write_error(ERR_EAGAIN),
        Writer::Cat => msg_cat_write_error(ERR_EAGAIN),
        Writer::Busybox => msg_busybox_cat_write_error(ERR_EAGAIN),
        Writer::Tee => msg_tee_standard_output(ERR_EAGAIN),
        Writer::Dd | Writer::Yes => msg_dd_error_writing("standard output", ERR_EAGAIN),
        Writer::OutOfScope => String::from("out of scope"),
    }
}

/// 无读者开档文案（`dd: failed to open '<fifo>': No such device or address`）。
pub fn msg_fifo_no_reader(path: &str) -> String {
    msg_dd_failed_open(path, ERR_ENXIO)
}

/// 真 28 的文件行文案（本卡唯一允许落锚的那一句）。
pub fn msg_file_enospc(path: &str) -> String {
    msg_dd_error_writing(path, ERR_ENOSPC)
}

/// 被 SIGPIPE 打死的状态串（bash 只报数，不报文案）。
pub fn msg_job_status(code: i32, sig: u8) -> String {
    let mut s = String::from("status=");
    s.push_str(dec_text(exit_expected(code, sig)).as_str());
    s.push_str(" signal=");
    s.push_str(dec_text(sig as u32).as_str());
    s
}

// ---------------------------------------------------------------------------
// 枚举：写者 / 目标 / 文案家族 / 阻塞形态 / 阶段 / 注解 / 档位 / 缺陷
// ---------------------------------------------------------------------------

/// 谁在写这一手。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Writer {
    /// GNU `dd`（外件，默认不屏蔽 SIGPIPE）。
    Dd,
    /// GNU `cat`（外件）。
    Cat,
    /// GNU `yes`（无限输出，最典型的管道写端）。
    Yes,
    /// bash 内建 `printf`/`echo`（**SIGPIPE 被置 SIG_IGN**）。
    BashPrintf,
    /// busybox `cat`。
    Busybox,
    /// GNU `tee`（既写文件又写末件）。
    Tee,
    /// 不归本卡管辖的写者（用于"越界申报"那一档）。
    OutOfScope,
}

impl Writer {
    /// 是否本卡管辖。
    pub fn owned(&self) -> bool {
        !matches!(self, Writer::OutOfScope)
    }

    /// 短名（逐字符）。
    pub fn name(&self) -> &'static str {
        match self {
            Writer::Dd => "dd",
            Writer::Cat => "cat",
            Writer::Yes => "yes",
            Writer::BashPrintf => "printf",
            Writer::Busybox => "busybox-cat",
            Writer::Tee => "tee",
            Writer::OutOfScope => "out-of-scope",
        }
    }

    /// 是不是 shell 内建（决定 SIGPIPE 是否被吞）。
    pub fn builtin(&self) -> bool {
        matches!(self, Writer::BashPrintf)
    }

    /// 缺省文案家族。
    pub fn default_flavor(&self) -> Flavor {
        match self {
            Writer::BashPrintf => Flavor::Bash,
            Writer::Busybox => Flavor::Busybox,
            _ => Flavor::Gnu,
        }
    }

    /// 是否外件（外件才可能死于 141）。
    pub fn external(&self) -> bool {
        !self.builtin() && self.owned()
    }

    /// 全集（顺序固定）。
    pub fn all() -> [Writer; 7] {
        [
            Writer::Dd,
            Writer::Cat,
            Writer::Yes,
            Writer::BashPrintf,
            Writer::Busybox,
            Writer::Tee,
            Writer::OutOfScope,
        ]
    }
}

/// 写到哪儿去——本卡"空间维是否适用"的唯一依据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// 匿名管道（`|`）。
    Pipe,
    /// 命名管道 FIFO。
    Fifo,
    /// 真文件（唯一可能有块的目标）。
    RegularFile,
    /// `/dev/null`（永不-full 的靶子）。
    NullDevice,
}

impl Target {
    /// 是否管道类（没有块、没有残留、满=流控）。
    pub fn pipe_like(&self) -> bool {
        matches!(self, Target::Pipe | Target::Fifo)
    }

    /// 是否有"空间"这回事。
    pub fn has_space(&self) -> bool {
        matches!(self, Target::RegularFile)
    }

    /// 是否永不可能满。
    pub fn infinite(&self) -> bool {
        matches!(self, Target::NullDevice)
    }

    /// 短名。
    pub fn name(&self) -> &'static str {
        match self {
            Target::Pipe => "pipe",
            Target::Fifo => "fifo",
            Target::RegularFile => "regular-file",
            Target::NullDevice => "null",
        }
    }

    /// 全集。
    pub fn all() -> [Target; 4] {
        [
            Target::Pipe,
            Target::Fifo,
            Target::RegularFile,
            Target::NullDevice,
        ]
    }
}

/// 文案家族（同一句 strerror 前面的动词/引号式样）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flavor {
    /// GNU coreutils 式：`tool: verb '<path>': <strerror>`。
    Gnu,
    /// bash 内建式：`bash: printf: write error: <strerror>`。
    Bash,
    /// busybox 式：与 GNU 同串（跨工具一致的那一部分）。
    Busybox,
}

impl Flavor {
    /// 短名。
    pub fn name(&self) -> &'static str {
        match self {
            Flavor::Gnu => "gnu",
            Flavor::Bash => "bash",
            Flavor::Busybox => "busybox",
        }
    }

    /// 是否用单引号包住目标串（GNU 式）。
    pub fn quotes_target(&self) -> bool {
        matches!(self, Flavor::Gnu)
    }

    /// 全集。
    pub fn all() -> [Flavor; 3] {
        [Flavor::Gnu, Flavor::Bash, Flavor::Busybox]
    }
}

/// 观测到的阻塞形态（本卡的物理输入之一，必须与几何复算相符）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stall {
    /// 没卡过：一次收满。
    No,
    /// 收了一半：`0 < written < want`。
    Short,
    /// 一口没收：`room == 0` 且 `want > 0`。
    Full,
    /// 断链：读者为 0。
    Dead,
}

impl Stall {
    /// 短名。
    pub fn name(&self) -> &'static str {
        match self {
            Stall::No => "no",
            Stall::Short => "short",
            Stall::Full => "full",
            Stall::Dead => "dead",
        }
    }

    /// 是否"卡过"（卡过的行才谈得上分档）。
    pub fn stalled(&self) -> bool {
        !matches!(self, Stall::No)
    }

    /// 是否读者已尽。
    pub fn dead(&self) -> bool {
        matches!(self, Stall::Dead)
    }

    /// 全集。
    pub fn all() -> [Stall; 4] {
        [Stall::No, Stall::Short, Stall::Full, Stall::Dead]
    }
}

/// 八道闸（秩 1..8）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 秩 1：命令行与操作数。
    Argv,
    /// 秩 2：开 FIFO/管道这一手。
    Open,
    /// 秩 3：读者/写者引用计数。
    Refcount,
    /// 秩 4：容量画像（几何自洽）。
    Capacity,
    /// 秩 5：空间闸（只对真文件生效）。
    Space,
    /// 秩 6：阻塞裁决闸。
    Block,
    /// 秩 7：写返回值与残写记账。
    Write,
    /// 秩 8：退出码与信号结算闸。
    Exit,
}

impl Stage {
    /// 秩（1 起算）。
    pub fn rank(&self) -> u32 {
        match self {
            Stage::Argv => 1u32,
            Stage::Open => 2u32,
            Stage::Refcount => 3u32,
            Stage::Capacity => 4u32,
            Stage::Space => 5u32,
            Stage::Block => 6u32,
            Stage::Write => 7u32,
            Stage::Exit => 8u32,
        }
    }

    /// 反查秩。
    pub fn from_rank(r: u32) -> Option<Stage> {
        match r {
            1 => Some(Stage::Argv),
            2 => Some(Stage::Open),
            3 => Some(Stage::Refcount),
            4 => Some(Stage::Capacity),
            5 => Some(Stage::Space),
            6 => Some(Stage::Block),
            7 => Some(Stage::Write),
            8 => Some(Stage::Exit),
            _ => None,
        }
    }

    /// 短名。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Argv => "argv",
            Stage::Open => "open",
            Stage::Refcount => "refcount",
            Stage::Capacity => "capacity",
            Stage::Space => "space",
            Stage::Block => "block",
            Stage::Write => "write",
            Stage::Exit => "exit",
        }
    }

    /// 下一道闸。
    pub fn next(&self) -> Option<Stage> {
        Stage::from_rank(self.rank() + 1)
    }

    /// 是否第一道。
    pub fn is_first(&self) -> bool {
        self.rank() == 1
    }

    /// 是否末道（封印闸）。
    pub fn is_last(&self) -> bool {
        self.rank() == STAGE_LAST_RANK
    }

    /// 是否空间闸（只有这一闸产出 28）。
    pub fn space_gate(&self) -> bool {
        matches!(self, Stage::Space)
    }

    /// 是否管道侧闸（open/refcount/capacity/block/write）。
    pub fn pipe_gate(&self) -> bool {
        matches!(
            self,
            Stage::Open | Stage::Refcount | Stage::Capacity | Stage::Block | Stage::Write
        )
    }

    /// 是否可能落本卡实况锚的闸。
    pub fn anchorable(&self) -> bool {
        matches!(self, Stage::Space | Stage::Write)
    }

    /// 八道闸按序列出。
    pub fn all() -> [Stage; 8] {
        [
            Stage::Argv,
            Stage::Open,
            Stage::Refcount,
            Stage::Capacity,
            Stage::Space,
            Stage::Block,
            Stage::Write,
            Stage::Exit,
        ]
    }
}

/// 注解维：分档 + 文案家族归因（本卡的 `note`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// 用法档：命令行就没过，管道一次没碰。
    Usage,
    /// 全绿交付档（含 `/dev/null`）。
    Drained,
    /// **阻塞档**（J1 的正解锚）：睡过、写完、一个错都没有。
    Blocked,
    /// 超 `PIPE_BUF` 的非原子短写档：短过，但最终全绿。
    AtomicShort,
    /// 缓塞档：`O_NONBLOCK` 撞上满管道，回 11。
    WouldBlock,
    /// 断链档（内建侧）：拿到 32 但 SIGPIPE 被吞，进程活着退 1。
    BrokenPipe,
    /// 断链档（外件侧）：32 + 13 号信号，状态 141。
    SigpipeDeath,
    /// 无读者开档：`O_NONBLOCK|O_WRONLY` 开空 FIFO，回 6。
    OpenNoReader,
    /// 谎报档：把管道满写成 28（物理上不可发生）。
    SpaceLie,
    /// 真空间档：目标文件 `free_blocks == 0`，28，允许落锚。
    FileSpace,
    /// 越界档：不归本卡管辖。
    OutOfScope,
}

impl Note {
    /// 短名（进指纹与释义）。
    pub fn name(&self) -> &'static str {
        match self {
            Note::Usage => "usage",
            Note::Drained => "drained",
            Note::Blocked => "block-band",
            Note::AtomicShort => "atomic-short",
            Note::WouldBlock => "would-block",
            Note::BrokenPipe => "broken-pipe",
            Note::SigpipeDeath => "sigpipe-death",
            Note::OpenNoReader => "open-no-reader",
            Note::SpaceLie => "space-lie",
            Note::FileSpace => "file-space",
            Note::OutOfScope => "out-of-scope",
        }
    }

    /// 是否用法档（未碰管道）。
    pub fn usage(&self) -> bool {
        matches!(self, Note::Usage | Note::OutOfScope)
    }

    /// 是否"一次都没碰管道"（用法档与越界档）。
    pub fn untouched(&self) -> bool {
        matches!(self, Note::Usage | Note::OutOfScope)
    }

    /// 是否绿档（阻塞档/非原子短写/全绿交付）。
    pub fn green_band(&self) -> bool {
        matches!(self, Note::Drained | Note::Blocked | Note::AtomicShort)
    }

    /// 是否管道侧分档。
    pub fn pipe_band(&self) -> bool {
        matches!(
            self,
            Note::Blocked
                | Note::AtomicShort
                | Note::WouldBlock
                | Note::BrokenPipe
                | Note::SigpipeDeath
                | Note::OpenNoReader
        )
    }

    /// 是否断链族（32）。
    pub fn broken(&self) -> bool {
        matches!(self, Note::BrokenPipe | Note::SigpipeDeath)
    }

    /// 是否空间族注解（真 28 与谎报 28）。
    pub fn space_family(&self) -> bool {
        matches!(self, Note::FileSpace | Note::SpaceLie)
    }

    /// 是否谎报注解（结构上不可能出现的申报）。
    pub fn lie(&self) -> bool {
        matches!(self, Note::SpaceLie)
    }

    /// 是否允许落本卡实况锚的注解（只有真 28 文件档）。
    pub fn anchorable(&self) -> bool {
        matches!(self, Note::FileSpace)
    }

    /// 该注解对应的 errno 真值（绿档为 0）。
    pub fn errno(&self) -> i32 {
        match self {
            Note::Usage | Note::Drained | Note::Blocked | Note::AtomicShort => ERR_NONE,
            Note::WouldBlock => ERR_EAGAIN,
            Note::BrokenPipe | Note::SigpipeDeath => ERR_EPIPE,
            Note::OpenNoReader => ERR_ENXIO,
            Note::SpaceLie | Note::FileSpace => ERR_ENOSPC,
            Note::OutOfScope => ERR_EINVAL,
        }
    }

    /// 全集（顺序固定）。
    pub fn all() -> [Note; 11] {
        [
            Note::Usage,
            Note::Drained,
            Note::Blocked,
            Note::AtomicShort,
            Note::WouldBlock,
            Note::BrokenPipe,
            Note::SigpipeDeath,
            Note::OpenNoReader,
            Note::SpaceLie,
            Note::FileSpace,
            Note::OutOfScope,
        ]
    }
}

/// 四档矩阵（本卡的分档筐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 一档：全绿交付（含睡过之后写完的阻塞档）。
    Drained,
    /// 二档：诚实红（缓塞/断链/无读者开/真 28），文案与几何自洽。
    Stalled,
    /// 三档：未碰管道（用法错、越界申报——不裁定）。
    Unpiped,
    /// 四档：谎报（几何矛盾或把管道满写成 28）。
    Falsified,
}

impl Tier {
    /// 按 outcome 归档：先拒谎报，再分未碰管道/全绿/诚实红。
    pub fn of(o: &Outcome) -> Tier {
        if !o.consistent {
            return Tier::Falsified;
        }
        if o.note.untouched() {
            return Tier::Unpiped;
        }
        if o.ok() {
            return Tier::Drained;
        }
        Tier::Stalled
    }

    /// 档位序号（0..3）。
    pub fn index(&self) -> usize {
        match self {
            Tier::Drained => 0usize,
            Tier::Stalled => 1usize,
            Tier::Unpiped => 2usize,
            Tier::Falsified => 3usize,
        }
    }

    /// 反查档位。
    pub fn from_index(i: usize) -> Option<Tier> {
        match i {
            0 => Some(Tier::Drained),
            1 => Some(Tier::Stalled),
            2 => Some(Tier::Unpiped),
            3 => Some(Tier::Falsified),
            _ => None,
        }
    }

    /// 档位名。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Drained => "drained",
            Tier::Stalled => "stalled",
            Tier::Unpiped => "unpiped",
            Tier::Falsified => "falsified",
        }
    }

    /// 是否成功侧。
    pub fn success(&self) -> bool {
        matches!(self, Tier::Drained)
    }

    /// 是否允许落本卡实况锚（只有诚实红一档、且注解是真文件空间档）。
    pub fn anchor_side(&self) -> bool {
        matches!(self, Tier::Stalled)
    }

    /// 谎报档是不是"必须判死"的档。
    pub fn is_falsified(&self) -> bool {
        matches!(self, Tier::Falsified)
    }

    /// 四档按序号列出。
    pub fn all() -> [Tier; TIER_COUNT] {
        [Tier::Drained, Tier::Stalled, Tier::Unpiped, Tier::Falsified]
    }
}

/// 判序六维 + 结构性附加维（`judge` 的返回类型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 六维全对。
    Ok,
    /// 成败互报（申报的绿/红与实际不一致）。
    Estate,
    /// 阶段错位（把阻塞裁决说成空间裁决等）。
    Stage,
    /// errno 错位（32 与 28 互换、11 与 6 互换）。
    Code,
    /// 残写字节错位（尾巴字节数报没报、报多少）。
    Value,
    /// 注解错位（四档归因混认）。
    Note,
    /// 锚位错位（管道行落 28 锚、真 28 不落锚）。
    Default,
    /// 容量画像矛盾（几何不自洽）。
    Band,
    /// 状态违例（关闸后再走、冻结后再登记）。
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
            Fault::Band,
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
            Fault::Estate => PF_ESTATE,
            Fault::Stage => PF_ESTAGE,
            Fault::Code => PF_ECODE,
            Fault::Value => PF_EVALUE,
            Fault::Note => PF_ENOTE,
            Fault::Default => PF_EDEFAULT,
            Fault::Band => PF_EBAND,
            Fault::State => PF_ESTATE,
            Fault::Count => PF_ECOUNT,
            Fault::Slot => PF_ESLOT,
            Fault::Seal => PF_ESEAL,
            Fault::Nob => PF_EOBS,
            Fault::Matrix => PF_EMATRIX,
        }
    }

    /// 是否"绿"。
    pub fn alive(&self) -> bool {
        matches!(self, Fault::Ok)
    }

    /// 是否本卡裁定过（`Ok` 或六缺陷之一）。
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
        Fault::Band => "band",
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

/// 期望表用的零号标记（活格）。
pub const WANT_LIVE: u8 = 0;
/// 死槽号基数（`WANT_NDX` 里 1..=6 对应六维）。
pub const WANT_DEAD_BASE: u8 = 1;

// ---------------------------------------------------------------------------
// 容量画像（本卡的"物理证据"：四档全靠这几格分开）
// ---------------------------------------------------------------------------

/// 一次判定当时的管道/盘面目击。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Capacity {
    /// 管道里已排队的字节（`F_GETPIPE_SZ` 语义下的占用）。
    pub buffered: u32,
    /// 管道容量（65536 或老式 4096；文件目标填块数换算的等价量）。
    pub capacity: u32,
    /// 读者引用计数（0 → 断链/无读者开）。
    pub readers: u32,
    /// 写者引用计数（0 → 读端会看到 EOF）。
    pub writers: u32,
    /// 目标文件可用块（**管道类目标必须为 0**：没有块可谈）。
    pub free_blocks: u32,
}

impl Capacity {
    /// 默认画像：一条开着、有读者、空的默认容量管道。
    pub const fn blank() -> Capacity {
        Capacity {
            buffered: 0u32,
            capacity: PIPE_CAP_DEFAULT,
            readers: READERS_ONE,
            writers: 1u32,
            free_blocks: 0u32,
        }
    }

    /// 满管道画像（阻塞/缓塞两档的分母）。
    pub const fn full_pipe() -> Capacity {
        Capacity {
            buffered: PIPE_CAP_DEFAULT,
            capacity: PIPE_CAP_DEFAULT,
            readers: READERS_ONE,
            writers: 1u32,
            free_blocks: 0u32,
        }
    }

    /// 无读者画像（断链档）。
    pub const fn dead_pipe() -> Capacity {
        Capacity {
            buffered: 0u32,
            capacity: PIPE_CAP_DEFAULT,
            readers: READERS_NONE,
            writers: 1u32,
            free_blocks: 0u32,
        }
    }

    /// 满盘文件画像（真 28 档）。
    pub const fn full_file() -> Capacity {
        Capacity {
            buffered: 0u32,
            capacity: PIPE_CAP_DEFAULT,
            readers: READERS_NONE,
            writers: 1u32,
            free_blocks: 0u32,
        }
    }

    /// 剩余可收字节（ saturating，防止 buffered > capacity 时下溢）。
    pub fn room(&self) -> u32 {
        self.capacity.saturating_sub(self.buffered)
    }

    /// 结构性合法：排队 ≤ 容量、读者 ≤ 上界、写者 ≥ 1、容量非零。
    pub fn well_formed(&self) -> bool {
        if self.capacity == 0u32 {
            return false;
        }
        if self.buffered > self.capacity {
            return false;
        }
        if self.readers > READERS_MAX {
            return false;
        }
        if self.writers == 0u32 {
            return false;
        }
        true
    }

    /// 目标类型与空间维是否互锁：管道类目标的 `free_blocks` 必须为 0。
    pub fn target_agrees(&self, t: Target) -> bool {
        if t.pipe_like() || t.infinite() {
            return self.free_blocks == 0u32;
        }
        true
    }

    /// 画像指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.buffered as u64);
        h = fold_word(h, self.capacity as u64);
        h = fold_word(h, self.readers as u64);
        h = fold_word(h, self.writers as u64);
        fold_word(h, self.free_blocks as u64)
    }

    /// 画像文本（`buffered/capacity` 式样）。
    pub fn text(&self) -> String {
        let mut s = dec_text(self.buffered);
        s.push('/');
        s.push_str(dec_text(self.capacity).as_str());
        s.push_str(" r");
        s.push_str(dec_text(self.readers).as_str());
        s.push_str(" w");
        s.push_str(dec_text(self.writers).as_str());
        s.push_str(" blk");
        s.push_str(dec_text(self.free_blocks).as_str());
        s
    }
}

// ---------------------------------------------------------------------------
// 形状 / 结论 / 申报 / 语料格
// ---------------------------------------------------------------------------

/// 一次管道写的全部输入维度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// 谁在写。
    pub writer: Writer,
    /// 写到哪儿。
    pub target: Target,
    /// 文案家族。
    pub flavor: Flavor,
    /// 文案里的目标串（语料全部用静态串，逐字进文案）。
    pub path: &'static str,
    /// 当时面貌。
    pub cap: Capacity,
    /// 那一手实际看到的 errno。
    pub errno_seen: i32,
    /// 实际送达的信号号（0 = 没有）。
    pub sig_seen: u8,
    /// 实际观测到的阻塞形态。
    pub stall: Stall,
    /// 选项位图。
    pub flags: u8,
    /// 操作数个数（0 = 用法错）。
    pub operand_count: u32,
    /// 命令行本身合不合法。
    pub argv_legal: bool,
    /// 本次申报要写的字节数（`want`）。
    pub want: u32,
    /// 本次返回的字节数（`written`）。
    pub written: u32,
    /// 申报的退出码。
    pub exit_declared: u32,
    /// 是否已把呈现面移交给后置卡。
    pub hand_off: bool,
}

impl Shape {
    /// 空形状：一条空管道上的一次全绿写，供 `const` 组装基型用。
    pub const fn blank() -> Shape {
        Shape {
            writer: Writer::Dd,
            target: Target::Pipe,
            flavor: Flavor::Gnu,
            path: "standard output",
            cap: Capacity::blank(),
            errno_seen: ERR_NONE,
            sig_seen: 0u8,
            stall: Stall::No,
            flags: 0u8,
            operand_count: 1u32,
            argv_legal: true,
            want: 0u32,
            written: 0u32,
            exit_declared: EXIT_OK,
            hand_off: false,
        }
    }

    /// 取某位选项。
    pub const fn flag(&self, bit: u8) -> bool {
        self.flags & bit != 0
    }

    /// 未知选项位。
    pub const fn unknown_flags(&self) -> u8 {
        self.flags & !FLAG_KNOWN
    }

    /// 是否非阻塞写端。
    pub const fn nonblock(&self) -> bool {
        self.flag(FLAG_NONBLOCK)
    }

    /// 是否屏蔽了 SIGPIPE。
    pub const fn ignores_pipe(&self) -> bool {
        self.flag(FLAG_IGNPIPE)
    }

    /// 这一手是不是纯 `open()`。
    pub const fn open_only(&self) -> bool {
        self.flag(FLAG_OPEN_ONLY)
    }

    /// 申报量是否超 `PIPE_BUF`（原子性闸）。
    pub const fn big_want(&self) -> bool {
        self.want > PIPE_BUF_ATOMIC
    }

    /// 用法是否坏。
    pub fn usage_bad(&self) -> bool {
        if !self.argv_legal {
            return true;
        }
        if self.operand_count == 0u32 {
            return true;
        }
        if !self.writer.owned() {
            return true;
        }
        if self.unknown_flags() != 0u8 {
            return true;
        }
        false
    }

    /// 残写字节（物理量，与 `written` 一致才算自洽）。
    pub fn leftover(&self) -> u32 {
        self.want.saturating_sub(self.written)
    }

    /// `written` 是否不超过 `want`。
    pub fn written_bound(&self) -> bool {
        self.written <= self.want
    }

    /// 几何复算出的阻塞形态（申报的 `stall` 必须与它相同）。
    pub fn stall_recompute(&self) -> Stall {
        if self.usage_bad() {
            return Stall::No;
        }
        if self.target.pipe_like() && self.cap.readers == READERS_NONE {
            return Stall::Dead;
        }
        if !self.target.pipe_like() {
            if self.leftover() > 0u32 {
                return Stall::Short;
            }
            return Stall::No;
        }
        let room = self.cap.room();
        if self.want > room {
            if room == 0u32 {
                return Stall::Full;
            }
            return Stall::Short;
        }
        Stall::No
    }

    /// 期望退出码（本卡不自造码：0 给绿、141 给"没屏蔽信号的死"、1 给其余）。
    pub fn exit_expected(&self, o: &Outcome) -> u32 {
        exit_for(o.code, self.sig_seen, o.note.usage())
    }

    /// 形状是否落在本卡合法域内（errno 认得、目标合法、位图自洽）。
    pub fn legal(&self) -> bool {
        if !errno_known(self.errno_seen) {
            return false;
        }
        if !self.cap.well_formed() {
            return false;
        }
        if !self.target.target_ok(self.cap) {
            return false;
        }
        if !self.written_bound() {
            return false;
        }
        if self.sig_seen != 0u8 && self.sig_seen != SIGPIPE_NO {
            return false;
        }
        if self.flavor == Flavor::Bash && !self.writer.builtin() {
            return false;
        }
        true
    }

    /// 形状指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.writer.name());
        h = fold_str(h, self.target.name());
        h = fold_str(h, self.flavor.name());
        h = fold_str(h, self.path);
        h = fold_word(h, self.cap.seal());
        h = fold_i32(h, self.errno_seen);
        h = fold_word(h, self.sig_seen as u64);
        h = fold_str(h, self.stall.name());
        h = fold_word(h, self.flags as u64);
        h = fold_word(h, self.operand_count as u64);
        h = fold_bool(h, self.argv_legal);
        h = fold_word(h, self.want as u64);
        h = fold_word(h, self.written as u64);
        h = fold_word(h, self.exit_declared as u64);
        fold_bool(h, self.hand_off)
    }

    /// 内建却申报信号致死？（内建吞了 SIGPIPE，不可能 141）
    pub fn builtin_death_lie(&self) -> bool {
        self.writer.builtin() && self.sig_seen == SIGPIPE_NO
    }

    /// 管道行填了正的块数？（本卡最深的一根刺）
    pub fn pipe_block_lie(&self) -> bool {
        self.target.pipe_like() && self.cap.free_blocks != 0u32
    }

    /// 管道行申报 28？（J1 的直接反例）
    pub fn pipe_enospc_lie(&self) -> bool {
        self.target.pipe_like() && self.errno_seen == ERR_ENOSPC
    }

    /// 十六个几何/申报量的合取式：结构自洽。
    pub fn structure_ok(&self) -> bool {
        if !self.legal() {
            return false;
        }
        if self.pipe_block_lie() {
            return false;
        }
        if self.builtin_death_lie() {
            return false;
        }
        if self.stall != self.stall_recompute() {
            return false;
        }
        true
    }
}

impl Target {
    /// 目标与画像是否合（`Shape::legal` 的实现件）。
    pub fn target_ok(self, c: &Capacity) -> bool {
        c.target_agrees(self)
    }

    /// 该目标是否可能吃到 28。
    pub fn can_enospc(self) -> bool {
        self.has_space()
    }

    /// 该目标是否可能吃到 EPIPE。
    pub fn can_epipe(self) -> bool {
        self.pipe_like()
    }

    /// 该目标是否可能吃到 ENXIO（只有 FIFO 的开手）。
    pub fn can_enxio(self) -> bool {
        matches!(self, Target::Fifo)
    }
}

/// 一次物理复算的结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 落点闸。
    pub stage: Stage,
    /// errno（0 表示这一手绿）。
    pub code: i32,
    /// 残写字节（绿格恒为 0）。
    pub value: u32,
    /// 分档注解。
    pub note: Note,
    /// 本卡实况锚：只有真 28 文件行才允许亮。
    pub default: bool,
    /// 结构自洽：几何矛盾、管道满写成 28 等都把它压成 `false`。
    pub consistent: bool,
}

impl Outcome {
    /// 全零初值。
    pub const fn blank() -> Outcome {
        Outcome {
            stage: Stage::Exit,
            code: ERR_NONE,
            value: 0u32,
            note: Note::Drained,
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

    /// 应然退出码。
    pub fn exit(&self, sig: u8) -> u32 {
        exit_for(self.code, sig, self.note.usage())
    }

    /// 该不该落锚（复算一遍，供 `default` 自检）：真 28、空间闸或写闸落点、
    /// 结构自洽、注解是真文件空间档。
    pub fn should_anchor(&self) -> bool {
        if !self.consistent {
            return false;
        }
        if self.code != ERR_ENOSPC {
            return false;
        }
        if !self.note.anchorable() {
            return false;
        }
        self.stage.anchorable()
    }

    /// 落点闸是否属于管道侧。
    pub fn on_pipe_side(&self) -> bool {
        self.stage.pipe_gate() || self.stage == Stage::Exit || self.stage == Stage::Argv
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
    /// 申报的残写字节。
    pub value: u32,
    /// 申报的分档注解。
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
            note: Note::Drained,
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

    /// 申报是否自称落了锚。
    pub fn anchored(&self) -> bool {
        self.default
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
    /// 短名（`pf11693-<slug>`，标签必须是本卡的）。
    pub tag: &'static str,
}

impl Case {
    /// 空语料格。
    pub const fn blank() -> Case {
        Case {
            shape: Shape::blank(),
            claim: Claim::blank(),
            tag: "pf11693-blank",
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

    /// 档位。
    pub fn tier(&self) -> Tier {
        Tier::of(&self.actual())
    }

    /// 语料格指纹。
    pub fn seal(&self) -> u64 {
        let mut h = self.shape.seal();
        h = fold_word(h, self.claim.seal());
        fold_str(h, self.tag)
    }
}

/// 退出码复算：用法 → 1（coreutils 的 missing-operand 走 `error(1)`），
/// 绿 → 0，未屏蔽信号的断链 → 141，其余红 → 1。
fn exit_for(code: i32, sig: u8, usage: bool) -> u32 {
    if usage {
        return EXIT_FILE;
    }
    if code == ERR_NONE {
        return EXIT_OK;
    }
    if code == ERR_EPIPE && sig == SIGPIPE_NO {
        return EXIT_SIGPIPE;
    }
    EXIT_FILE
}

/// 自由函数版应然退出码（状态串渲染用；不带用法维，与判定器同口径）。
pub fn exit_expected(code: i32, sig: u8) -> u32 {
    exit_for(code, sig, false)
}

/// 退出码文本。
pub fn exit_text(v: u32) -> String {
    dec_text(v)
}

// ---------------------------------------------------------------------------
// 物理复算：闸门纵线（define）
// ---------------------------------------------------------------------------

/// 只读几何复算：从形状算出"这一手**应该**落在哪道闸、该看到什么 errno、
/// 该留多少残写、该归哪一档"。不看申报，因此谎报改变不了它。
fn physics(s: &Shape) -> (Stage, i32, u32, Note) {
    // 闸 1：argv。
    if s.usage_bad() {
        if !s.writer.owned() {
            return (Stage::Argv, ERR_EINVAL, 0u32, Note::OutOfScope);
        }
        return (Stage::Argv, ERR_NONE, 0u32, Note::Usage);
    }
    // 闸 4 提前：几何本身矛盾就到此为止（容量画像谎报不配进后续裁决）。
    if !s.cap.well_formed() {
        return (Stage::Capacity, ERR_EINVAL, 0u32, Note::OutOfScope);
    }
    // 闸 2：open。
    if s.open_only() {
        if s.target.can_enxio() && s.cap.readers == READERS_NONE {
            if s.nonblock() {
                return (Stage::Open, ERR_ENXIO, s.want, Note::OpenNoReader);
            }
            // 阻塞打开无读者：这一手不返回，账上无可观测事实，按绿处理，
            // 但要求申报 `want == written`（否则就是"没返回却报了残写"）。
            return (Stage::Open, ERR_NONE, 0u32, Note::Blocked);
        }
        return (Stage::Open, ERR_NONE, 0u32, Note::Blocked);
    }
    // 闸 3：引用计数。读者为 0 → 断链族。
    if s.target.pipe_like() && s.cap.readers == READERS_NONE {
        let n = if s.ignores_pipe() || !s.writer.external() {
            Note::BrokenPipe
        } else {
            Note::SigpipeDeath
        };
        return (Stage::Refcount, ERR_EPIPE, s.leftover(), n);
    }
    // `/dev/null`：永不-full。
    if s.target.infinite() {
        return (Stage::Exit, ERR_NONE, 0u32, Note::Drained);
    }
    // 闸 5：空间（只对真文件生效）。
    if s.target == Target::RegularFile {
        if s.cap.free_blocks == 0u32 {
            return (Stage::Space, ERR_ENOSPC, s.leftover(), Note::FileSpace);
        }
        return (Stage::Write, ERR_NONE, 0u32, Note::Drained);
    }
    // 闸 6/7：阻塞裁决与写返回值。
    let room = s.cap.room();
    if s.want <= room {
        return (Stage::Write, ERR_NONE, 0u32, Note::Drained);
    }
    if s.nonblock() {
        // 内核必收满 `room`，尾巴留在用户态：残写是复算量，不是观测量。
        return (Stage::Block, ERR_EAGAIN, s.want - room, Note::WouldBlock);
    }
    if s.want > PIPE_BUF_ATOMIC {
        return (Stage::Write, ERR_NONE, 0u32, Note::AtomicShort);
    }
    (Stage::Block, ERR_NONE, 0u32, Note::Blocked)
}

/// 断链族里"应不应该收到 13 号信号"：外部命令且没屏蔽 SIGPIPE 才会死。
fn expected_sig(s: &Shape, code: i32) -> u8 {
    if code == ERR_EPIPE && s.writer.external() && !s.ignores_pipe() {
        return SIGPIPE_NO;
    }
    0u8
}

/// **闸门纵线**：形状 → 结论。申报不许参与，因此这一函数就是本卡的"基准机"。
pub fn define(s: &Shape) -> Outcome {
    // J1 的反例先走：管道行申报 28，物理上不可能，直接谎报档。
    if s.pipe_enospc_lie() {
        return Outcome::make(
            Stage::Space,
            s.errno_seen,
            s.leftover(),
            Note::SpaceLie,
            false,
            false,
        );
    }
    let (st, code, val, note) = physics(s);
    let sig = expected_sig(s, code);
    let mut consistent = s.structure_ok()
        && s.errno_seen == code
        && s.sig_seen == sig
        && s.leftover() == val
        && s.exit_declared == exit_for(code, sig, note.usage());
    // 绿行不许有残写（`want == written`），否则"写完了"与"还剩 N 字节"自相矛盾。
    if code == ERR_NONE && val != 0u32 {
        consistent = false;
    }
    let mut o = Outcome::make(st, code, val, note, false, consistent);
    o.default = o.should_anchor();
    o
}

/// `define` 的 Option 版（越界/非法形状给 `None`，调用方落 `PF_EOBS`）。
pub fn define_or_none(s: &Shape) -> Option<Outcome> {
    if s.target == Target::Pipe && s.cap.capacity == 0u32 {
        return None;
    }
    Some(define(s))
}

/// 段位交叉署名是否自洽（三个前置/后置卡的位距复算）。
pub fn anchors_agree() -> bool {
    if SEG_BASE.saturating_sub(ANCHOR_F11692_SEG) != ANCHOR_F11692_GAP {
        return false;
    }
    if ANCHOR_F11694_SEG.saturating_sub(SEG_BASE) != ANCHOR_F11694_GAP {
        return false;
    }
    if ANCHOR_F11695_SEG.saturating_sub(SEG_BASE) != ANCHOR_F11695_GAP {
        return false;
    }
    if ANCHOR_F11694_SEG <= ANCHOR_F11692_SEG {
        return false;
    }
    true
}

/// 本卡的整行文案（绿行**一个字节都不打**——这也是 J1 的一部分）。
pub fn message(s: &Shape, o: &Outcome) -> String {
    if o.consistent && o.code == ERR_NONE {
        return String::new();
    }
    match o.note {
        Note::Usage => match s.writer {
            Writer::Tee => msg_tee_missing_operand(),
            Writer::Dd | Writer::Yes | Writer::Cat | Writer::Busybox | Writer::BashPrintf => {
                msg_dd_invalid_number(s.path)
            }
            Writer::OutOfScope => String::from("out of scope"),
        },
        Note::OutOfScope => String::from("out of scope"),
        Note::OpenNoReader => msg_fifo_no_reader(s.path),
        Note::BrokenPipe | Note::SigpipeDeath => match s.flavor {
            Flavor::Bash => msg_bash_printf_write_error(ERR_EPIPE),
            Flavor::Busybox => msg_busybox_cat_write_error(ERR_EPIPE),
            Flavor::Gnu => match s.writer {
                Writer::Cat => msg_cat_write_error(ERR_EPIPE),
                Writer::Tee => msg_tee_standard_output(ERR_EPIPE),
                _ => msg_dd_error_writing(s.path, ERR_EPIPE),
            },
        },
        Note::WouldBlock => msg_would_block(s.writer),
        Note::FileSpace => msg_file_enospc(s.path),
        Note::SpaceLie => msg_pipe_enospc_lie(s.path),
        Note::Drained | Note::Blocked | Note::AtomicShort => {
            if o.consistent {
                String::new()
            } else {
                msg_dd_error_writing(s.path, o.code)
            }
        }
    }
}

/// 一格的释义行（内部诊断文本，不是上游文案）。
pub fn detail(c: &Case) -> String {
    let a = c.actual();
    let mut s = String::from(c.tag);
    s.push_str(" stage=");
    s.push_str(a.stage.name());
    s.push('(');
    s.push_str(dec_text(a.stage.rank()).as_str());
    s.push(')');
    s.push_str(" errno=");
    s.push_str(errno_triple(a.code).as_str());
    s.push_str(" left=");
    s.push_str(dec_text(a.value).as_str());
    s.push_str(" band=");
    s.push_str(a.note.name());
    s.push_str(" anchor=");
    s.push_str(bool_text(a.default));
    s.push_str(" geom=");
    s.push_str(c.shape.cap.text().as_str());
    s.push_str(" want=");
    s.push_str(dec_text(c.shape.want).as_str());
    s.push_str(" got=");
    s.push_str(dec_text(c.shape.written).as_str());
    s.push_str(" sig=");
    s.push_str(dec_text(c.shape.sig_seen as u32).as_str());
    s.push_str(" exit=");
    s.push_str(dec_text(c.shape.exit_declared).as_str());
    s.push_str(" ok=");
    s.push_str(bool_text(c.ok()));
    s
}

/// 一格的表行（语料导出，制表符分隔，逐字符）。
pub fn render_line(c: &Case) -> String {
    let a = c.actual();
    let mut s = String::from(c.tag);
    s.push('\t');
    s.push_str(c.shape.writer.name());
    s.push('\t');
    s.push_str(c.shape.target.name());
    s.push('\t');
    s.push_str(a.stage.name());
    s.push('\t');
    s.push_str(errno_name(a.code));
    s.push('\t');
    s.push_str(dec_text(a.value).as_str());
    s.push('\t');
    s.push_str(a.note.name());
    s.push('\t');
    s.push_str(bool_text(a.default));
    s.push('\t');
    s.push_str(fault_short(judge(c)));
    s
}

// ---------------------------------------------------------------------------
// 语料：14 槽 = 8 活 + 6 死
// ---------------------------------------------------------------------------

/// 形状基型：一条**写满的**阻塞管道上的一次 4096 字节写，最终全写完。
/// 这就是 J1 的正解现场，故 0 号槽与它逐格相同。
const BASE: Shape = Shape {
    writer: Writer::Dd,
    target: Target::Pipe,
    flavor: Flavor::Gnu,
    path: "standard output",
    cap: Capacity {
        buffered: PIPE_CAP_DEFAULT,
        capacity: PIPE_CAP_DEFAULT,
        readers: READERS_ONE,
        writers: 1u32,
        free_blocks: 0u32,
    },
    errno_seen: ERR_NONE,
    sig_seen: 0u8,
    stall: Stall::Full,
    flags: 0u8,
    operand_count: 1u32,
    argv_legal: true,
    want: PIPE_BUF_ATOMIC,
    written: PIPE_BUF_ATOMIC,
    exit_declared: EXIT_OK,
    hand_off: false,
};

/// 申报基型：与 `BASE` 复算出的实际结论逐格相同。
const BASE_CLAIM: Claim = Claim {
    stage: Stage::Block,
    code: ERR_NONE,
    value: 0u32,
    note: Note::Blocked,
    default: false,
};

/// 十四格语料形状。前八格申报正确，后六格一槽只坏一维。
pub const SHAPES: [Shape; CORPUS] = [
    // 0 J1 正解：管道满、阻塞、睡过之后 4096 字节全写完——非 ENOSPC 实况。
    Shape {
        ..BASE
    },
    // 1 读者先退：外部 `cat` 拿到 32 并被 13 号信号打死，状态 141。
    Shape {
        writer: Writer::Cat,
        cap: Capacity {
            buffered: 0u32,
            capacity: PIPE_CAP_DEFAULT,
            readers: READERS_NONE,
            writers: 1u32,
            free_blocks: 0u32,
        },
        errno_seen: ERR_EPIPE,
        sig_seen: SIGPIPE_NO,
        stall: Stall::Dead,
        flags: FLAG_BIG_WANT,
        want: 8192u32,
        written: 1024u32,
        exit_declared: EXIT_SIGPIPE,
        ..BASE
    },
    // 2 `/dev/null`：永不-full，65536 字节一次收光。
    Shape {
        target: Target::NullDevice,
        path: "/dev/null",
        cap: Capacity {
            buffered: 0u32,
            capacity: PIPE_CAP_DEFAULT,
            readers: READERS_NONE,
            writers: 1u32,
            free_blocks: 0u32,
        },
        stall: Stall::No,
        flags: FLAG_BIG_WANT,
        want: PIPE_CAP_DEFAULT,
        written: PIPE_CAP_DEFAULT,
        ..BASE
    },
    // 3 缓塞档：同一份几何但置了 O_NONBLOCK，收 5536 后回 11，尾巴 2656。
    Shape {
        cap: Capacity {
            buffered: 60_000u32,
            capacity: PIPE_CAP_DEFAULT,
            readers: READERS_ONE,
            writers: 1u32,
            free_blocks: 0u32,
        },
        errno_seen: ERR_EAGAIN,
        stall: Stall::Short,
        flags: FLAG_NONBLOCK | FLAG_BIG_WANT,
        want: 8192u32,
        written: 5536u32,
        exit_declared: EXIT_FILE,
        ..BASE
    },
    // 4 真空间档：目标是文件、可用块 0，28，本卡唯一允许落锚的一格。
    Shape {
        target: Target::RegularFile,
        path: "srv/spool/big",
        cap: Capacity {
            buffered: 0u32,
            capacity: PIPE_CAP_DEFAULT,
            readers: READERS_NONE,
            writers: 1u32,
            free_blocks: 0u32,
        },
        errno_seen: ERR_ENOSPC,
        stall: Stall::Short,
        flags: FLAG_BIG_WANT,
        want: PIPE_CAP_DEFAULT,
        written: 32_768u32,
        exit_declared: EXIT_FILE,
        ..BASE
    },
    // 5 无读者开档：O_NONBLOCK|O_WRONLY 开空 FIFO，回 6，一字节未写。
    Shape {
        target: Target::Fifo,
        path: "srv/fifo",
        cap: Capacity {
            buffered: 0u32,
            capacity: PIPE_CAP_DEFAULT,
            readers: READERS_NONE,
            writers: 1u32,
            free_blocks: 0u32,
        },
        errno_seen: ERR_ENXIO,
        stall: Stall::Dead,
        flags: FLAG_OPEN_ONLY | FLAG_NONBLOCK,
        want: PIPE_BUF_ATOMIC,
        written: 0u32,
        exit_declared: EXIT_FILE,
        ..BASE
    },
    // 6 用法档：`tee` 缺操作数，管道一次没碰，coreutils 真值退 1。
    Shape {
        writer: Writer::Tee,
        path: "-",
        cap: Capacity {
            buffered: 0u32,
            capacity: PIPE_CAP_DEFAULT,
            readers: READERS_ONE,
            writers: 1u32,
            free_blocks: 0u32,
        },
        stall: Stall::No,
        operand_count: 0u32,
        want: 0u32,
        written: 0u32,
        exit_declared: EXIT_FILE,
        ..BASE
    },
    // 7 非原子短写档：申报 65536 > PIPE_BUF，阻塞短过后仍全绿。
    Shape {
        flags: FLAG_BIG_WANT | FLAG_SLOW_READER,
        want: PIPE_CAP_DEFAULT,
        written: PIPE_CAP_DEFAULT,
        ..BASE
    },
    // 8 死槽 · Estate：缓塞档红行被申报成成功。
    Shape {
        cap: Capacity {
            buffered: 61_000u32,
            capacity: PIPE_CAP_DEFAULT,
            readers: READERS_ONE,
            writers: 1u32,
            free_blocks: 0u32,
        },
        errno_seen: ERR_EAGAIN,
        stall: Stall::Short,
        flags: FLAG_NONBLOCK | FLAG_BIG_WANT,
        want: 8192u32,
        written: 4536u32,
        exit_declared: EXIT_FILE,
        ..BASE
    },
    // 9 死槽 · Stage：阻塞档绿行被申报成"落在空间闸"。
    Shape {
        path: "standard output",
        ..BASE
    },
    // 10 死槽 · Code：断链档被申报成满盘（32 → 28）。
    Shape {
        writer: Writer::Cat,
        cap: Capacity {
            buffered: 0u32,
            capacity: PIPE_CAP_DEFAULT,
            readers: READERS_NONE,
            writers: 1u32,
            free_blocks: 0u32,
        },
        errno_seen: ERR_EPIPE,
        sig_seen: SIGPIPE_NO,
        stall: Stall::Dead,
        flags: FLAG_BIG_WANT,
        want: 8192u32,
        written: 2048u32,
        exit_declared: EXIT_SIGPIPE,
        ..BASE
    },
    // 11 死槽 · Value：缓塞档申报"零残写"。
    Shape {
        cap: Capacity {
            buffered: 62_000u32,
            capacity: PIPE_CAP_DEFAULT,
            readers: READERS_ONE,
            writers: 1u32,
            free_blocks: 0u32,
        },
        errno_seen: ERR_EAGAIN,
        stall: Stall::Short,
        flags: FLAG_NONBLOCK | FLAG_BIG_WANT,
        want: 8192u32,
        written: 3536u32,
        exit_declared: EXIT_FILE,
        ..BASE
    },
    // 12 死槽 · Note：无读者开档被读成缓塞档。
    Shape {
        target: Target::Fifo,
        path: "srv/fifo",
        cap: Capacity {
            buffered: 0u32,
            capacity: PIPE_CAP_DEFAULT,
            readers: READERS_NONE,
            writers: 1u32,
            free_blocks: 0u32,
        },
        errno_seen: ERR_ENXIO,
        stall: Stall::Dead,
        flags: FLAG_OPEN_ONLY | FLAG_NONBLOCK,
        want: PIPE_BUF_ATOMIC,
        written: 0u32,
        exit_declared: EXIT_FILE,
        ..BASE
    },
    // 13 死槽 · Default：管道满的绿行被落了 ENOSPC 实况锚（J1 的头号反例）。
    Shape {
        path: "standard output",
        ..BASE
    },
];

/// 十四格申报。活格与实际逐格相同；死格只坏一维。
pub const CLAIMS: [Claim; CORPUS] = [
    // 0
    Claim {
        ..BASE_CLAIM
    },
    // 1
    Claim {
        stage: Stage::Refcount,
        code: ERR_EPIPE,
        value: 7168u32,
        note: Note::SigpipeDeath,
        default: false,
    },
    // 2
    Claim {
        stage: Stage::Exit,
        code: ERR_NONE,
        value: 0u32,
        note: Note::Drained,
        default: false,
    },
    // 3
    Claim {
        stage: Stage::Block,
        code: ERR_EAGAIN,
        value: 2656u32,
        note: Note::WouldBlock,
        default: false,
    },
    // 4
    Claim {
        stage: Stage::Space,
        code: ERR_ENOSPC,
        value: 32_768u32,
        note: Note::FileSpace,
        default: true,
    },
    // 5
    Claim {
        stage: Stage::Open,
        code: ERR_ENXIO,
        value: 4096u32,
        note: Note::OpenNoReader,
        default: false,
    },
    // 6
    Claim {
        stage: Stage::Argv,
        code: ERR_NONE,
        value: 0u32,
        note: Note::Usage,
        default: false,
    },
    // 7
    Claim {
        stage: Stage::Write,
        code: ERR_NONE,
        value: 0u32,
        note: Note::AtomicShort,
        default: false,
    },
    // 8 Estate：申报成功（code 0），其余四维照实。
    Claim {
        stage: Stage::Block,
        code: ERR_NONE,
        value: 3656u32,
        note: Note::WouldBlock,
        default: false,
    },
    // 9 Stage：落点闸谎报成 space。
    Claim {
        stage: Stage::Space,
        code: ERR_NONE,
        value: 0u32,
        note: Note::Blocked,
        default: false,
    },
    // 10 Code：把断链写成满盘。
    Claim {
        stage: Stage::Refcount,
        code: ERR_ENOSPC,
        value: 6144u32,
        note: Note::SigpipeDeath,
        default: false,
    },
    // 11 Value：残写照零。
    Claim {
        stage: Stage::Block,
        code: ERR_EAGAIN,
        value: 0u32,
        note: Note::WouldBlock,
        default: false,
    },
    // 12 Note：无读者开档被归成缓塞档。
    Claim {
        stage: Stage::Open,
        code: ERR_ENXIO,
        value: 4096u32,
        note: Note::WouldBlock,
        default: false,
    },
    // 13 Default：绿行谎落 28 实况锚。
    Claim {
        stage: Stage::Block,
        code: ERR_NONE,
        value: 0u32,
        note: Note::Blocked,
        default: true,
    },
];

/// 十四格短名（标签必须是本卡的）。
pub const WANT_TAGS: [&str; CORPUS] = [
    "pf11693-block-band",
    "pf11693-sigpipe-death",
    "pf11693-null-drain",
    "pf11693-would-block",
    "pf11693-file-enospc",
    "pf11693-open-no-reader",
    "pf11693-missing-operand",
    "pf11693-atomic-short",
    "pf11693-estate-swap",
    "pf11693-stage-swap",
    "pf11693-code-swap",
    "pf11693-value-swap",
    "pf11693-note-swap",
    "pf11693-default-swap",
];

/// 期望表（第二形式）：**实际**落点闸。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Block,
    Stage::Refcount,
    Stage::Exit,
    Stage::Block,
    Stage::Space,
    Stage::Open,
    Stage::Argv,
    Stage::Write,
    Stage::Block,
    Stage::Block,
    Stage::Refcount,
    Stage::Block,
    Stage::Open,
    Stage::Block,
];

/// 期望表（第二形式）：**实际** errno。
pub const WANT_CODE: [i32; CORPUS] = [
    ERR_NONE,
    ERR_EPIPE,
    ERR_NONE,
    ERR_EAGAIN,
    ERR_ENOSPC,
    ERR_ENXIO,
    ERR_NONE,
    ERR_NONE,
    ERR_EAGAIN,
    ERR_NONE,
    ERR_EPIPE,
    ERR_EAGAIN,
    ERR_ENXIO,
    ERR_NONE,
];

/// 期望表（第二形式）：**实际**残写字节。
pub const WANT_VALUE: [u32; CORPUS] = [
    0,
    7168,
    0,
    2656,
    32_768,
    4096,
    0,
    0,
    3656,
    0,
    6144,
    4656,
    4096,
    0,
];

/// 期望表（第二形式）：**实际**分档注解。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::Blocked,
    Note::SigpipeDeath,
    Note::Drained,
    Note::WouldBlock,
    Note::FileSpace,
    Note::OpenNoReader,
    Note::Usage,
    Note::AtomicShort,
    Note::WouldBlock,
    Note::Blocked,
    Note::SigpipeDeath,
    Note::WouldBlock,
    Note::OpenNoReader,
    Note::Blocked,
];

/// 期望表（第二形式）：**实际**锚位。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, true, false, false, false, false, false, false, false, false,
    false,
];

/// 期望表（第二形式）：**实际**是否结构自洽。
pub const WANT_CONSISTENT: [bool; CORPUS] = [true; CORPUS];

/// 死槽号：0 = 活格；1..=6 对应 `REJECT_FAULTS` 的下标。
pub const WANT_NDX: [u8; CORPUS] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6];

/// 六维死槽的维度名（逐字符，`corpus_ok()` 用它对标签）。
pub const WANT_DIM_NAMES: [&str; REJECT] = [
    "estate", "stage", "code", "value", "note", "default",
];

/// 活格槽号表。
const ACCEPT_SLOTS: [usize; ACCEPT] = [0usize, 1, 2, 3, 4, 5, 6, 7];

/// 死格槽号表。
const REJECT_SLOTS: [usize; REJECT] = [8usize, 9, 10, 11, 12, 13];

/// 申报表（第二形式）：申报的落点闸。与 `CLAIMS` 交叉核对。
pub const ACT_STAGE: [Stage; CORPUS] = [
    Stage::Block,
    Stage::Refcount,
    Stage::Exit,
    Stage::Block,
    Stage::Space,
    Stage::Open,
    Stage::Argv,
    Stage::Write,
    Stage::Block,
    Stage::Space,
    Stage::Refcount,
    Stage::Block,
    Stage::Open,
    Stage::Block,
];

/// 申报表（第二形式）：申报的 errno。
pub const ACT_CODE: [i32; CORPUS] = [
    ERR_NONE,
    ERR_EPIPE,
    ERR_NONE,
    ERR_EAGAIN,
    ERR_ENOSPC,
    ERR_ENXIO,
    ERR_NONE,
    ERR_NONE,
    ERR_NONE,
    ERR_NONE,
    ERR_ENOSPC,
    ERR_EAGAIN,
    ERR_ENXIO,
    ERR_NONE,
];

/// 申报表（第二形式）：申报的残写字节。
pub const ACT_VALUE: [u32; CORPUS] = [
    0,
    7168,
    0,
    2656,
    32_768,
    4096,
    0,
    0,
    3656,
    0,
    6144,
    0,
    4096,
    0,
];

/// 申报表（第二形式）：申报的分档注解。
pub const ACT_NOTE: [Note; CORPUS] = [
    Note::Blocked,
    Note::SigpipeDeath,
    Note::Drained,
    Note::WouldBlock,
    Note::FileSpace,
    Note::OpenNoReader,
    Note::Usage,
    Note::AtomicShort,
    Note::WouldBlock,
    Note::Blocked,
    Note::SigpipeDeath,
    Note::WouldBlock,
    Note::WouldBlock,
    Note::Blocked,
];

/// 申报表（第二形式）：申报的锚位。
pub const ACT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, true, false, false, false, false, false, false, false, false,
    true,
];

/// 六维死槽各坏哪一维（供 `slots_ok()` 反查）。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

/// 六维死槽的错误码（互不相同，账本逐格对账用）。
pub const REJECT_CODES: [u32; REJECT] = [
    PF_ESTATE,
    PF_ESTAGE,
    PF_ECODE,
    PF_EVALUE,
    PF_ENOTE,
    PF_EDEFAULT,
];

/// 账本值链：前 7 格是统计数，**第 8 格是红档率 929**。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 3, 4, 1, 1, 929];

/// 十四格语料。
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

/// 取第 i 格；越界给 `None`（调用方落 `PF_EOBS`）。
pub fn case_at(i: usize) -> Option<Case> {
    cases().get(i).copied()
}

/// 覆盖率检查：格数必须正好是 `CORPUS`。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        return Ok(());
    }
    Err(PF_ECOUNT)
}

/// 第 i 格期望的缺陷维；越界给 `None`。
pub fn expected(i: usize) -> Option<Fault> {
    let ndx = WANT_NDX.get(i)?;
    if *ndx == WANT_LIVE {
        return Some(Fault::Ok);
    }
    let k = (*ndx as usize) - 1;
    REJECT_FAULTS.get(k).copied()
}

/// **判序**：状态 → 阶段 → errno → 残写 → 注解 → 锚位。次序固定，不许换。
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
// 场景族：语料之外的几何/身份组合，逐格钉住四档与谎报档
// ---------------------------------------------------------------------------

/// 场景数。
pub const SCENE_COUNT: usize = 9;

/// 九个场景：内建吞信号 × 分流末件 × 大申报断链 × 两型谎报 × 有空间文件 ×
/// 非阻塞但有余量 × 几何矛盾 × 跨工具一致。
pub fn scenes() -> [Shape; SCENE_COUNT] {
    [
        // 0 shell 内建 `printf`：拿到 EPIPE 但 SIGPIPE 被 SIG_IGN 吞掉 → 退 1。
        Shape {
            writer: Writer::BashPrintf,
            flavor: Flavor::Bash,
            cap: Capacity {
                buffered: 0u32,
                capacity: PIPE_CAP_DEFAULT,
                readers: READERS_NONE,
                writers: 1u32,
                free_blocks: 0u32,
            },
            errno_seen: ERR_EPIPE,
            sig_seen: 0u8,
            stall: Stall::Dead,
            flags: FLAG_IGNPIPE,
            want: 1024u32,
            written: 0u32,
            exit_declared: EXIT_FILE,
            path: "standard output",
            ..BASE
        },
        // 1 `tee` 末件阻塞写完：与 0 号槽同档，写者不同。
        Shape {
            writer: Writer::Tee,
            path: "log.txt",
            flags: FLAG_LAST_OF_PIPE,
            ..BASE
        },
        // 2 `yes | head`：大申报量断链，残写 32768。
        Shape {
            writer: Writer::Yes,
            cap: Capacity {
                buffered: 0u32,
                capacity: PIPE_CAP_DEFAULT,
                readers: READERS_NONE,
                writers: 1u32,
                free_blocks: 0u32,
            },
            errno_seen: ERR_EPIPE,
            sig_seen: SIGPIPE_NO,
            stall: Stall::Dead,
            flags: FLAG_BIG_WANT,
            want: PIPE_CAP_DEFAULT,
            written: 32_768u32,
            exit_declared: EXIT_SIGPIPE,
            ..BASE
        },
        // 3 谎报：管道行填了正的块数（本卡的互锁闸）。
        Shape {
            cap: Capacity {
                buffered: 0u32,
                capacity: PIPE_CAP_DEFAULT,
                readers: READERS_ONE,
                writers: 1u32,
                free_blocks: 8u32,
            },
            stall: Stall::No,
            want: PIPE_BUF_ATOMIC,
            written: PIPE_BUF_ATOMIC,
            ..BASE
        },
        // 4 谎报：管道行申报 28（J1 的反例本体）。
        Shape {
            cap: Capacity {
                buffered: PIPE_CAP_DEFAULT,
                capacity: PIPE_CAP_DEFAULT,
                readers: READERS_ONE,
                writers: 1u32,
                free_blocks: 0u32,
            },
            errno_seen: ERR_ENOSPC,
            stall: Stall::Full,
            want: PIPE_BUF_ATOMIC,
            written: PIPE_BUF_ATOMIC,
            exit_declared: EXIT_FILE,
            ..BASE
        },
        // 5 真文件、有块：全绿，不落锚。
        Shape {
            target: Target::RegularFile,
            path: "srv/spool/ok",
            cap: Capacity {
                buffered: 0u32,
                capacity: PIPE_CAP_DEFAULT,
                readers: READERS_NONE,
                writers: 1u32,
                free_blocks: 4096u32,
            },
            errno_seen: ERR_NONE,
            stall: Stall::No,
            flags: FLAG_BIG_WANT,
            want: 8192u32,
            written: 8192u32,
            exit_declared: EXIT_OK,
            ..BASE
        },
        // 6 非阻塞但余量够：一次收光，全绿。
        Shape {
            cap: Capacity {
                buffered: 32_768u32,
                capacity: PIPE_CAP_DEFAULT,
                readers: READERS_ONE,
                writers: 1u32,
                free_blocks: 0u32,
            },
            errno_seen: ERR_NONE,
            stall: Stall::No,
            flags: FLAG_NONBLOCK | FLAG_BIG_WANT,
            want: 8192u32,
            written: 8192u32,
            exit_declared: EXIT_OK,
            ..BASE
        },
        // 7 几何矛盾：排队字节超过容量。
        Shape {
            cap: Capacity {
                buffered: 70_000u32,
                capacity: PIPE_CAP_DEFAULT,
                readers: READERS_ONE,
                writers: 1u32,
                free_blocks: 0u32,
            },
            errno_seen: ERR_NONE,
            stall: Stall::Full,
            want: PIPE_BUF_ATOMIC,
            written: PIPE_BUF_ATOMIC,
            exit_declared: EXIT_OK,
            ..BASE
        },
        // 8 busybox `cat` 断链：串与 GNU 逐字一致（跨工具一致）。
        Shape {
            writer: Writer::Busybox,
            flavor: Flavor::Busybox,
            cap: Capacity {
                buffered: 0u32,
                capacity: PIPE_CAP_DEFAULT,
                readers: READERS_NONE,
                writers: 1u32,
                free_blocks: 0u32,
            },
            errno_seen: ERR_EPIPE,
            sig_seen: SIGPIPE_NO,
            stall: Stall::Dead,
            want: PIPE_BUF_ATOMIC,
            written: 0u32,
            exit_declared: EXIT_SIGPIPE,
            ..BASE
        },
    ]
}

/// 场景短名（逐字符）。
pub const SCENE_TAGS: [&str; SCENE_COUNT] = [
    "pf-scene-builtin-ignpipe",
    "pf-scene-tee-last",
    "pf-scene-yes-head",
    "pf-scene-pipe-block-lie",
    "pf-scene-pipe-enospc-lie",
    "pf-scene-file-with-space",
    "pf-scene-nonblock-roomy",
    "pf-scene-over-capacity",
    "pf-scene-busybox-consistent",
];

/// 场景实际落点闸。
pub const SCENE_WANT_STAGE: [Stage; SCENE_COUNT] = [
    Stage::Refcount,
    Stage::Block,
    Stage::Refcount,
    Stage::Write,
    Stage::Space,
    Stage::Write,
    Stage::Write,
    Stage::Capacity,
    Stage::Refcount,
];

/// 场景实际 errno。
pub const SCENE_WANT_CODE: [i32; SCENE_COUNT] = [
    ERR_EPIPE,
    ERR_NONE,
    ERR_EPIPE,
    ERR_NONE,
    ERR_ENOSPC,
    ERR_NONE,
    ERR_NONE,
    ERR_EINVAL,
    ERR_EPIPE,
];

/// 场景实际残写字节。
pub const SCENE_WANT_VALUE: [u32; SCENE_COUNT] = [1024, 0, 32_768, 0, 0, 0, 0, 0, 4096];

/// 场景实际分档注解。
pub const SCENE_WANT_NOTE: [Note; SCENE_COUNT] = [
    Note::BrokenPipe,
    Note::Blocked,
    Note::SigpipeDeath,
    Note::Drained,
    Note::SpaceLie,
    Note::Drained,
    Note::Drained,
    Note::OutOfScope,
    Note::SigpipeDeath,
];

/// 场景实际是否结构自洽。
pub const SCENE_WANT_CONSISTENT: [bool; SCENE_COUNT] =
    [true, true, true, false, false, true, true, false, true];

/// 场景实际锚位。
pub const SCENE_WANT_DEFAULT: [bool; SCENE_COUNT] =
    [false, false, false, false, false, false, false, false, false];

/// 取第 i 个场景。
pub fn scene_at(i: usize) -> Option<Shape> {
    scenes().get(i).copied()
}

/// 第 i 个场景是否落了锚。
pub fn scene_anchored(i: usize) -> Option<bool> {
    SCENE_WANT_DEFAULT.get(i).copied()
}

/// 场景的诚实语料格（申报逐格等于复算，用来单测物理面）。
pub fn scene_case(i: usize) -> Option<Case> {
    let sh = scene_at(i)?;
    let tg = *SCENE_TAGS.get(i)?;
    let a = define(&sh);
    Some(Case {
        shape: sh,
        claim: Claim::make(a.stage, a.code, a.value, a.note, a.default),
        tag: tg,
    })
}

/// 场景族是否与期望表逐格一致，且三具谎报现场都落在谎报档。
pub fn scenes_agree() -> bool {
    let mut i = 0usize;
    while i < SCENE_COUNT {
        let Some(sh) = scene_at(i) else {
            return false;
        };
        let a = define(&sh);
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
        let Some(cs) = SCENE_WANT_CONSISTENT.get(i) else {
            return false;
        };
        let Some(df) = SCENE_WANT_DEFAULT.get(i) else {
            return false;
        };
        if a.stage != *st || a.code != *cd || a.value != *vl || a.note != *nt {
            return false;
        }
        if a.consistent != *cs || a.default != *df {
            return false;
        }
        if Tier::of(&a) == Tier::Falsified && *cs {
            return false;
        }
        if let Some(c) = scene_case(i) {
            if !matches!(judge(&c), Fault::Ok) {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 矩阵：按 `Tier::of(&actual)` 分四档
// ---------------------------------------------------------------------------

/// 一档的行统计。
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

    /// 渲染一行。
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
    /// 一档：全绿交付。
    pub drained: Row,
    /// 二档：诚实红。
    pub stalled: Row,
    /// 三档：未碰管道。
    pub unpiped: Row,
    /// 四档：谎报。
    pub falsified: Row,
}

impl Matrix {
    /// 零矩阵。
    pub const fn blank() -> Matrix {
        Matrix {
            drained: Row::blank(),
            stalled: Row::blank(),
            unpiped: Row::blank(),
            falsified: Row::blank(),
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
                Tier::Drained => &mut m.drained,
                Tier::Stalled => &mut m.stalled,
                Tier::Unpiped => &mut m.unpiped,
                Tier::Falsified => &mut m.falsified,
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
            Tier::Drained => Some(self.drained),
            Tier::Stalled => Some(self.stalled),
            Tier::Unpiped => Some(self.unpiped),
            Tier::Falsified => Some(self.falsified),
        }
    }

    /// 四档格数合计。
    pub fn slots(&self) -> usize {
        self.drained
            .total
            .saturating_add(self.stalled.total)
            .saturating_add(self.unpiped.total)
            .saturating_add(self.falsified.total)
    }

    /// 四档判对合计。
    pub fn goods(&self) -> usize {
        self.drained
            .good
            .saturating_add(self.stalled.good)
            .saturating_add(self.unpiped.good)
            .saturating_add(self.falsified.good)
    }

    /// 全对的行数（空行不算全对）。
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

    /// 全矩阵红格数。
    pub fn red_total(&self) -> usize {
        self.drained
            .red()
            .saturating_add(self.stalled.red())
            .saturating_add(self.unpiped.red())
            .saturating_add(self.falsified.red())
    }

    /// 覆盖是否完整。
    pub fn covered(&self) -> bool {
        self.slots() == CORPUS && self.goods() == ACCEPT
    }

    /// 按档位取行（`Tier::of` 的反查面）。
    pub fn tier_counts(&self) -> [usize; TIER_COUNT] {
        [
            self.drained.total,
            self.stalled.total,
            self.unpiped.total,
            self.falsified.total,
        ]
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
    pub drained: u32,
    /// 二档（诚实红）格数。
    pub stalled: u32,
    /// 三档（未碰管道）格数。
    pub unpiped: u32,
    /// 四档（谎报）格数。
    pub falsified: u32,
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

    /// 是否全绿。
    pub fn green(&self) -> bool {
        self.total == CORPUS as u32 && self.matched == self.total
    }

    /// 红档数。
    pub fn reds(&self) -> u32 {
        self.total.saturating_sub(self.matched)
    }

    /// 闸门：不全绿就报计数错。
    pub fn gate(&self) -> Result<(), u32> {
        if self.green() {
            Ok(())
        } else {
            Err(PF_ECOUNT)
        }
    }

    /// 四档格数合计。
    pub fn tier_cells(&self) -> u32 {
        self.drained
            .saturating_add(self.stalled)
            .saturating_add(self.unpiped)
            .saturating_add(self.falsified)
    }

    /// 码非零的档合计（诚实红 + 谎报）。
    pub fn red_cells(&self) -> u32 {
        self.stalled.saturating_add(self.falsified)
    }

    /// 渲染整份汇总（逐字稳定，与 `RENDER_OK` 对账）。
    pub fn render(&self) -> String {
        let mut s = String::from("pf-pipe corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" drained=");
        s.push_str(u32_text(self.drained).as_str());
        s.push_str(" stalled=");
        s.push_str(u32_text(self.stalled).as_str());
        s.push_str(" unpiped=");
        s.push_str(u32_text(self.unpiped).as_str());
        s.push_str(" falsified=");
        s.push_str(u32_text(self.falsified).as_str());
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
const RENDER_OK: &str = "pf-pipe corpus=14 accept=8 reject=6 drained=3 stalled=4 unpiped=1 falsified=0 anchor=1 match=14 rate=1000ppm\n";

/// 空报表。
pub fn empty_report() -> Report {
    Report {
        total: 0,
        accept: 0,
        reject: 0,
        drained: 0,
        stalled: 0,
        unpiped: 0,
        falsified: 0,
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
/// 是硬编码的而不是算出来的。槽号越界一律 `PF_ESLOT`。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(PF_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut drained = 0u32;
    let mut stalled = 0u32;
    let mut unpiped = 0u32;
    let mut falsified = 0u32;
    let mut anchored = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let Some(c) = cs.get(i) else {
            return Err(PF_EOBS);
        };
        let a = c.actual();
        let mut f = judge(c);
        if let Some((j, forced)) = over {
            if j == i {
                f = forced;
            }
        }
        let Some(want) = expected(i) else {
            return Err(PF_EOBS);
        };
        total = total.saturating_add(1);
        if f == want {
            matched = matched.saturating_add(1);
        }
        if matches!(f, Fault::Ok) {
            accept = accept.saturating_add(1);
            match Tier::of(&a) {
                Tier::Drained => drained = drained.saturating_add(1),
                Tier::Stalled => stalled = stalled.saturating_add(1),
                Tier::Unpiped => unpiped = unpiped.saturating_add(1),
                Tier::Falsified => falsified = falsified.saturating_add(1),
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
        drained,
        stalled,
        unpiped,
        falsified,
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
    if r.drained != m.drained.good as u32 {
        return false;
    }
    if r.stalled != m.stalled.good as u32 {
        return false;
    }
    if r.unpiped != m.unpiped.good as u32 {
        return false;
    }
    if r.falsified != m.falsified.good as u32 {
        return false;
    }
    if m.goods() != ACCEPT || m.perfect_rows() != TIER_COUNT - 1 {
        return false;
    }
    r.tier_cells() as usize == ACCEPT
}

/// 红档率标定：注入一槽必须落到 929，且两槽注入必须更低。
pub fn red_rate_calibrated() -> bool {
    let Ok(base) = run() else {
        return false;
    };
    if base.rate() != RATE_FULL {
        return false;
    }
    match run_with(Some((0usize, Fault::Note))) {
        Ok(r) => {
            if r.rate() != RED_RATE {
                return false;
            }
            if r.matched as usize != CORPUS - 1 {
                return false;
            }
        }
        Err(_) => return false,
    }
    match run_with(Some((4usize, Fault::Estate))) {
        Ok(r) => r.rate() < RATE_FULL,
        Err(_) => false,
    }
}

/// 六具尸位各自压低红档率（一次注入一槽，六次都必须掉，不许有"注入无效"的哑槽）。
pub fn inject_six_lowers_rate() -> bool {
    let Ok(base) = run() else {
        return false;
    };
    let mut i = 0usize;
    while i < REJECT {
        let Some(k) = REJECT_SLOTS.get(i) else {
            return false;
        };
        let Some(f) = REJECT_FAULTS.get(i) else {
            return false;
        };
        // 死槽本来判的就是这一维：把它改判成另一维，报表必须掉。
        let other = match REJECT_FAULTS.get((i + 1) % REJECT) {
            Some(x) => *x,
            None => return false,
        };
        let _ = f;
        match run_with(Some((*k, other))) {
            Ok(r) => {
                if r.rate() >= base.rate() {
                    return false;
                }
            }
            Err(_) => return false,
        }
        i += 1;
    }
    true
}

// @@CHUNK5
