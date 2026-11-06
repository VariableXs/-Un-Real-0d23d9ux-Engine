//! UNX-F11677 帮助文本双机快照验证
//!
//! 这张卡管的是**同一条帮助文本在两台机器上是不是同一个字节序列**。
//! 前一卡 F11676 判"三档 CONFIG 裁剪后的 notfound 矩阵"（哪些 applet 存在），
//! 后一卡 F11678 判"错误进 stderr、help 进 stdout"（文本落到哪条流）。
//! 本卡夹在中间，管的是最朴素也最容易糊弄过去的一件事：**逐字**。
//!
//! ```text
//! # 基准机（GNU coreutils 9.5，LC_ALL=C，COLUMNS=80）
//! $ touch --help | od -c | head -1
//! 0000000   U   s   a   g   e   :       t   o   u   c   h       [ O P T I O N ] . . .
//! # 目标机（同一版本、同一档位，只是终端宽度 100）
//! $ touch --help | md5sum
//! d2c1...   -                          ← 码 0，帮助照样"成功"打印了
//! ```
//!
//! 两行都不报错、退出码都是 0，文本却不同。**帮助文本没有失败这回事**，
//! 所以"码 0"在帮助面上完全不代表"文本一致"——这就是本卡的心脏：
//! 帮助/文案的一致性必须独立于退出码成立，一致率 100% 才算绿，
//! 任何一个字节的漂移都要显式挂出 CONFLICT，不许被"看起来一样"吃掉。
//!
//! 双机快照真正会漂移的维度有十一条，每条都有真实来源：
//!
//! * **换行符档**（`\n` vs `\r\n`）：基准机管道导出、目标机经过 Windows
//!   侧的文本模式转换，肉眼完全看不出，`wc -l` 也照样对。
//! * **行尾空白**：coreutils 在 8.32 前后清理过 `--help` 里的尾随空格，
//!   同一大版本不同小版本就会差；这是"版本串相同但文本不同"的正宗现场。
//! * **换行宽度**：gnulib 的 `usage` 串本身不折行，但带 `--help` 的程序
//!   普遍按 `COLUMNS`（缺省 80）折；一台导出 `COLUMNS=100`、另一台未设，
//!   长选项说明那几行会**在不同列折断**，行数相同、字节数不同。
//! * **裁剪档位**：`--enable-small` / `CONFIG_FEATURE_*` 关掉之后，帮助里
//!   对应的选项行**整行消失**（不是变短）。本卡只判"帮助文本少了几行"，
//!   谁被裁掉归 F11676 判。
//! * **译文档**：`LC_ALL=C` 与 `zh_CN.UTF-8` 下 gettext 命中不同 msg，
//!   行数甚至相同而字节数完全不同；跨机比较必须钉死同一档，否则
//!   一致率是假的。
//! * **程序名**：帮助首行与页脚里的程序名来自 `argv[0]` 基名
//!   （`Try 'ls --help' for more information.` 与
//!   `Full documentation at: <https://www.gnu.org/software/coreutils/ls>`）；
//!   同一个二进制被软链成别的名字调用，首行与页脚一起变，正文一个字节没动。
//!   这类"只差名字"的漂移在快照里最容易被归一化抹平，本卡判死归一化。
//! * **applet 列表排序**：`busybox --list` 在不同构建里可能按注册序或
//!   字典序输出，集合相同、次序不同——`sort` 之后一致率 100%，
//!   逐字一致率 0%。本卡把"先排序再比"归入不可信比对档。
//! * **版本横幅时间戳**：`BusyBox v1.36.1 (2023-11-07 11:02:00 PST) multi-call binary.`
//!   里带**构建时刻**。两台机器不可能同秒构建，所以这条横幅在双机快照里
//!   永远不可能逐字相等。正确做法是**显式 CONFLICT**（本卡放行），
//!   而不是"忽略时间戳字段"（本卡判死）——一旦允许忽略，同一档里
//!   真实的内容差异也就一起被忽略了。
//! * **页脚与 info 指引**、**`Try ... --help` 提示行有无**、**末行换行符**：
//!   三处都是"最后一行"级别的差异，`head`/`grep` 类抽查永远抓不到。
//!
//! 比对方式本身也是一维，而且是最容易被做假的一维：
//! `diff` 的退出码就是本卡的码维（**0 相同 / 1 不同 / 2 有麻烦**，
//! 这是 diff 的真实语义，不是本卡自造）。用 `diff -w`（忽略空白）、
//! 用 `cmp -s` 之前先 `sed` 归一化、只比行数、或者干脆跳过比对直接报
//! "一致"——四种都属不可信比对档，本卡一律压成"掩盖"档判死，
//! 因为**一致率 100% 这个判据一旦被归一化污染，它就再也无法证明任何事**。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 八道闸，秩从 **1** 起算；`Gate` 只接受"前进到下一秩"。前一闸不产出事实，
//! 后一闸就没有输入，倒过来必先产生假绿。
//!
//! 1. `ask`（秩 1）——**请求形态**。`--help` / `-h` / `help <applet>` /
//!    无操作数 usage / `--version` / `--list` 取的是**不同文本源**，
//!    各自的应有码也不同。这一闸排在最前是为了钉死：请求形态不同就不要
//!    拿去比（两台机器一个跑 `--help` 一个跑 `-h`，"一致率 100%"是笑话）。
//! 2. `name`（秩 2）——**程序名**。首行与页脚由 `argv[0]` 基名生成。
//!    它必须在 `ask` 之后（先确认两边要的是同一种文本）、在 `cfg` 之前
//!    （名字差异会伪装成"整段文本不同"，先摘掉才能看见真实档位差）。
//! 3. `cfg`（秩 3）——**编译档位**。Full/Trim/Tiny 决定帮助里**有哪些行**，
//!    是行集合层面的差异，必须先于"行内容"判：否则裁剪掉一行会被
//!    后面的宽度闸误读成"折行不同"。
//! 4. `text`（秩 4）——**文本源是否产出**。一侧根本没打出文本（applet 无
//!    help、Tiny 档把 help 编掉了、进程被信号打断），此时**没有对照面**，
//!    后面所有比对口径都不成立。这一闸必须能"熔断"：它给出 2 档（有麻烦）
//!    而不是 1 档（不同），因为"没文本"和"文本不同"是两种账。
//! 5. `locale`（秩 5）——**译文档**。两边必须同一档才能进入逐字比较；
//!    排在 `text` 之后是因为要先有文本才谈得上它是什么语言。
//! 6. `wrap`（秩 6）——**行宽**。`COLUMNS` 一致才比；排在 locale 之后，
//!    因为译文长度变化会连带改变折行位置，先定语种再定宽度。
//! 7. `stream`（秩 7）——**落流与冲刷**。help 全文必须走 stdout（与 F11678
//!    联签），且捕获前必须冲刷；文本落进 stderr 或未冲刷，快照拿到的是
//!    一个**不完整的串**，此时逐字比对结果没有意义。排在内容闸之前，
//!    是因为它否定的是"这次比较本身是否成立"，而不是"内容差在哪"。
//! 8. `snap`（秩 8）——**快照封印闸**。前七闸全绿才进入逐字比对，
//!    按固定优先级挂出差异类别（时间戳 → 页脚 → 提示行 → 行尾空白 →
//!    换行符 → 末行换行 → 排序），或宣告 `identical` 并点亮**一致锚**。
//!    锚只在"码 0 且零差异且比对方式可信"时亮——这一句就是本卡判据原文
//!    "逐字一致率 100%"在代码里的落点。
//!
//! ## 关键差异（本卡深度所在）
//!
//! * **与 F11676（裁剪矩阵重建抽样，段基址 `0x6d2a0`，位距 `0x10`）的边界**：
//!   F11676 判**哪些 applet/选项存在**（行集合的有无，三档 CONFIG 矩阵）；
//!   本卡判**同一条帮助文本的字节序列**。同一个 Tiny 档，F11676 说
//!   "`ls` 的 `-Z` 不存在"，本卡说"`ls --help` 比基准机少 2 行、
//!   少 61 字节，差异类别是裁剪"。本卡第 3 闸只**承认**档位差存在并定性，
//!   不重算裁剪矩阵；反过来说，F11676 的矩阵里 applet 明明存在而帮助文本
//!   少了一整段，这类矛盾归本卡判死。
//! * **与 F11678（notfound stderr/stdout 分离，段基址 `0x6d2d0`，位距 `0x20`）
//!   的边界**：F11678 判**落到哪条流**（错误进 stderr、help 进 stdout）；
//!   本卡判**落定之后文本是否逐字相同**。本卡第 7 闸借用它的结论
//!   （落错流 ⇒ 本次快照作废），但本卡绝不裁"该不该进 stderr"——那是
//!   F11678 的账。两卡在同一现场联签，口径不同：F11678 全绿而本卡红
//!   （流对了、内容漂了）是本批最典型的组合。
//! * **与 F11679（未知 applet 场景 × 退出码矩阵，段基址 `0x6d2e0`，位距
//!   `0x30`）的边界**：F11679 管 **127/1/0/2 的场景×码映射**；本卡的码维
//!   只有 `diff` 的 0/1/2 三档，**不出现 127**，也不许把 127 当作"帮助
//!   文本不一致"的证据。applet 未注册时的 `applet not found` 串在本卡
//!   只作为"快照里的一行文本"参与比对，其文案对不对归 F11674
//!   （段基址 `0x6d270`）、退出码对不对归 F11679。
//! * **与 F11667（busybox `--help` 退出码 0 与 usage 全文，段基址 `0x6d1e0`）
//!   的边界**：F11667 判**单机上** usage 段齐不齐、码是否为 0；本卡判
//!   **两台机器**的同一份 usage 是否逐字相同。F11667 全绿（段齐、码 0）
//!   而本卡红（两机的 usage 换行宽度不同）是合法状态，反过来也成立——
//!   本卡不许用"这段本来就缺"给自己松绑，那种缺归 F11667 判。
//! * **本卡的排他职责**：① 把"退出码 0"与"文本一致"拆成两维，
//!   任何一维单独成立都不许点亮一致锚；② 把比对方式（逐字 / 忽略空白 /
//!   只比行数 / 跳过）立成独立一维，不可信比对一律压进"掩盖"档；
//!   ③ 版本横幅构建时间戳**必须显式冲突**，归一化忽略判死；
//!   ④ 程序名（`argv[0]` 基名）漂移要在第 2 闸单独定性，不许混进内容差异；
//!   ⑤ 一致率以千分率落账，红档标定值 929（14 格中 13）钉死在账本第 8 格。
//! * **谁判死谁放行**：同一请求形态、同名、同档、同 locale、同宽度、
//!   落对流且冲刷、逐字比对后零差异 ⇒ 放行并亮一致锚；差异存在但被
//!   如实申报 ⇒ 放行（诚实冲突档，判据要的就是它显式）；差异存在却被
//!   归一化/排序/忽略时间戳/只比行数吃掉，或码 0 被当成一致 ⇒ 判死。
//!
//! 判据六维：结束阶段（stage）、比对码（code，diff 的 0/1/2）、差异项数
//! （value）、差异类别（note）、一致锚（default）、成败申报（estate）。
//! 判序固定：状态 → 阶段 → 码 → 差异数 → 类别 → 锚位。
//!
//! 语料 14 槽：8 槽申报正确（逐字全等 3、诚实冲突 3、无对照面 1、
//! 掩盖如实申报 1），6 槽一槽只坏一维，依次挂 Estate/Stage/Code/Value/
//! Note/Default。账本 14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码，
//! 冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11677）。取自分配表，不许自改。
pub const SEG_BASE: u32 = 0x6d2b0;

/// 交叉署名：F11676「裁剪矩阵重建抽样」的段基址（本卡的直接前置）。
/// 本卡**不 import** 那个模块的任何符号，只把段位与位距原样声明一遍。
pub const ANCHOR_F11676_SEG: u32 = 0x6d2a0;
/// F11676 与本卡的位距（0x6d2b0 - 0x6d2a0）。
pub const ANCHOR_F11676_GAP: u32 = 0x10;

/// 交叉署名：F11678「notfound stderr/stdout 分离」的段基址（本卡的直接后置）。
pub const ANCHOR_F11678_SEG: u32 = 0x6d2d0;
/// F11678 与本卡的位距（0x6d2d0 - 0x6d2b0）。
pub const ANCHOR_F11678_GAP: u32 = 0x20;

/// 交叉署名：F11679「未知 applet 场景 × 退出码矩阵」的段基址。
/// 该卡的 127 档**不进**本卡的码维，本卡只借它的边界说明"谁判 127"。
pub const ANCHOR_F11679_SEG: u32 = 0x6d2e0;
/// F11679 与本卡的位距（0x6d2e0 - 0x6d2b0）。
pub const ANCHOR_F11679_GAP: u32 = 0x30;

/// 交叉署名：F11667「busybox --help 退出码 0 与 usage 全文」的段基址。
pub const ANCHOR_F11667_SEG: u32 = 0x6d1e0;
/// F11667 与本卡的位距（0x6d2b0 - 0x6d1e0）。
pub const ANCHOR_F11667_GAP: u32 = 0xd0;

/// 交叉署名：F11674「未知 applet 文案逐字」的段基址。文案真值归该卡，
/// 本卡只把整段文本作为快照参与逐字比较。
pub const ANCHOR_F11674_SEG: u32 = 0x6d270;
/// F11674 与本卡的位距（0x6d2b0 - 0x6d270）。
pub const ANCHOR_F11674_GAP: u32 = 0x40;

// ---------------------------------------------------------------------------
// 规模常量
// ---------------------------------------------------------------------------

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 申报正确的槽数（活槽）。
pub const ACCEPT: usize = 8;

/// 申报错误的槽数（死槽，一槽只坏一维）。
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

/// 场景族格数（语料之外，逐格钉住十一条漂移维与四种比对档）。
pub const SCENE_COUNT: usize = 12;

/// gnulib `--help` 正文的行数下界（少于这个行数就是**没拿到文本**，不是短文本）。
pub const LINE_FLOOR: u32 = 4;

/// 快照行数上界（语料里最深 26 行，超出即形状非法）。
pub const LINE_CEIL: u32 = 64;

/// 单行长度上界（本卡语料里最长的一行 79 列，超过即形状非法）。
pub const LINE_WIDTH_MAX: u32 = 100;

/// gnulib/终端缺省折行列数。
pub const COLS_DEFAULT: u32 = 80;

/// 第二台机器的对照列数（故意取一个不同值，逼出换行漂移档）。
pub const COLS_WIDE: u32 = 100;

/// `COLUMNS` 未导出的哨兵值（0 表示"没设"，与"设成 0"在本卡里同档）。
pub const COLS_UNSET: u32 = 0;

/// 本卡认得的差异维条数（换行符/行尾空白/宽度/裁剪/译文/名字/排序/时间戳/
/// 页脚/提示行/末行换行，共 11 条，另有 1 条比对方式维独立成 `Compare`）。
pub const DIVERGENCE_KINDS: usize = 11;

// ---------------------------------------------------------------------------
// 码维（GNU diff 的真实退出码语义：0 相同 / 1 不同 / 2 有麻烦）
// ---------------------------------------------------------------------------

/// 逐字相同。注意：**码 0 不等于一致**，还要 `value == 0` 且比对方式可信。
pub const CODE_SAME: i32 = 0;
/// 文本不同（诚实冲突）。
pub const CODE_DIFF: i32 = 1;
/// 比对没法做（一侧无文本、落错流、没冲刷、比对器自己出错）。
pub const CODE_TROUBLE: i32 = 2;
/// 本卡不裁定的占位码：故意取一个撞不上的值，越界行不可能被读成正常行。
pub const CODE_UNJUDGED: i32 = 99;

/// 码维档位数量（0/1/2 三档真实码 + 1 档不裁定）。
pub const CODE_RANKS: usize = 4;

/// 本卡码维集合大小（含占位档）。
pub const CODE_SET_SIZE: usize = 4;

/// 与 F11679 的边界锚：那一档的 127 **绝不**出现在本卡码维里。
pub const FORBIDDEN_APPLET_CODE: i32 = 127;

/// 与 F11679 的边界锚：那一档的 126（found but not executable）同样禁止。
pub const FORBIDDEN_EXEC_CODE: i32 = 126;

// ---------------------------------------------------------------------------
// 帮助请求形态的选项位（`opts` 位图）
// ---------------------------------------------------------------------------

/// `--help`：gnulib 的长选项帮助，走 stdout，退出 0。
pub const OPT_HELP_LONG: u8 = 0x01;
/// `-h`：busybox 习惯用 `-h` 打 usage；GNU 多数 applet **不认** `-h`。
pub const OPT_HELP_SHORT: u8 = 0x02;
/// `--version`：版本横幅，busybox 档里带构建时刻。
pub const OPT_VERSION: u8 = 0x04;
/// `--list`：busybox 的 applet 清单（次序随构建而变）。
pub const OPT_LIST: u8 = 0x08;
/// `--list --full`：busybox 的带说明清单，比 `--list` 更长。
pub const OPT_LIST_FULL: u8 = 0x10;
/// `help APPLET`：busybox 的 help applet，取单个 applet 的 usage。
pub const OPT_HELP_APPLET: u8 = 0x20;
/// 无操作数：GNU 退 2 打 `Try 'x --help'`，busybox 退 1 打 usage。
pub const OPT_BARE: u8 = 0x40;
/// `-s`：静默比较（`diff -s` 相同才打一行 "identical"）。
pub const OPT_REPORT_SAME: u8 = 0x80;

/// 本卡认得的请求位全集。
pub const OPT_KNOWN: u8 = OPT_HELP_LONG
    | OPT_HELP_SHORT
    | OPT_VERSION
    | OPT_LIST
    | OPT_LIST_FULL
    | OPT_HELP_APPLET
    | OPT_BARE
    | OPT_REPORT_SAME;

/// 请求位个数（语料与谓词按位遍历用）。
pub const OPT_BIT_COUNT: usize = 8;

/// 请求位表（顺序与 `OPT_*` 值一致）。
pub const OPT_BITS: [u8; OPT_BIT_COUNT] = [
    OPT_HELP_LONG,
    OPT_HELP_SHORT,
    OPT_VERSION,
    OPT_LIST,
    OPT_LIST_FULL,
    OPT_HELP_APPLET,
    OPT_BARE,
    OPT_REPORT_SAME,
];

// ---------------------------------------------------------------------------
// 差异位图（`diverg`，u16；十一条漂移维各占一位）
// ---------------------------------------------------------------------------

/// `\n` 对 `\r\n`：肉眼不可见的换行符档差。
pub const DIV_CRLF: u16 = 0x0001;
/// 行尾空白：coreutils 8.32 前后清理过 `--help` 的尾随空格。
pub const DIV_TRAILING: u16 = 0x0002;
/// 折行宽度：`COLUMNS` 不同，长说明在不同列断开。
pub const DIV_WRAP: u16 = 0x0004;
/// 裁剪档位：Trim/Tiny 把选项行整行删掉。
pub const DIV_TRIM: u16 = 0x0008;
/// 译文档：`LC_ALL` 不同，gettext 命中不同 msg。
pub const DIV_LOCALE: u16 = 0x0010;
/// 程序名：`argv[0]` 基名进首行与页脚。
pub const DIV_NAME: u16 = 0x0020;
/// 清单排序：`busybox --list` 集合同次序不同。
pub const DIV_ORDER: u16 = 0x0040;
/// 构建时间戳：版本横幅里 `(...)` 的时刻，双机必然不同。
pub const DIV_STAMP: u16 = 0x0080;
/// 页脚：`Full documentation at:` 与 `info '(coreutils) x invocation'` 两行。
pub const DIV_FOOTER: u16 = 0x0100;
/// 提示行：`Try 'x --help' for more information.` 一行有无。
pub const DIV_HINT: u16 = 0x0200;
/// 末行换行：一侧最后一行没有 `\n`。
pub const DIV_EOFNL: u16 = 0x0400;
/// 别名大小写：`Ls` 与 `ls` 取到不同文本源（本卡只当文本差异读）。
pub const DIV_CASE: u16 = 0x0800;
/// 未映射位（本卡认得的位之外）。
pub const DIV_UNKNOWN_MASK: u16 = 0xf000;

/// 本卡认得的差异位全集。
pub const DIV_KNOWN: u16 = DIV_CRLF
    | DIV_TRAILING
    | DIV_WRAP
    | DIV_TRIM
    | DIV_LOCALE
    | DIV_NAME
    | DIV_ORDER
    | DIV_STAMP
    | DIV_FOOTER
    | DIV_HINT
    | DIV_EOFNL
    | DIV_CASE;

/// 差异位数（12，含大小写位；判据原文的十一条内容漂移维 + 1 条别名档）。
pub const DIV_BIT_COUNT: usize = 12;

/// 差异位表（顺序与 `DIV_*` 值一致）。
pub const DIV_BITS: [u16; DIV_BIT_COUNT] = [
    DIV_CRLF,
    DIV_TRAILING,
    DIV_WRAP,
    DIV_TRIM,
    DIV_LOCALE,
    DIV_NAME,
    DIV_ORDER,
    DIV_STAMP,
    DIV_FOOTER,
    DIV_HINT,
    DIV_EOFNL,
    DIV_CASE,
];

/// 一次快照最多挂几条差异（语料里最多 3 条，超出即形状非法）。
pub const DIV_MAX_Kinds: u32 = 6;

// ---------------------------------------------------------------------------
// 错误码（13 个，SEG_BASE + 0x01..0x0d）
// ---------------------------------------------------------------------------

/// 01 阶段申报不符：差异落在哪道闸说错了（内容漂移被写成封印闸差异）。
pub const HD_ESTAGE: u32 = SEG_BASE + 0x01;
/// 02 比对码错位：0/1/2 三档互换（无对照面被写成"文本不同"）。
pub const HD_ECODE: u32 = SEG_BASE + 0x02;
/// 03 差异项数错位：挂出来的漂移条数与实际不符。
pub const HD_EVALUE: u32 = SEG_BASE + 0x03;
/// 04 差异类别错位：把时间戳漂移说成行尾空白（九种真实类别混认）。
pub const HD_ENOTE: u32 = SEG_BASE + 0x04;
/// 05 一致锚错位：码 0 就当一致（漏亮锚），或有漂移却亮锚（多亮锚）。
pub const HD_EDEFAULT: u32 = SEG_BASE + 0x05;
/// 06 对照档错位：locale/CONFIG/列数/比对方式四类前提混认。
pub const HD_EPRECOND: u32 = SEG_BASE + 0x06;
/// 07 落流错位：help 全文没走 stdout，或快照前没冲刷（与 F11678 联签维）。
pub const HD_ESTREAM: u32 = SEG_BASE + 0x07;
/// 08 封印状态违例：快照已封印还继续走闸/再封印。
pub const HD_ESTATE: u32 = SEG_BASE + 0x08;
/// 09 计数不平：档位数、行数、对判命中数三者对不上。
pub const HD_ECOUNT: u32 = SEG_BASE + 0x09;
/// 10 槽位越界：账本槽号不等于当前长度，或注入槽号越界。
pub const HD_ESLOT: u32 = SEG_BASE + 0x0a;
/// 11 指纹不符：重放与冻结指纹不同（账被改过）。
pub const HD_ESEAL: u32 = SEG_BASE + 0x0b;
/// 12 观测缺失：期望表/场景表越界，判据无从对账。
pub const HD_ENOBS: u32 = SEG_BASE + 0x0c;
/// 13 账本不平：格数、成败分布或统计值与标定表不符。
pub const HD_EBAL: u32 = SEG_BASE + 0x0d;

/// 全部 13 个错误码（顺序即码号顺序）。
pub const ALL_CODES: [u32; 13] = [
    HD_ESTAGE,
    HD_ECODE,
    HD_EVALUE,
    HD_ENOTE,
    HD_EDEFAULT,
    HD_EPRECOND,
    HD_ESTREAM,
    HD_ESTATE,
    HD_ECOUNT,
    HD_ESLOT,
    HD_ESEAL,
    HD_ENOBS,
    HD_EBAL,
];

/// 账本操作码：语料总格数。
pub const OP_CORPUS: u8 = 0x01;
/// 账本操作码：逐字全等格数。
pub const OP_VERBATIM: u8 = 0x02;
/// 账本操作码：诚实冲突格数。
pub const OP_CONFLICT: u8 = 0x03;
/// 账本操作码：无对照面格数。
pub const OP_NOTEXT: u8 = 0x04;
/// 账本操作码：一致锚亮格数。
pub const OP_ANCHOR: u8 = 0x05;
/// 账本操作码：红档率。
pub const OP_RATE: u8 = 0x06;
/// 账本操作码：缺陷格。
pub const OP_DEFECT: u8 = 0x07;
/// 账本操作码：重放。
pub const OP_REPLAY: u8 = 0x08;
/// 账本操作码：比对方式维。
pub const OP_COMPARE: u8 = 0x09;
/// 账本操作码：落流维。
pub const OP_STREAM: u8 = 0x0a;
/// 账本操作码：译文档维。
pub const OP_LOCALE: u8 = 0x0b;
/// 账本操作码个数。
pub const OP_COUNT: usize = 11;

/// 操作码表（谓词遍历用，顺序与值一致）。
pub const OPS: [u8; OP_COUNT] = [
    OP_CORPUS,
    OP_VERBATIM,
    OP_CONFLICT,
    OP_NOTEXT,
    OP_ANCHOR,
    OP_RATE,
    OP_DEFECT,
    OP_REPLAY,
    OP_COMPARE,
    OP_STREAM,
    OP_LOCALE,
];

// ---------------------------------------------------------------------------
// FNV-1a 64 位指纹
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移量。
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

/// 折入一个字符串（快照文本逐字节进指纹）。
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

/// 折入一个有符号数（比对码用）。
fn fold_i32(h: u64, v: i32) -> u64 {
    fold_word(h, v as i64 as u64)
}

/// 折入一个 16 位位图（差异位用）。
fn fold_u16(h: u64, v: u16) -> u64 {
    fold_word(h, v as u64)
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

/// 有符号十进制文本（比对码是 i32）。
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

/// 段位文本：固定 `0x6dxxx` 式样，宽度钉死，防止指纹里段位串位。
pub fn seg_text(v: u32) -> String {
    let mut s = String::from("0x");
    s.push_str(hex_text(v, 5).as_str());
    s
}

/// 差异位图的十六进制文本（定宽 4 位）。
pub fn div_text(bits: u16) -> String {
    hex_text(bits as u32, 4)
}

/// 千分率文本：`1000ppm` 这种写法与快照报表逐字对账用。
fn permille_text(v: u32) -> String {
    let mut s = dec_text(v);
    s.push_str("ppm");
    s
}

// ---------------------------------------------------------------------------
// 快照文本真值：逐字复刻上游 --help / usage 措辞
//
// 这一段的纪律：一个字符都不许多，也不许少。基准机与目标机的快照
// 都由这些串拼出来，任何一处笔误都会在 message_ok() 与语料对账时露出来。
// ---------------------------------------------------------------------------

/// GNU coreutils `touch --help` 首行（真实措辞，`[OPTION]...` 前一个空格）。
pub const SNAP_GNU_TOUCH_USAGE: &str = "Usage: touch [OPTION]... [FILE]...";

/// GNU coreutils `ls --help` 首行。
pub const SNAP_GNU_LS_USAGE: &str = "Usage: ls [OPTION]... [FILE]...";

/// GNU coreutils `mkdir --help` 首行（`DIRECTORY...`，不是 `DIR...`）。
pub const SNAP_GNU_MKDIR_USAGE: &str = "Usage: mkdir [OPTION]... DIRECTORY...";

/// GNU coreutils `cat --help` 首行。
pub const SNAP_GNU_CAT_USAGE: &str = "Usage: cat [OPTION]... [FILE]...";

/// GNU coreutils `cp --help` 第一形态：`Usage:` 之后是**两个空格**。
pub const SNAP_GNU_CP_USAGE_MAIN: &str = "Usage:  cp [OPTION]... [-T] SOURCE DEST";

/// GNU coreutils `cp --help` 第二形态：行首**两个空格** + `or:` + 两个空格。
pub const SNAP_GNU_CP_USAGE_ALT1: &str = "  or:  cp [OPTION]... SOURCE... DIRECTORY";

/// GNU coreutils `cp --help` 第三形态。
pub const SNAP_GNU_CP_USAGE_ALT2: &str = "  or:  cp [OPTION]... -t DIRECTORY SOURCE...";

/// GNU coreutils `echo --help` 第一形态（多形态块，与 cp 同族）。
pub const SNAP_GNU_ECHO_USAGE_MAIN: &str = "Usage:  echo [STRING]...";

/// GNU coreutils `echo --help` 第二形态（`-n` 单独成形态）。
pub const SNAP_GNU_ECHO_USAGE_ALT: &str = "  or:  echo -n STRING";

/// gnulib 帮助段的定长说明行：`--help` 前 6 空格、后 5 空格。
pub const SNAP_GNU_HELP_OPT: &str = "      --help     display this help and exit";

/// gnulib 帮助段的定长说明行：`--version` 前 6 空格、后 2 空格（同列对齐）。
pub const SNAP_GNU_VERSION_OPT: &str = "      --version  output version information and exit";

/// gnulib 帮助段的中置说明行，逐字。
pub const SNAP_GNU_MANDATORY: &str =
    "Mandatory arguments to long options are mandatory for short options too.";

/// GNU coreutils 帮助页脚第一行（在线帮助地址）。
pub const SNAP_GNU_ONLINE_HELP: &str =
    "GNU coreutils online help: <https://www.gnu.org/software/coreutils/>";

/// GNU coreutils 帮助页脚第二行的前缀（后面拼 applet 名与 `>`）。
pub const SNAP_GNU_FULL_DOC_PREFIX: &str = "Full documentation at: <https://www.gnu.org/software/coreutils/";

/// GNU coreutils 帮助页脚第三行的前缀（后面拼 applet 名与 ` invocation'`）。
pub const SNAP_GNU_INFO_PREFIX: &str = "or available locally via: info '(coreutils) ";

/// GNU coreutils 帮助页脚第三行的后缀（前面是 applet 名）。
pub const SNAP_GNU_INFO_SUFFIX: &str = " invocation'";

/// GNU 用法错误尾行（单引号包住 `程序名 --help`）。
pub const SNAP_GNU_TRY_HELP_SUFFIX: &str = " --help' for more information.";

/// GNU 用法错误尾行的前缀（`Try '` + 程序名）。
pub const SNAP_GNU_TRY_HELP_PREFIX: &str = "Try '";

/// busybox 版本横幅前缀（`v1.36.1` 与构建时刻之间一个空格）。
pub const SNAP_BB_BANNER_PREFIX: &str = "BusyBox v1.36.1 (";

/// busybox 版本横幅后缀（构建时刻之后到行尾）。
pub const SNAP_BB_BANNER_SUFFIX: &str = ") multi-call binary.";

/// busybox 一个真实构建时刻（双机快照里"必然不同"的那一维）。
pub const SNAP_BB_STAMP_BASE: &str = "2023-11-07 11:02:00 PST";

/// busybox 目标机上的另一个构建时刻（同版本、不同机）。
pub const SNAP_BB_STAMP_TARGET: &str = "2023-11-07 11:04:37 UTC";

/// busybox 全局 usage 第一行。
pub const SNAP_BB_USAGE_MAIN: &str = "Usage: busybox [function [arguments]...]";

/// busybox 全局 usage 第二行：行首**三个空格** + `or:` + 一个空格。
pub const SNAP_BB_USAGE_LIST: &str = "   or: busybox --list[-full]";

/// busybox 全局 usage 第三行。
pub const SNAP_BB_USAGE_INSTALL: &str = "   or: busybox --install [-s] [DIR]";

/// busybox 全局 usage 第四行（裸函数调用形态）。
pub const SNAP_BB_USAGE_FUNC: &str = "   or: function [arguments]...";

/// busybox 版权段第一行（真实措辞，年份区间 1998-2015）。
pub const SNAP_BB_COPYRIGHT: &str = "BusyBox is copyrighted by many authors between 1998-2015.";

/// busybox 版权段第二行（许可说明，`detailed` 换到下一行）。
pub const SNAP_BB_LICENSE: &str = "Licensed under GPLv2. See source distribution for detailed";

/// busybox 版权段第三行（承接上一行的 `copyright notices.`）。
pub const SNAP_BB_LICENSE_TAIL: &str = "copyright notices.";

/// busybox 未注册名时的行（本卡只把它当快照里的一行文本，码归 F11679）。
pub const SNAP_BB_APPLET_NOT_FOUND: &str = "applet not found";

/// 未注册名的 `--help` 落点（F11668 的案发现场；本卡钉住这一行的字节）。
pub const SNAP_BB_NO_HELP: &str = "no help available";

/// GNU `--help` 里被裁掉的选项行示例（Tiny 档整行消失，本卡据此判裁剪）。
pub const SNAP_GNU_TOUCH_OPT_A: &str = "  -a, --access-only          change only access time";

/// 同一 applet 的另一条选项说明行（Full 档才有）。
pub const SNAP_GNU_TOUCH_OPT_M: &str = "  -m, --time-only            change only modification time";

/// 说明行之间必须**逐字相同**的空白列；本卡把列数写进断言而不是靠肉眼看。
pub const SNAP_GNU_OPT_COLUMN: u32 = 27;

/// busybox `--list` 的清单串（基准机次序）。
pub const SNAP_BB_LIST_BASE: &str = "adduser\nls\ncp\n touch";

/// busybox `--list` 的清单串（目标机字典序；集合相同、次序不同）。
pub const SNAP_BB_LIST_SORTED: &str = "adduser\ncp\nls\n touch";

/// 一行 `--help` 里的制表符（本卡语料用空格，出现制表符即换行宽度档之外的漂移）。
pub const SNAP_TAB: &str = "\t";

/// `diff -s` 在两文件相同时打的那一行（`-s` 位为真时的真实输出前缀）。
pub const SNAP_DIFF_IDENTICAL_SUFFIX: &str = " differ";

/// `diff -s` 相同时的输出行前缀（后面拼两个文件名）。
pub const SNAP_DIFF_SAME_PREFIX: &str = "Files ";

/// `diff` 找不到操作数时的真实措辞（本卡用它当"比对器自己出错"档的样本）。
pub fn msg_diff_missing_operand(after: &str) -> String {
    let mut s = String::from("diff: missing operand after '");
    s.push_str(after);
    s.push('\'');
    s
}

/// `diff` 多出一个操作数时的真实措辞。
pub fn msg_diff_extra_operand(operand: &str) -> String {
    let mut s = String::from("diff: extra operand '");
    s.push_str(operand);
    s.push('\'');
    s
}

/// `diff -s` 两文件相同时的逐字输出（`Files A and B are identical`）。
pub fn msg_diff_are_identical(a: &str, b: &str) -> String {
    let mut s = String::from(SNAP_DIFF_SAME_PREFIX);
    s.push_str(a);
    s.push_str(" and ");
    s.push_str(b);
    s.push_str(" are identical");
    s
}

/// `diff` 判定两文件不同的那一路**没有**标准输出行（只有退出码 1）。
/// 本卡把这一事实钉成函数，防止有人以为"码 1 还会打一行 differing"。
pub fn msg_diff_none() -> &'static str {
    ""
}

/// GNU 用法错误尾行：`Try 'ls --help' for more information.`
pub fn msg_gnu_try_help(program: &str) -> String {
    let mut s = String::from(SNAP_GNU_TRY_HELP_PREFIX);
    s.push_str(program);
    s.push_str(SNAP_GNU_TRY_HELP_SUFFIX);
    s.push_str(" information.");
    s
}

/// GNU 未识别长选项的首行：`ls: unrecognized option '--foo'`。
pub fn msg_gnu_unrecognized(program: &str, opt: &str) -> String {
    let mut s = String::from(program);
    s.push_str(": unrecognized option '");
    s.push_str(opt);
    s.push('\'');
    s
}

/// GNU 帮助页脚第二行：`Full documentation at: <.../touch>`。
pub fn msg_gnu_full_doc(applet: &str) -> String {
    let mut s = String::from(SNAP_GNU_FULL_DOC_PREFIX);
    s.push_str(applet);
    s.push('>');
    s
}

/// GNU 帮助页脚第三行：`or available locally via: info '(coreutils) touch invocation'`。
pub fn msg_gnu_info_line(applet: &str) -> String {
    let mut s = String::from(SNAP_GNU_INFO_PREFIX);
    s.push_str(applet);
    s.push_str(SNAP_GNU_INFO_SUFFIX);
    s
}

/// busybox 版本横幅整行（把构建时刻作为参数，正是双机必然不同的一维）。
pub fn msg_bb_banner(stamp: &str) -> String {
    let mut s = String::from(SNAP_BB_BANNER_PREFIX);
    s.push_str(stamp);
    s.push_str(SNAP_BB_BANNER_SUFFIX);
    s
}

/// busybox 未注册名整行：`foo: applet not found`（名前缀呈现，与 F11674 联签）。
pub fn msg_bb_applet_not_found(name: &str) -> String {
    let mut s = String::from(name);
    s.push_str(": ");
    s.push_str(SNAP_BB_APPLET_NOT_FOUND);
    s
}

/// busybox `help 未知名` 的真实落点（F11668 案发现场）。
pub fn msg_bb_no_help(name: &str) -> String {
    let mut s = String::from(name);
    s.push_str(": ");
    s.push_str(SNAP_BB_NO_HELP);
    s
}

/// 按 applet 取 GNU 帮助首行；认不出的一律空串（空串在谓词里判死，不许放行）。
pub fn msg_gnu_usage_line(applet: &str) -> &'static str {
    if applet == "touch" {
        return SNAP_GNU_TOUCH_USAGE;
    }
    if applet == "ls" {
        return SNAP_GNU_LS_USAGE;
    }
    if applet == "mkdir" {
        return SNAP_GNU_MKDIR_USAGE;
    }
    if applet == "cat" {
        return SNAP_GNU_CAT_USAGE;
    }
    if applet == "cp" {
        return SNAP_GNU_CP_USAGE_MAIN;
    }
    if applet == "echo" {
        return SNAP_GNU_ECHO_USAGE_MAIN;
    }
    ""
}

/// 多形态 applet 的第二/第三形态行数（cp=2、echo=1、其余 0）。
/// 本卡用它顶住"只比首行"的假一致：首行同、形态行缺一条也是漂移。
pub fn gnu_alt_form_count(applet: &str) -> u32 {
    if applet == "cp" {
        return 2u32;
    }
    if applet == "echo" {
        return 1u32;
    }
    0u32
}

/// 拼一份 GNU `--help` 快照全文（基准机/目标机共用同一拼装器，
/// 差异由调用方按位图施加）。行数与字节数由 `snap_stats()` 复算。
pub fn snap_gnu_help(applet: &str, forms: bool, with_opts: bool, with_footer: bool) -> String {
    let mut s = String::new();
    s.push_str(msg_gnu_usage_line(applet));
    s.push('\n');
    if forms {
        let n = gnu_alt_form_count(applet);
        if n >= 1u32 {
            if applet == "cp" {
                s.push_str(SNAP_GNU_CP_USAGE_ALT1);
                s.push('\n');
            } else {
                s.push_str(SNAP_GNU_ECHO_USAGE_ALT);
                s.push('\n');
            }
        }
        if n >= 2u32 {
            s.push_str(SNAP_GNU_CP_USAGE_ALT2);
            s.push('\n');
        }
    }
    if with_opts {
        s.push_str(SNAP_GNU_MANDATORY);
        s.push('\n');
        s.push_str(SNAP_GNU_TOUCH_OPT_A);
        s.push('\n');
        s.push_str(SNAP_GNU_HELP_OPT);
        s.push('\n');
        s.push_str(SNAP_GNU_VERSION_OPT);
        s.push('\n');
    }
    if with_footer {
        s.push_str(SNAP_GNU_ONLINE_HELP);
        s.push('\n');
        s.push_str(msg_gnu_full_doc(applet).as_str());
        s.push('\n');
        s.push_str(msg_gnu_info_line(applet).as_str());
        s.push('\n');
    }
    s
}

/// 拼一份 busybox 全局 usage 快照全文（横幅里的构建时刻单独成参）。
pub fn snap_bb_usage(stamp: &str, with_copyright: bool, with_forms: bool) -> String {
    let mut s = String::new();
    s.push_str(msg_bb_banner(stamp).as_str());
    s.push('\n');
    if with_copyright {
        s.push_str(SNAP_BB_COPYRIGHT);
        s.push('\n');
        s.push_str(SNAP_BB_LICENSE);
        s.push('\n');
        s.push_str(SNAP_BB_LICENSE_TAIL);
        s.push('\n');
        s.push('\n');
    }
    s.push_str(SNAP_BB_USAGE_MAIN);
    s.push('\n');
    if with_forms {
        s.push_str(SNAP_BB_USAGE_LIST);
        s.push('\n');
        s.push_str(SNAP_BB_USAGE_INSTALL);
        s.push('\n');
    }
    s.push_str(SNAP_BB_USAGE_FUNC);
    s.push('\n');
    s
}

/// 在快照上施加一类真实漂移（本卡的语料靠它，而不是手改字符串常量）。
/// 返回 `(文本, 差异位)`；位为 0 表示这一类没改动任何东西。
pub fn apply_drift(text: &str, kind: &str) -> (String, u16) {
    let mut s = String::new();
    let mut bit: u16 = 0;
    if kind == "crlf" {
        let bytes = text.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                if *b == b'\n' {
                    s.push('\r');
                }
                s.push(*b as char);
            }
            i += 1;
        }
        bit = DIV_CRLF;
    } else if kind == "trailing" {
        let bytes = text.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                s.push(*b as char);
                if *b == b'\n' {
                    s.push(' ');
                }
            }
            i += 1;
        }
        bit = DIV_TRAILING;
    } else if kind == "eof-nl" {
        let bytes = text.as_bytes();
        let mut n = bytes.len();
        if n > 0 {
            if let Some(last) = bytes.get(n - 1) {
                if *last == b'\n' {
                    n -= 1;
                }
            }
        }
        let mut i = 0usize;
        while i < n {
            if let Some(b) = bytes.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        bit = DIV_EOFNL;
    } else if kind == "order" {
        // 只把最前面两行对调：集合相同、次序不同，`sort` 之后就"一致"了。
        let bytes = text.as_bytes();
        let mut first_end = 0usize;
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                if *b == b'\n' {
                    first_end = i;
                    break;
                }
            }
            i += 1;
        }
        let mut second_end = bytes.len();
        let mut j = first_end + 1;
        while j < bytes.len() {
            if let Some(b) = bytes.get(j) {
                if *b == b'\n' {
                    second_end = j;
                    break;
                }
            }
            j += 1;
        }
        if first_end > 0 && second_end > first_end {
            let mut k = first_end + 1;
            while k < second_end {
                if let Some(b) = bytes.get(k) {
                    s.push(*b as char);
                }
                k += 1;
            }
            s.push('\n');
            let mut m = 0usize;
            while m < first_end {
                if let Some(b) = bytes.get(m) {
                    s.push(*b as char);
                }
                m += 1;
            }
            s.push('\n');
            let mut p = second_end;
            while p < bytes.len() {
                if let Some(b) = bytes.get(p) {
                    s.push(*b as char);
                }
                p += 1;
            }
            bit = DIV_ORDER;
        }
    } else if kind == "footer" {
        s.push_str(text);
        bit = DIV_FOOTER;
    } else {
        s.push_str(text);
    }
    (s, bit)
}

/// 快照统计：`(行数, 字节数, 最长行宽)`。行数按 `\n` 计，末行无换行也计一行。
pub fn snap_stats(text: &str) -> (u32, u32, u32) {
    let bytes = text.as_bytes();
    let mut lines = 0u32;
    let mut cur = 0u32;
    let mut wide = 0u32;
    let mut i = 0usize;
    let mut any = false;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            any = true;
            if *b == b'\n' {
                lines = lines.saturating_add(1);
                if cur > wide {
                    wide = cur;
                }
                cur = 0;
            } else if *b != b'\r' {
                cur = cur.saturating_add(1);
            }
        }
        i += 1;
    }
    if any && cur > 0 {
        lines = lines.saturating_add(1);
    }
    if cur > wide {
        wide = cur;
    }
    (lines, bytes.len() as u32, wide)
}

// ---------------------------------------------------------------------------
// 域模型枚举
// ---------------------------------------------------------------------------

/// 被快照的 applet（帮助文本的主人）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Applet {
    /// GNU `touch`。
    Touch,
    /// GNU `ls`。
    Ls,
    /// GNU `cp`（多形态 usage，`Usage:` 后两个空格）。
    Cp,
    /// GNU `mkdir`。
    Mkdir,
    /// GNU `cat`。
    Cat,
    /// GNU `echo`（多形态 usage 的第二个样本）。
    Echo,
    /// busybox 本体（全局 usage / `--list` / `--version`）。
    Busybox,
    /// 本卡不认得的 applet：一律无对照面，不许放行。
    Foreign,
}

impl Applet {
    /// applet 名（进页脚与 `Try '...'` 串，必须与快照里的名字逐字相同）。
    pub fn name(&self) -> &'static str {
        match self {
            Applet::Touch => "touch",
            Applet::Ls => "ls",
            Applet::Cp => "cp",
            Applet::Mkdir => "mkdir",
            Applet::Cat => "cat",
            Applet::Echo => "echo",
            Applet::Busybox => "busybox",
            Applet::Foreign => "foreign",
        }
    }

    /// 序号（0..7）。
    pub fn index(&self) -> usize {
        match self {
            Applet::Touch => 0usize,
            Applet::Ls => 1usize,
            Applet::Cp => 2usize,
            Applet::Mkdir => 3usize,
            Applet::Cat => 4usize,
            Applet::Echo => 5usize,
            Applet::Busybox => 6usize,
            Applet::Foreign => 7usize,
        }
    }

    /// 反查。
    pub fn from_index(i: usize) -> Option<Applet> {
        match i {
            0 => Some(Applet::Touch),
            1 => Some(Applet::Ls),
            2 => Some(Applet::Cp),
            3 => Some(Applet::Mkdir),
            4 => Some(Applet::Cat),
            5 => Some(Applet::Echo),
            6 => Some(Applet::Busybox),
            7 => Some(Applet::Foreign),
            _ => None,
        }
    }

    /// applet 全集。
    pub fn all() -> [Applet; 8] {
        [
            Applet::Touch,
            Applet::Ls,
            Applet::Cp,
            Applet::Mkdir,
            Applet::Cat,
            Applet::Echo,
            Applet::Busybox,
            Applet::Foreign,
        ]
    }

    /// 是否 GNU coreutils 家族（帮助页脚有 `Full documentation at:` 三行）。
    pub fn is_gnu(&self) -> bool {
        match self {
            Applet::Touch | Applet::Ls | Applet::Cp | Applet::Mkdir | Applet::Cat | Applet::Echo => {
                true
            }
            Applet::Busybox | Applet::Foreign => false,
        }
    }

    /// 是否有真实快照文本源（`Foreign` 没有，一律无对照面）。
    pub fn has_help_source(&self) -> bool {
        !matches!(self, Applet::Foreign)
    }

    /// 是否多形态 usage（`Usage:` 后两个空格、后面还有 `  or:` 行）。
    pub fn has_alt_forms(&self) -> bool {
        matches!(self, Applet::Cp | Applet::Echo)
    }
}

/// 帮助请求形态（第一道闸的输入）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ask {
    /// `--help`：gnulib 长选项帮助，stdout，码 0。
    HelpLong,
    /// `-h`：busybox 习惯；GNU 多数 applet 不认（不认就是无对照面）。
    HelpShort,
    /// `help APPLET`：busybox 的 help applet。
    HelpApplet,
    /// 无操作数 usage：GNU 退 2、busybox 退 1，两边**本来就不是同一段文本**。
    BareUsage,
    /// `--version`：版本横幅（busybox 档里带构建时刻）。
    VersionLong,
    /// `--list`：busybox 的 applet 清单。
    ListApplets,
    /// `--list --full`：带说明的清单。
    ListFull,
    /// 没有帮助请求：本卡无从比对，直接无对照面。
    None,
}

impl Ask {
    /// 形态名。
    pub fn name(&self) -> &'static str {
        match self {
            Ask::HelpLong => "help-long",
            Ask::HelpShort => "help-short",
            Ask::HelpApplet => "help-applet",
            Ask::BareUsage => "bare-usage",
            Ask::VersionLong => "version-long",
            Ask::ListApplets => "list-applets",
            Ask::ListFull => "list-full",
            Ask::None => "none",
        }
    }

    /// 秩（1..8，与闸门一一对应，但注意：`Ask` 的秩不等于 `Stage` 的秩）。
    pub fn rank(&self) -> u32 {
        match self {
            Ask::HelpLong => 1u32,
            Ask::HelpShort => 2u32,
            Ask::HelpApplet => 3u32,
            Ask::BareUsage => 4u32,
            Ask::VersionLong => 5u32,
            Ask::ListApplets => 6u32,
            Ask::ListFull => 7u32,
            Ask::None => 8u32,
        }
    }

    /// 全集。
    pub fn all() -> [Ask; 8] {
        [
            Ask::HelpLong,
            Ask::HelpShort,
            Ask::HelpApplet,
            Ask::BareUsage,
            Ask::VersionLong,
            Ask::ListApplets,
            Ask::ListFull,
            Ask::None,
        ]
    }

    /// 反查。
    pub fn from_rank(r: u32) -> Option<Ask> {
        match r {
            1 => Some(Ask::HelpLong),
            2 => Some(Ask::HelpShort),
            3 => Some(Ask::HelpApplet),
            4 => Some(Ask::BareUsage),
            5 => Some(Ask::VersionLong),
            6 => Some(Ask::ListApplets),
            7 => Some(Ask::ListFull),
            8 => Some(Ask::None),
            _ => None,
        }
    }

    /// 该形态在**同一台**机器上的应有退出码（帮助面上的真实码，与 applet 分发无关）。
    /// GNU 的 `--help`/`--version` 一律 0；无操作数是 2；busybox 的裸 usage 是 1。
    pub fn own_exit(&self, applet: Applet) -> u32 {
        match self {
            Ask::HelpLong | Ask::HelpApplet | Ask::VersionLong | Ask::ListApplets | Ask::ListFull => {
                0u32
            }
            Ask::HelpShort => {
                if applet.is_gnu() {
                    2u32
                } else {
                    0u32
                }
            }
            Ask::BareUsage => {
                if applet.is_gnu() {
                    2u32
                } else {
                    1u32
                }
            }
            Ask::None => 0u32,
        }
    }

    /// 是否"取的是帮助文本"（决定第 7 闸能不能要求它落 stdout）。
    pub fn is_help_text(&self) -> bool {
        matches!(
            self,
            Ask::HelpLong | Ask::HelpShort | Ask::HelpApplet | Ask::BareUsage | Ask::ListApplets | Ask::ListFull
        )
    }

    /// 该形态两侧必须**同为**这一形态才可比对；`None` 与其余任何形态都不可比。
    pub fn comparable(&self, other: &Ask) -> bool {
        self == other && !matches!(self, Ask::None)
    }
}

/// CONFIG 裁剪档位（与 F11676 的三档同名同序，但本卡只读"帮助里有几行"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Build {
    /// 全功能档。
    Full,
    /// 中等裁剪档（说明文字缩短，选项行仍在）。
    Trim,
    /// `--enable-small` 档（帮助整段被裁，`--list` 也短）。
    Tiny,
    /// 未知档：一律无对照面。
    Unknown,
}

impl Build {
    /// 档名。
    pub fn name(&self) -> &'static str {
        match self {
            Build::Full => "full",
            Build::Trim => "trim",
            Build::Tiny => "tiny",
            Build::Unknown => "unknown",
        }
    }

    /// 秩（0..3）。
    pub fn rank(&self) -> u32 {
        match self {
            Build::Full => 0u32,
            Build::Trim => 1u32,
            Build::Tiny => 2u32,
            Build::Unknown => 3u32,
        }
    }

    /// 全集。
    pub fn all() -> [Build; 4] {
        [Build::Full, Build::Trim, Build::Tiny, Build::Unknown]
    }

    /// 反查。
    pub fn from_rank(r: u32) -> Option<Build> {
        match r {
            0 => Some(Build::Full),
            1 => Some(Build::Trim),
            2 => Some(Build::Tiny),
            3 => Some(Build::Unknown),
            _ => None,
        }
    }

    /// 该档下帮助文本保留的选项行数（Full 5、Trim 3、Tiny 1、Unknown 0）。
    /// 这一维就是本卡第 3 闸的量化落点：**整行消失**，不是变短。
    pub fn option_lines(&self) -> u32 {
        match self {
            Build::Full => 5u32,
            Build::Trim => 3u32,
            Build::Tiny => 1u32,
            Build::Unknown => 0u32,
        }
    }

    /// 是否可能完全没有帮助文本（Tiny 档 `--enable-small` 时 busybox 不内嵌 usage）。
    pub fn help_may_be_absent(&self) -> bool {
        matches!(self, Build::Tiny | Build::Unknown)
    }

    /// 是否合法档（`Unknown` 不合法，进第 4 闸无对照面）。
    pub fn known(&self) -> bool {
        !matches!(self, Build::Unknown)
    }
}

/// 译文档（第五道闸的输入）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Locale {
    /// `LC_ALL=C`：本卡基准档，所有逐字真值都在这档下钉住。
    C,
    /// `LC_ALL=POSIX`：与 C 同文本，但 gettext 走另一条查找链。
    Posix,
    /// `zh_CN.UTF-8`：命中译文，行数可能相同而字节数不同。
    ZhCn,
    /// 未设：由环境继承，快照不可复现——本卡判它"有麻烦"。
    Unset,
}

impl Locale {
    /// 档名。
    pub fn name(&self) -> &'static str {
        match self {
            Locale::C => "C",
            Locale::Posix => "POSIX",
            Locale::ZhCn => "zh_CN.UTF-8",
            Locale::Unset => "unset",
        }
    }

    /// 秩（0..3）。
    pub fn rank(&self) -> u32 {
        match self {
            Locale::C => 0u32,
            Locale::Posix => 1u32,
            Locale::ZhCn => 2u32,
            Locale::Unset => 3u32,
        }
    }

    /// 全集。
    pub fn all() -> [Locale; 4] {
        [Locale::C, Locale::Posix, Locale::ZhCn, Locale::Unset]
    }

    /// 反查。
    pub fn from_rank(r: u32) -> Option<Locale> {
        match r {
            0 => Some(Locale::C),
            1 => Some(Locale::Posix),
            2 => Some(Locale::ZhCn),
            3 => Some(Locale::Unset),
            _ => None,
        }
    }

    /// 是否英文原文档（C/POSIX 下 gnulib 的 `--help` 是同一串 ASCII）。
    pub fn ascii_source(&self) -> bool {
        matches!(self, Locale::C | Locale::Posix)
    }

    /// 是否"可复现档"：未设 locales 的快照本卡不认。
    pub fn reproducible(&self) -> bool {
        !matches!(self, Locale::Unset)
    }

    /// 译文膨胀系数（千分率）：中文短、德文长，本卡用它把"字节数不同"
    /// 与"内容不同"分开——系数只用于场景族的合法性检查，不进判据。
    pub fn len_permille(&self) -> u32 {
        match self {
            Locale::C => 1000u32,
            Locale::Posix => 1000u32,
            Locale::ZhCn => 780u32,
            Locale::Unset => 1000u32,
        }
    }
}

/// 比对方式（本卡最容易被做假的一维）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Compare {
    /// 逐字：`cmp` / `diff` 不带忽略选项。唯一可信档。
    Verbatim,
    /// 忽略空白：`diff -w` / `diff -B`——把行尾空白与折行漂移一起吃掉。
    IgnoreSpace,
    /// 只比行数：`wc -l` 相等就报一致——字节漂移全漏。
    LineCountOnly,
    /// 跳过比对：直接抄基准机结论。本卡判据在这一档**根本不该存在**。
    Skip,
}

impl Compare {
    /// 档名。
    pub fn name(&self) -> &'static str {
        match self {
            Compare::Verbatim => "verbatim",
            Compare::IgnoreSpace => "ignore-space",
            Compare::LineCountOnly => "line-count",
            Compare::Skip => "skip",
        }
    }

    /// 秩（0..3）。
    pub fn rank(&self) -> u32 {
        match self {
            Compare::Verbatim => 0u32,
            Compare::IgnoreSpace => 1u32,
            Compare::LineCountOnly => 2u32,
            Compare::Skip => 3u32,
        }
    }

    /// 全集。
    pub fn all() -> [Compare; 4] {
        [
            Compare::Verbatim,
            Compare::IgnoreSpace,
            Compare::LineCountOnly,
            Compare::Skip,
        ]
    }

    /// 反查。
    pub fn from_rank(r: u32) -> Option<Compare> {
        match r {
            0 => Some(Compare::Verbatim),
            1 => Some(Compare::IgnoreSpace),
            2 => Some(Compare::LineCountOnly),
            3 => Some(Compare::Skip),
            _ => None,
        }
    }

    /// 是否可信（只有逐字档可信）。
    pub fn trustworthy(&self) -> bool {
        matches!(self, Compare::Verbatim)
    }

    /// 是否会**吞掉**某一类真实漂移（本卡用它证明"归一化不比"危害的具体落点）。
    /// 返回被吞的差异位掩码；逐字档返回 0。
    pub fn swallows(&self) -> u16 {
        match self {
            Compare::Verbatim => 0u16,
            Compare::IgnoreSpace => DIV_CRLF | DIV_TRAILING | DIV_WRAP | DIV_EOFNL,
            Compare::LineCountOnly => {
                DIV_CRLF
                    | DIV_TRAILING
                    | DIV_WRAP
                    | DIV_LOCALE
                    | DIV_NAME
                    | DIV_STAMP
                    | DIV_FOOTER
                    | DIV_HINT
                    | DIV_EOFNL
            }
            Compare::Skip => DIV_CRLF
                | DIV_TRAILING
                | DIV_WRAP
                | DIV_TRIM
                | DIV_LOCALE
                | DIV_NAME
                | DIV_ORDER
                | DIV_STAMP
                | DIV_FOOTER
                | DIV_HINT
                | DIV_EOFNL
                | DIV_CASE,
        }
    }
}

/// 落流档（第七道闸的输入，与 F11678 联签）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stream {
    /// 两侧帮助都落 stdout（唯一放行档）。
    BothStdout,
    /// 目标机把帮助全文落进了 stderr（busybox 的部分 applet 真会这样）。
    TargetStderr,
    /// 基准机落错流。
    BaseStderr,
    /// 两机合流（`2>&1` 抓在一起）：无法分离错误与帮助。
    Merged,
    /// 一侧根本没写字（进程被信号打断、help 编掉）。
    Silent,
}

impl Stream {
    /// 档名。
    pub fn name(&self) -> &'static str {
        match self {
            Stream::BothStdout => "both-stdout",
            Stream::TargetStderr => "target-stderr",
            Stream::BaseStderr => "base-stderr",
            Stream::Merged => "merged",
            Stream::Silent => "silent",
        }
    }

    /// 秩（0..4）。
    pub fn rank(&self) -> u32 {
        match self {
            Stream::BothStdout => 0u32,
            Stream::TargetStderr => 1u32,
            Stream::BaseStderr => 2u32,
            Stream::Merged => 3u32,
            Stream::Silent => 4u32,
        }
    }

    /// 全集。
    pub fn all() -> [Stream; 5] {
        [
            Stream::BothStdout,
            Stream::TargetStderr,
            Stream::BaseStderr,
            Stream::Merged,
            Stream::Silent,
        ]
    }

    /// 反查。
    pub fn from_rank(r: u32) -> Option<Stream> {
        match r {
            0 => Some(Stream::BothStdout),
            1 => Some(Stream::TargetStderr),
            2 => Some(Stream::BaseStderr),
            3 => Some(Stream::Merged),
            4 => Some(Stream::Silent),
            _ => None,
        }
    }

    /// 帮助全文是否落在该落的地方（F11678 的结论在本卡的读法）。
    pub fn help_ok(&self) -> bool {
        matches!(self, Stream::BothStdout)
    }

    /// 是否"根本没文本"档（本卡第 4 闸用）。
    pub fn no_text(&self) -> bool {
        matches!(self, Stream::Silent)
    }

    /// 是否让本次比对作废（落错流或合流：拿到的是混着的串）。
    pub fn voids_compare(&self) -> bool {
        matches!(self, Stream::TargetStderr | Stream::BaseStderr | Stream::Merged)
    }
}

/// 观测纵线的八道闸（秩 1..8）。本卡的秩**只能前进**。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 秩 1：请求形态闸——两边要的是不是同一种文本。
    Ask,
    /// 秩 2：程序名闸——`argv[0]` 基名进首行与页脚。
    Name,
    /// 秩 3：编译档位闸——裁剪让整行消失。
    Cfg,
    /// 秩 4：文本源闸——一侧到底有没有产出文本。
    Text,
    /// 秩 5：译文档闸——gettext 档是否一致。
    Locale,
    /// 秩 6：行宽闸——`COLUMNS` 是否一致。
    Wrap,
    /// 秩 7：落流闸——帮助是否落在 stdout 且已冲刷。
    Stream,
    /// 秩 8：快照封印闸——逐字比对与一致锚。
    Snap,
}

impl Stage {
    /// 秩（1..8）。
    pub fn rank(&self) -> u32 {
        match self {
            Stage::Ask => 1u32,
            Stage::Name => 2u32,
            Stage::Cfg => 3u32,
            Stage::Text => 4u32,
            Stage::Locale => 5u32,
            Stage::Wrap => 6u32,
            Stage::Stream => 7u32,
            Stage::Snap => 8u32,
        }
    }

    /// 闸名（进渲染，逐字稳定）。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Ask => "ask",
            Stage::Name => "name",
            Stage::Cfg => "cfg",
            Stage::Text => "text",
            Stage::Locale => "locale",
            Stage::Wrap => "wrap",
            Stage::Stream => "stream",
            Stage::Snap => "snap",
        }
    }

    /// 全集（按秩升序）。
    pub fn all() -> [Stage; 8] {
        [
            Stage::Ask,
            Stage::Name,
            Stage::Cfg,
            Stage::Text,
            Stage::Locale,
            Stage::Wrap,
            Stage::Stream,
            Stage::Snap,
        ]
    }

    /// 反查秩。
    pub fn from_rank(r: u32) -> Option<Stage> {
        match r {
            1 => Some(Stage::Ask),
            2 => Some(Stage::Name),
            3 => Some(Stage::Cfg),
            4 => Some(Stage::Text),
            5 => Some(Stage::Locale),
            6 => Some(Stage::Wrap),
            7 => Some(Stage::Stream),
            8 => Some(Stage::Snap),
            _ => None,
        }
    }

    /// 第一道闸。
    pub const fn first() -> Stage {
        Stage::Ask
    }

    /// 最后一道闸（封印闸）。
    pub const fn last() -> Stage {
        Stage::Snap
    }

    /// 是否封印闸。
    pub fn is_terminal(&self) -> bool {
        matches!(self, Stage::Snap)
    }

    /// 是否"这次压根没文本可比"的两道闸（请求形态与文本源）。
    /// 这两道闸产出的账不进逐字比对，落 2 档（有麻烦）。
    pub fn untouched(&self) -> bool {
        matches!(self, Stage::Ask | Stage::Text)
    }

    /// 是否前提类闸（名字/档位/译文/宽度）：它们判的是"两边是不是同一条文本"。
    pub fn precondition(&self) -> bool {
        matches!(self, Stage::Name | Stage::Cfg | Stage::Locale | Stage::Wrap)
    }

    /// 下一秩；封印闸没有下一秩。
    pub fn next(&self) -> Option<Stage> {
        Stage::from_rank(self.rank().saturating_add(1))
    }
}

/// 差异类别（注解维）。一条账只挂**一个**主类别，其余差异并进 `value` 计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// 逐字全等——本卡唯一亮一致锚的类别。
    Identical,
    /// 无对照面：一侧或两侧没有文本。
    NoText,
    /// 两侧请求形态不同，取的本来就不是同一段文本。
    NotComparable,
    /// 程序名漂移（`argv[0]` 基名不同）。
    ProgramName,
    /// 裁剪档位漂移（选项行整行消失）。
    TrimmedOption,
    /// 译文漂移（locale 档不同）。
    LocaleDrift,
    /// 折行宽度漂移（`COLUMNS` 不同）。
    WrapDrift,
    /// 落流漂移（help 进了 stderr / 合流 / 未冲刷）。
    StreamSwap,
    /// 构建时间戳漂移（版本横幅里那一串时刻）。
    StampDrift,
    /// 清单排序漂移（集合相同、次序不同）。
    ListOrder,
    /// 页脚漂移（`Full documentation at:` / `info '(coreutils) …'`）。
    FooterDrift,
    /// 提示行漂移（`Try '…  --help' for more information.` 有无）。
    HintDrift,
    /// 行尾空白漂移。
    TrailingBlank,
    /// 换行符档漂移（`\n` 对 `\r\n`）。
    EolStyle,
    /// 末行换行漂移。
    EofNewline,
    /// 别名大小写漂移（`Ls` 与 `ls`）。
    CaseAlias,
    /// 比对方式不可信（忽略空白/只比行数/跳过）而现场确有差异。
    FuzzyMask,
    /// 字节数/行数不同却没挂出任何差异维（只比了行数的典型露馅）。
    ByteDrift,
}

impl Note {
    /// 类别名（进渲染与指纹）。
    pub fn name(&self) -> &'static str {
        match self {
            Note::Identical => "identical",
            Note::NoText => "no-text",
            Note::NotComparable => "not-comparable",
            Note::ProgramName => "program-name",
            Note::TrimmedOption => "trimmed-option",
            Note::LocaleDrift => "locale-drift",
            Note::WrapDrift => "wrap-drift",
            Note::StreamSwap => "stream-swap",
            Note::StampDrift => "stamp-drift",
            Note::ListOrder => "list-order",
            Note::FooterDrift => "footer-drift",
            Note::HintDrift => "hint-drift",
            Note::TrailingBlank => "trailing-blank",
            Note::EolStyle => "eol-style",
            Note::EofNewline => "eof-newline",
            Note::CaseAlias => "case-alias",
            Note::FuzzyMask => "fuzzy-mask",
            Note::ByteDrift => "byte-drift",
        }
    }

    /// 秩（0..17）。
    pub fn rank(&self) -> u32 {
        match self {
            Note::Identical => 0u32,
            Note::NoText => 1u32,
            Note::NotComparable => 2u32,
            Note::ProgramName => 3u32,
            Note::TrimmedOption => 4u32,
            Note::LocaleDrift => 5u32,
            Note::WrapDrift => 6u32,
            Note::StreamSwap => 7u32,
            Note::StampDrift => 8u32,
            Note::ListOrder => 9u32,
            Note::FooterDrift => 10u32,
            Note::HintDrift => 11u32,
            Note::TrailingBlank => 12u32,
            Note::EolStyle => 13u32,
            Note::EofNewline => 14u32,
            Note::CaseAlias => 15u32,
            Note::FuzzyMask => 16u32,
            Note::ByteDrift => 17u32,
        }
    }

    /// 全集（按秩升序）。
    pub fn all() -> [Note; 18] {
        [
            Note::Identical,
            Note::NoText,
            Note::NotComparable,
            Note::ProgramName,
            Note::TrimmedOption,
            Note::LocaleDrift,
            Note::WrapDrift,
            Note::StreamSwap,
            Note::StampDrift,
            Note::ListOrder,
            Note::FooterDrift,
            Note::HintDrift,
            Note::TrailingBlank,
            Note::EolStyle,
            Note::EofNewline,
            Note::CaseAlias,
            Note::FuzzyMask,
            Note::ByteDrift,
        ]
    }

    /// 反查。
    pub fn from_rank(r: u32) -> Option<Note> {
        match r {
            0 => Some(Note::Identical),
            1 => Some(Note::NoText),
            2 => Some(Note::NotComparable),
            3 => Some(Note::ProgramName),
            4 => Some(Note::TrimmedOption),
            5 => Some(Note::LocaleDrift),
            6 => Some(Note::WrapDrift),
            7 => Some(Note::StreamSwap),
            8 => Some(Note::StampDrift),
            9 => Some(Note::ListOrder),
            10 => Some(Note::FooterDrift),
            11 => Some(Note::HintDrift),
            12 => Some(Note::TrailingBlank),
            13 => Some(Note::EolStyle),
            14 => Some(Note::EofNewline),
            15 => Some(Note::CaseAlias),
            16 => Some(Note::FuzzyMask),
            17 => Some(Note::ByteDrift),
            _ => None,
        }
    }

    /// 是否"逐字全等"。
    pub fn identical(&self) -> bool {
        matches!(self, Note::Identical)
    }

    /// 是否无对照面（前置闸，非内容差异）。
    pub fn no_text(&self) -> bool {
        matches!(self, Note::NoText | Note::NotComparable)
    }

    /// 是否内容类漂移（能点亮"诚实冲突"的那些）。
    pub fn drift(&self) -> bool {
        matches!(
            self,
            Note::ProgramName
                | Note::TrimmedOption
                | Note::LocaleDrift
                | Note::WrapDrift
                | Note::StampDrift
                | Note::ListOrder
                | Note::FooterDrift
                | Note::HintDrift
                | Note::TrailingBlank
                | Note::EolStyle
                | Note::EofNewline
                | Note::CaseAlias
                | Note::ByteDrift
        )
    }

    /// 是否比对方式类（掩盖档专用）。
    pub fn masked_class(&self) -> bool {
        matches!(self, Note::FuzzyMask | Note::ByteDrift)
    }

    /// 该类别对应的差异位；无对应位返回 0。
    pub fn drift_bit(&self) -> u16 {
        match self {
            Note::ProgramName => DIV_NAME,
            Note::TrimmedOption => DIV_TRIM,
            Note::LocaleDrift => DIV_LOCALE,
            Note::WrapDrift => DIV_WRAP,
            Note::StampDrift => DIV_STAMP,
            Note::ListOrder => DIV_ORDER,
            Note::FooterDrift => DIV_FOOTER,
            Note::HintDrift => DIV_HINT,
            Note::TrailingBlank => DIV_TRAILING,
            Note::EolStyle => DIV_CRLF,
            Note::EofNewline => DIV_EOFNL,
            Note::CaseAlias => DIV_CASE,
            _ => 0u16,
        }
    }

    /// 该类别落在哪道闸（与 `define()` 的闸序一致，谓词据此对账）。
    pub fn home_stage(&self) -> Stage {
        match self {
            Note::Identical => Stage::Snap,
            Note::NotComparable => Stage::Ask,
            Note::NoText => Stage::Text,
            Note::ProgramName => Stage::Name,
            Note::TrimmedOption => Stage::Cfg,
            Note::LocaleDrift => Stage::Locale,
            Note::WrapDrift => Stage::Wrap,
            Note::StreamSwap => Stage::Stream,
            Note::StampDrift => Stage::Snap,
            Note::ListOrder => Stage::Snap,
            Note::FooterDrift => Stage::Snap,
            Note::HintDrift => Stage::Snap,
            Note::TrailingBlank => Stage::Snap,
            Note::EolStyle => Stage::Snap,
            Note::EofNewline => Stage::Snap,
            Note::CaseAlias => Stage::Snap,
            Note::FuzzyMask => Stage::Snap,
            Note::ByteDrift => Stage::Snap,
        }
    }

    /// 是否本批判据原文里点名的"逐字"类别（一致锚只认这一条）。
    pub fn verbatim_class(&self) -> bool {
        matches!(self, Note::Identical)
    }
}

/// 四档：一致锚、诚实冲突、无对照面、掩盖。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 一档：逐字全等，一致锚亮。
    Verbatim,
    /// 二档：诚实冲突——确有漂移且被如实挂出。
    Conflict,
    /// 三档：无对照面——没文本可比，判不了逐字。
    NoText,
    /// 四档：掩盖——比对方式不可信或现场自相矛盾，判死。
    Masked,
}

impl Tier {
    /// 按实际结论归档。顺序：先拒掩盖（结构不一致），再分无对照面/全等/诚实冲突。
    pub fn of(o: &Outcome) -> Tier {
        if !o.consistent {
            return Tier::Masked;
        }
        if o.note.no_text() {
            return Tier::NoText;
        }
        if o.ok() {
            return Tier::Verbatim;
        }
        Tier::Conflict
    }

    /// 档位序号（0..3）。
    pub fn index(&self) -> usize {
        match self {
            Tier::Verbatim => 0usize,
            Tier::Conflict => 1usize,
            Tier::NoText => 2usize,
            Tier::Masked => 3usize,
        }
    }

    /// 反查档位。
    pub fn from_index(i: usize) -> Option<Tier> {
        match i {
            0 => Some(Tier::Verbatim),
            1 => Some(Tier::Conflict),
            2 => Some(Tier::NoText),
            3 => Some(Tier::Masked),
            _ => None,
        }
    }

    /// 档位名。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Verbatim => "verbatim",
            Tier::Conflict => "conflict",
            Tier::NoText => "no-text",
            Tier::Masked => "masked",
        }
    }

    /// 是否成功侧。
    pub fn success(&self) -> bool {
        matches!(self, Tier::Verbatim)
    }

    /// 是否允许亮一致锚（只有全等一档）。
    pub fn anchored(&self) -> bool {
        matches!(self, Tier::Verbatim)
    }

    /// 是否必须判死的档。
    pub fn is_masked(&self) -> bool {
        matches!(self, Tier::Masked)
    }

    /// 全集。
    pub fn all() -> [Tier; 4] {
        [Tier::Verbatim, Tier::Conflict, Tier::NoText, Tier::Masked]
    }
}

/// judge 的返回：六个缺陷维 + 全对。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 六维全对。
    Ok,
    /// 成败互报：申报绿而实际有漂移（或反之）。
    Estate,
    /// 阶段错位：漂移落在哪道闸说错了。
    Stage,
    /// 比对码错位：0/1/2 互换。
    Code,
    /// 差异项数错位。
    Value,
    /// 差异类别错位。
    Note,
    /// 一致锚错位。
    Default,
}

impl Fault {
    /// 缺陷名。
    pub fn name(&self) -> &'static str {
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

    /// 该缺陷维对应的本卡错误码。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0u32,
            Fault::Estate => HD_ESTATE,
            Fault::Stage => HD_ESTAGE,
            Fault::Code => HD_ECODE,
            Fault::Value => HD_EVALUE,
            Fault::Note => HD_ENOTE,
            Fault::Default => HD_EDEFAULT,
        }
    }

    /// 是否已判死（非 `Ok`）。
    pub fn judged(&self) -> bool {
        !matches!(self, Fault::Ok)
    }

    /// 缺陷序（1..6，与判序一致）；`Ok` 为 0。
    pub fn index(&self) -> usize {
        match self {
            Fault::Ok => 0usize,
            Fault::Estate => 1usize,
            Fault::Stage => 2usize,
            Fault::Code => 3usize,
            Fault::Value => 4usize,
            Fault::Note => 5usize,
            Fault::Default => 6usize,
        }
    }

    /// 全集（判序顺序）。
    pub fn all() -> [Fault; 7] {
        [
            Fault::Ok,
            Fault::Estate,
            Fault::Stage,
            Fault::Code,
            Fault::Value,
            Fault::Note,
            Fault::Default,
        ]
    }
}

/// 差异位计数（不递归、不依赖外部 crate）。
fn bit_count(bits: u16) -> u32 {
    let mut v = bits;
    let mut n = 0u32;
    let mut k = 0usize;
    while k < 16 {
        if v & 0x0001 != 0 {
            n = n.saturating_add(1);
        }
        v >>= 1;
        k += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// 域模型：形状（一次双机快照的全部输入维）
// ---------------------------------------------------------------------------

/// 一次双机快照的现场。两台机器各自的事实**分开记**，
/// 任何"合并成一侧"的写法都会让本卡判据失效。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// 被快照的 applet。
    pub applet: Applet,
    /// 基准机的请求形态。
    pub base_ask: Ask,
    /// 目标机的请求形态。
    pub target_ask: Ask,
    /// 基准机 `argv[0]` 基名。
    pub base_name: &'static str,
    /// 目标机 `argv[0]` 基名。
    pub target_name: &'static str,
    /// 基准机 CONFIG 档。
    pub base_build: Build,
    /// 目标机 CONFIG 档。
    pub target_build: Build,
    /// 基准机 locale。
    pub base_locale: Locale,
    /// 目标机 locale。
    pub target_locale: Locale,
    /// 基准机 `COLUMNS`（0 表示未导出）。
    pub base_cols: u32,
    /// 目标机 `COLUMNS`。
    pub target_cols: u32,
    /// 基准机快照行数。
    pub base_lines: u32,
    /// 目标机快照行数。
    pub target_lines: u32,
    /// 基准机快照字节数。
    pub base_bytes: u32,
    /// 目标机快照字节数。
    pub target_bytes: u32,
    /// 首个差异行号（1 起）；0 表示没定位到差异。
    pub first_diff: u32,
    /// 申报的差异位图（现场事实；与前提维不一致时按位复算）。
    pub diverg: u16,
    /// 比对方式。
    pub compare: Compare,
    /// 落流档。
    pub stream: Stream,
    /// 取快照前是否冲刷过 stdio（未冲刷的快照可能是半截串）。
    pub flushed: bool,
    /// 基准机是否产出文本。
    pub text_base: bool,
    /// 目标机是否产出文本。
    pub text_target: bool,
    /// 基准机这次帮助调用的退出码。
    pub base_exit: u32,
    /// 目标机这次帮助调用的退出码。
    pub target_exit: u32,
    /// 请求位图（`--help`/`-h`/`-s` 等）。
    pub opts: u8,
}

impl Shape {
    /// 空形状：一块全绿的 GNU `touch --help` 双机一致快照，供组装基型用。
    pub const fn blank() -> Shape {
        Shape {
            applet: Applet::Touch,
            base_ask: Ask::HelpLong,
            target_ask: Ask::HelpLong,
            base_name: "touch",
            target_name: "touch",
            base_build: Build::Full,
            target_build: Build::Full,
            base_locale: Locale::C,
            target_locale: Locale::C,
            base_cols: COLS_DEFAULT,
            target_cols: COLS_DEFAULT,
            base_lines: 9u32,
            target_lines: 9u32,
            base_bytes: 420u32,
            target_bytes: 420u32,
            first_diff: 0u32,
            diverg: 0u16,
            compare: Compare::Verbatim,
            stream: Stream::BothStdout,
            flushed: true,
            text_base: true,
            text_target: true,
            base_exit: 0u32,
            target_exit: 0u32,
            opts: OPT_HELP_LONG,
        }
    }

    /// 取某位请求选项。
    pub const fn opt_has(&self, bit: u8) -> bool {
        self.opts & bit != 0
    }

    /// 未知请求位。
    pub const fn unknown_opt_bits(&self) -> u8 {
        self.opts & !OPT_KNOWN
    }

    /// 未知差异位。
    pub const fn unknown_div_bits(&self) -> u16 {
        self.diverg & DIV_UNKNOWN_MASK
    }

    /// 复算后的有效差异位：前提维（名字/档位/译文/宽度）一旦不同就**必然**带位。
    /// 语料里漏挂位不算漏——现场本身就把位补上，谎报（挂了位而前提相同）
    /// 才由 `self_conflict()` 判死。
    pub fn diverg_effective(&self) -> u16 {
        let mut b = self.diverg & DIV_KNOWN;
        if self.base_name != self.target_name {
            b |= DIV_NAME;
        }
        if self.base_build != self.target_build {
            b |= DIV_TRIM;
        }
        if self.base_locale != self.target_locale {
            b |= DIV_LOCALE;
        }
        if self.base_cols != self.target_cols {
            b |= DIV_WRAP;
        }
        b
    }

    /// 差异项数（`value` 维）。
    pub fn diverg_count(&self) -> u32 {
        bit_count(self.diverg_effective())
    }

    /// 两侧请求形态是否可比。
    pub fn asks_comparable(&self) -> bool {
        self.base_ask.comparable(&self.target_ask)
    }

    /// 是否任一侧没产出文本。
    pub fn any_text_absent(&self) -> bool {
        !self.text_base || !self.text_target
    }

    /// 两侧是否都有文本。
    pub fn both_text(&self) -> bool {
        self.text_base && self.text_target
    }

    /// 行数合计（谓词用来核对快照体量）。
    pub fn line_sum(&self) -> u32 {
        self.base_lines.saturating_add(self.target_lines)
    }

    /// 字节差（绝对值）。
    pub fn byte_delta(&self) -> u32 {
        if self.base_bytes > self.target_bytes {
            self.base_bytes.saturating_sub(self.target_bytes)
        } else {
            self.target_bytes.saturating_sub(self.base_bytes)
        }
    }

    /// 行数是否同。
    pub fn lines_agree(&self) -> bool {
        self.base_lines == self.target_lines
    }

    /// 字节数是否同。
    pub fn bytes_agree(&self) -> bool {
        self.base_bytes == self.target_bytes
    }

    /// 前提四档（名字/档位/译文/宽度）是否全同。
    pub fn preconditions_agree(&self) -> bool {
        self.base_name == self.target_name
            && self.base_build == self.target_build
            && self.base_locale == self.target_locale
            && self.base_cols == self.target_cols
    }

    /// 落流是否可用来做快照（走 stdout 且已冲刷）。
    pub fn stream_usable(&self) -> bool {
        self.stream.help_ok() && self.flushed
    }

    /// 现场是否自相矛盾（谎报）。任何一条命中就把实际结论压进"掩盖"档。
    pub fn self_conflict(&self) -> bool {
        let b = self.diverg_effective();
        // 挂了名字位而两侧名字其实相同。
        if self.diverg & DIV_NAME != 0 && self.base_name == self.target_name {
            return true;
        }
        // 挂了裁剪位而档位其实相同。
        if self.diverg & DIV_TRIM != 0 && self.base_build == self.target_build {
            return true;
        }
        // 挂了译工位而 locale 其实相同。
        if self.diverg & DIV_LOCALE != 0 && self.base_locale == self.target_locale {
            return true;
        }
        // 挂了宽度位而列数其实相同。
        if self.diverg & DIV_WRAP != 0 && self.base_cols == self.target_cols {
            return true;
        }
        // 有差异却没定位首差行（"归一化之后就没差异了"的典型）。
        if b != 0 && self.first_diff == 0 {
            return true;
        }
        // 定位了首差行却没有任何差异维。
        if self.first_diff != 0 && b == 0 {
            return true;
        }
        // 首差行越出文本范围。
        let hi = if self.base_lines > self.target_lines {
            self.base_lines
        } else {
            self.target_lines
        };
        if self.first_diff > hi {
            return true;
        }
        // 比对方式不可信而现场确有差异。
        if !self.compare.trustworthy() && b != 0 {
            return true;
        }
        // 跳过比对：只要两侧都有文本，"跳过"本身就没有资格充当证据。
        if self.compare == Compare::Skip && self.both_text() {
            return true;
        }
        // 声明两侧都有文本，却落在"根本没写"这一流档。
        if self.both_text() && self.stream.no_text() {
            return true;
        }
        // 退出码 0 却没有文本产出。
        if self.base_exit == 0 && !self.text_base {
            return true;
        }
        if self.target_exit == 0 && !self.text_target {
            return true;
        }
        // 没差异、没漂移位，行列数却不同（只比了行数的典型露馅）。
        if b == 0
            && self.first_diff == 0
            && self.both_text()
            && (!self.lines_agree() || !self.bytes_agree())
        {
            return true;
        }
        false
    }

    /// 形状是否落在本卡认得的合法域内。
    pub fn legal(&self) -> bool {
        if self.unknown_opt_bits() != 0 {
            return false;
        }
        if self.unknown_div_bits() != 0 {
            return false;
        }
        if self.base_lines > LINE_CEIL || self.target_lines > LINE_CEIL {
            return false;
        }
        if self.base_bytes == 0 && self.text_base {
            return false;
        }
        if self.target_bytes == 0 && self.text_target {
            return false;
        }
        if self.first_diff > LINE_CEIL {
            return false;
        }
        if bit_count(self.diverg & DIV_KNOWN) > DIV_MAX_Kinds {
            return false;
        }
        if self.base_exit > 2 || self.target_exit > 2 {
            return false;
        }
        true
    }

    /// 逐字全等的必要条件（不含一致锚，锚还要过比对档与落流档）。
    pub fn looks_identical(&self) -> bool {
        self.asks_comparable()
            && self.preconditions_agree()
            && self.both_text()
            && self.bit_only_premise() == 0
            && self.first_diff == 0
            && self.lines_agree()
            && self.bytes_agree()
    }

    /// 剥掉前提位之后剩下的差异位（封印闸读的就是这个）。
    pub fn bit_only_premise(&self) -> u16 {
        let b = self.diverg & DIV_KNOWN;
        b & !(DIV_NAME | DIV_TRIM | DIV_LOCALE | DIV_WRAP)
    }
}

/// 一格的**实际**结论（本卡引擎算出来的账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 落点闸。
    pub stage: Stage,
    /// 比对码（diff 的 0/1/2）。
    pub code: i32,
    /// 差异项数。
    pub value: u32,
    /// 差异类别。
    pub note: Note,
    /// 一致锚：只有逐字全等才亮。
    pub default: bool,
    /// 结构自洽：谎报、归一化掩盖等都把它压成 `false`。
    pub consistent: bool,
}

impl Outcome {
    /// 全零初值。
    pub const fn blank() -> Outcome {
        Outcome {
            stage: Stage::Snap,
            code: CODE_SAME,
            value: 0u32,
            note: Note::Identical,
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

    /// 是否绿（逐字全等）。**码 0 单独不算绿**：还要求零差异且自洽。
    pub fn ok(&self) -> bool {
        self.code == CODE_SAME && self.value == 0 && self.consistent
    }

    /// 是否红（有差异或无法比对）。
    pub fn failed(&self) -> bool {
        self.code != CODE_SAME || self.value != 0
    }

    /// 是否亮了本卡的一致锚。
    pub fn anchored(&self) -> bool {
        self.default
    }

    /// 是否谎报（结构不自洽）。
    pub fn refused(&self) -> bool {
        !self.consistent
    }

    /// 是否有对照面（能谈"逐字"的前提）。
    pub fn comparable(&self) -> bool {
        !self.note.no_text()
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
    /// 申报的比对码。
    pub code: i32,
    /// 申报的差异项数。
    pub value: u32,
    /// 申报的差异类别。
    pub note: Note,
    /// 申报的一致锚。
    pub default: bool,
}

impl Claim {
    /// 全零初值。
    pub const fn blank() -> Claim {
        Claim {
            stage: Stage::Snap,
            code: CODE_SAME,
            value: 0u32,
            note: Note::Identical,
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

    /// 申报读成绿还是红。**只看码**——这正是本卡要打的假：码 0 不等于逐字一致。
    pub fn success(&self) -> bool {
        self.code == CODE_SAME
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

/// 一格语料：现场形状 + 申报 + 短名。
#[derive(Clone, Copy, Debug)]
pub struct Case {
    /// 输入形状。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
    /// 短名（`f11677-<slug>`，标签必须是本卡的）。
    pub tag: &'static str,
}

impl Case {
    /// 空语料格。
    pub const fn blank() -> Case {
        Case {
            shape: Shape::blank(),
            claim: Claim::blank(),
            tag: "f11677-blank",
        }
    }

    /// 实际结论。
    pub fn actual(&self) -> Outcome {
        define(&self.shape)
    }

    /// 本格的缺陷维。
    pub fn fault(&self) -> Fault {
        judge(self)
    }

    /// 是否申报正确。
    pub fn accepted(&self) -> bool {
        matches!(self.fault(), Fault::Ok)
    }

    /// 该格的期望缺陷（活格为 `Ok`，死格按 WANT_NDX 指定）。
    pub fn expected_fault(&self, i: usize) -> Option<Fault> {
        expected(i)
    }
}

/// 由申报反推一份"诚实申报"（谓词与场景族用它构造正例，杜绝手抄笔误）。
pub fn claim_of(o: &Outcome) -> Claim {
    Claim {
        stage: o.stage,
        code: o.code,
        value: o.value,
        note: o.note,
        default: o.default,
    }
}

// ---------------------------------------------------------------------------
// 引擎：八闸纵线
// ---------------------------------------------------------------------------

/// 封印闸内按固定优先级挑主类别（先重后轻；时间戳排在最前，
/// 因为它是"双机永远不可能逐字相等"的那一维，必须先挂出来）。
fn snap_note(s: &Shape) -> Note {
    let b = s.diverg_effective();
    if s.compare == Compare::Skip {
        return Note::FuzzyMask;
    }
    if !s.compare.trustworthy() && b != 0 {
        return Note::FuzzyMask;
    }
    if b & DIV_STAMP != 0 {
        return Note::StampDrift;
    }
    if b & DIV_ORDER != 0 {
        return Note::ListOrder;
    }
    if b & DIV_FOOTER != 0 {
        return Note::FooterDrift;
    }
    if b & DIV_HINT != 0 {
        return Note::HintDrift;
    }
    if b & DIV_TRAILING != 0 {
        return Note::TrailingBlank;
    }
    if b & DIV_CRLF != 0 {
        return Note::EolStyle;
    }
    if b & DIV_EOFNL != 0 {
        return Note::EofNewline;
    }
    if b & DIV_CASE != 0 {
        return Note::CaseAlias;
    }
    if b == 0 && (!s.lines_agree() || !s.bytes_agree()) {
        return Note::ByteDrift;
    }
    Note::Identical
}

/// **闸门纵线**：八道闸按秩 1..8 顺序过，第一道不成立的闸就是这格的落点。
///
/// 每一道闸产出的三元组（码档、类别、是否亮锚）都由该闸自己的语义定死：
/// 前置类闸落 1 档（确有不同）、无对照面闸落 2 档（没法比）、
/// 只有封印闸在无差异时才落 0 档并点亮一致锚。
pub fn define(s: &Shape) -> Outcome {
    let conflicted = s.self_conflict();
    let value = s.diverg_count();

    // 秩 1：请求形态闸。
    if !s.applet.has_help_source() || !s.asks_comparable() {
        return Outcome::make(Stage::Ask, CODE_TROUBLE, 0, Note::NotComparable, false, !conflicted);
    }

    // 秩 2：程序名闸。
    if s.base_name != s.target_name || s.diverg & DIV_NAME != 0 {
        return Outcome::make(Stage::Name, CODE_DIFF, value, Note::ProgramName, false, !conflicted);
    }

    // 秩 3：编译档位闸。
    if s.base_build != s.target_build
        || !s.base_build.known()
        || !s.target_build.known()
        || s.diverg & DIV_TRIM != 0
    {
        return Outcome::make(Stage::Cfg, CODE_DIFF, value, Note::TrimmedOption, false, !conflicted);
    }

    // 秩 4：文本源闸（没有对照面，后面所有口径都不成立）。
    if s.any_text_absent() || s.stream.no_text() {
        return Outcome::make(Stage::Text, CODE_TROUBLE, 0, Note::NoText, false, !conflicted);
    }

    // 秩 5：译文档闸。
    if s.base_locale != s.target_locale
        || !s.base_locale.reproducible()
        || !s.target_locale.reproducible()
        || s.diverg & DIV_LOCALE != 0
    {
        return Outcome::make(Stage::Locale, CODE_DIFF, value, Note::LocaleDrift, false, !conflicted);
    }

    // 秩 6：行宽闸。
    if s.base_cols != s.target_cols || s.diverg & DIV_WRAP != 0 {
        return Outcome::make(Stage::Wrap, CODE_DIFF, value, Note::WrapDrift, false, !conflicted);
    }

    // 秩 7：落流闸（与 F11678 联签；落错流或没冲刷 ⇒ 本次快照作废）。
    if !s.stream_usable() {
        return Outcome::make(Stage::Stream, CODE_TROUBLE, value, Note::StreamSwap, false, !conflicted);
    }

    // 秩 8：快照封印闸。
    let note = snap_note(s);
    if note == Note::Identical && s.compare.trustworthy() && !conflicted {
        return Outcome::make(Stage::Snap, CODE_SAME, 0, Note::Identical, true, true);
    }
    let code = if conflicted && !s.compare.trustworthy() {
        CODE_DIFF
    } else if s.first_diff == 0 && value == 0 {
        CODE_TROUBLE
    } else {
        CODE_DIFF
    };
    Outcome::make(Stage::Snap, code, value, note, false, !conflicted)
}

/// 合法形状才给结论；非法形状返回 `None`（谓词用它把非法槽挡在判据之外）。
pub fn define_or_none(s: &Shape) -> Option<Outcome> {
    if !s.legal() {
        return None;
    }
    Some(define(s))
}

// ---------------------------------------------------------------------------
// 语料：8 活 + 6 死
// ---------------------------------------------------------------------------

/// 基型：同一台 GNU coreutils 9.5、同一档、同一 locale、同一列数下的
/// `touch --help` 双机全等快照（全绿）。
const BASE: Shape = Shape {
    applet: Applet::Touch,
    base_ask: Ask::HelpLong,
    target_ask: Ask::HelpLong,
    base_name: "touch",
    target_name: "touch",
    base_build: Build::Full,
    target_build: Build::Full,
    base_locale: Locale::C,
    target_locale: Locale::C,
    base_cols: COLS_DEFAULT,
    target_cols: COLS_DEFAULT,
    base_lines: 9,
    target_lines: 9,
    base_bytes: 420,
    target_bytes: 420,
    first_diff: 0,
    diverg: 0,
    compare: Compare::Verbatim,
    stream: Stream::BothStdout,
    flushed: true,
    text_base: true,
    text_target: true,
    base_exit: 0,
    target_exit: 0,
    opts: OPT_HELP_LONG,
};

/// 申报基型：与 `BASE` 推出的实际结论逐格相同（逐字全等且亮一致锚）。
const BASE_CLAIM: Claim = Claim {
    stage: Stage::Snap,
    code: CODE_SAME,
    value: 0,
    note: Note::Identical,
    default: true,
};

/// 十四格语料形状。前八格申报正确，后六格一槽只坏一维。
pub const SHAPES: [Shape; CORPUS] = [
    // 0 基准机与目标机的 `touch --help` 逐字全等（一致锚亮）。
    Shape {
        base_bytes: 412,
        target_bytes: 412,
        ..BASE
    },
    // 1 多形态 usage（`Usage:` 后两个空格 + 两条 `  or:`）逐字全等。
    Shape {
        applet: Applet::Cp,
        base_name: "cp",
        target_name: "cp",
        base_lines: 11,
        target_lines: 11,
        base_bytes: 512,
        target_bytes: 512,
        ..BASE
    },
    // 2 reproducible build 下的 `busybox --version`：构建时刻两边相同。
    Shape {
        applet: Applet::Busybox,
        base_ask: Ask::VersionLong,
        target_ask: Ask::VersionLong,
        base_name: "busybox",
        target_name: "busybox",
        base_cols: COLS_UNSET,
        target_cols: COLS_UNSET,
        base_lines: 1,
        target_lines: 1,
        base_bytes: 58,
        target_bytes: 58,
        opts: OPT_VERSION,
        ..BASE
    },
    // 3 `busybox --list`：集合相同、次序不同（行数与字节数都相同，只有次序漂）。
    Shape {
        applet: Applet::Busybox,
        base_ask: Ask::ListApplets,
        target_ask: Ask::ListApplets,
        base_name: "busybox",
        target_name: "busybox",
        base_cols: COLS_UNSET,
        target_cols: COLS_UNSET,
        base_lines: 4,
        target_lines: 4,
        base_bytes: 20,
        target_bytes: 20,
        first_diff: 1,
        diverg: DIV_ORDER,
        opts: OPT_LIST,
        ..BASE
    },
    // 4 译文档漂移：基准 `LC_ALL=C`，目标 `zh_CN.UTF-8`。
    Shape {
        applet: Applet::Ls,
        base_name: "ls",
        target_name: "ls",
        target_locale: Locale::ZhCn,
        base_lines: 12,
        target_lines: 12,
        base_bytes: 388,
        target_bytes: 296,
        first_diff: 5,
        diverg: DIV_LOCALE,
        ..BASE
    },
    // 5 裁剪档漂移：Full 对 Tiny，选项行整行消失。
    Shape {
        applet: Applet::Mkdir,
        base_name: "mkdir",
        target_name: "mkdir",
        target_build: Build::Tiny,
        base_lines: 10,
        target_lines: 5,
        base_bytes: 366,
        target_bytes: 171,
        first_diff: 6,
        diverg: DIV_TRIM,
        ..BASE
    },
    // 6 无对照面：Tiny 档两侧都没有 `--list --full` 的文本。
    Shape {
        applet: Applet::Busybox,
        base_ask: Ask::ListFull,
        target_ask: Ask::ListFull,
        base_name: "busybox",
        target_name: "busybox",
        base_build: Build::Tiny,
        target_build: Build::Tiny,
        base_cols: COLS_UNSET,
        target_cols: COLS_UNSET,
        base_lines: 0,
        target_lines: 0,
        base_bytes: 0,
        target_bytes: 0,
        stream: Stream::Silent,
        text_base: false,
        text_target: false,
        base_exit: 1,
        target_exit: 1,
        opts: OPT_LIST | OPT_LIST_FULL,
        ..BASE
    },
    // 7 掩盖：`diff -w` 吃掉行尾空白，快照确有漂移而比对档不可信。
    Shape {
        applet: Applet::Mkdir,
        base_name: "mkdir",
        target_name: "mkdir",
        base_lines: 10,
        target_lines: 10,
        base_bytes: 376,
        target_bytes: 366,
        first_diff: 2,
        diverg: DIV_TRAILING,
        compare: Compare::IgnoreSpace,
        ..BASE
    },
    // 8 Estate：现场逐字全等，申报方按"码 1 = 不一致"整份倒过来报。
    Shape {
        base_bytes: 412,
        target_bytes: 412,
        ..BASE
    },
    // 9 Stage：漂移在译文档闸，申报方推到最后那道封印闸。
    Shape {
        applet: Applet::Ls,
        base_name: "ls",
        target_name: "ls",
        target_locale: Locale::ZhCn,
        base_lines: 12,
        target_lines: 12,
        base_bytes: 388,
        target_bytes: 296,
        first_diff: 5,
        diverg: DIV_LOCALE,
        ..BASE
    },
    // 10 Code：裁剪漂移（确有不同），申报方写成"没法比"那一档。
    Shape {
        applet: Applet::Mkdir,
        base_name: "mkdir",
        target_name: "mkdir",
        target_build: Build::Tiny,
        base_lines: 10,
        target_lines: 5,
        base_bytes: 366,
        target_bytes: 171,
        first_diff: 6,
        diverg: DIV_TRIM,
        ..BASE
    },
    // 11 Value：时间戳与页脚两条都漂（差异项数 2），申报方只挂了一条。
    Shape {
        applet: Applet::Busybox,
        base_ask: Ask::VersionLong,
        target_ask: Ask::VersionLong,
        base_name: "busybox",
        target_name: "busybox",
        base_cols: COLS_UNSET,
        target_cols: COLS_UNSET,
        base_lines: 3,
        target_lines: 3,
        base_bytes: 140,
        target_bytes: 128,
        first_diff: 1,
        diverg: DIV_STAMP | DIV_FOOTER,
        opts: OPT_VERSION,
        ..BASE
    },
    // 12 Note：换行符档漂移，申报方说成行尾空白。
    Shape {
        applet: Applet::Cat,
        base_name: "cat",
        target_name: "cat",
        base_lines: 8,
        target_lines: 8,
        base_bytes: 268,
        target_bytes: 276,
        first_diff: 1,
        diverg: DIV_CRLF,
        ..BASE
    },
    // 13 Default：现场逐字全等且该亮锚，申报方把锚压灭（漏报一致）。
    Shape {
        applet: Applet::Busybox,
        base_ask: Ask::VersionLong,
        target_ask: Ask::VersionLong,
        base_name: "busybox",
        target_name: "busybox",
        base_cols: COLS_UNSET,
        target_cols: COLS_UNSET,
        base_lines: 1,
        target_lines: 1,
        base_bytes: 58,
        target_bytes: 58,
        opts: OPT_VERSION,
        ..BASE
    },
];

/// 十四格申报。前八格与 `define(SHAPES[i])` 逐格相同，后六格各坏一维。
pub const CLAIMS: [Claim; CORPUS] = [
    BASE_CLAIM,
    BASE_CLAIM,
    BASE_CLAIM,
    Claim {
        stage: Stage::Snap,
        code: CODE_DIFF,
        value: 1,
        note: Note::ListOrder,
        default: false,
    },
    Claim {
        stage: Stage::Locale,
        code: CODE_DIFF,
        value: 1,
        note: Note::LocaleDrift,
        default: false,
    },
    Claim {
        stage: Stage::Cfg,
        code: CODE_DIFF,
        value: 1,
        note: Note::TrimmedOption,
        default: false,
    },
    Claim {
        stage: Stage::Text,
        code: CODE_TROUBLE,
        value: 0,
        note: Note::NoText,
        default: false,
    },
    Claim {
        stage: Stage::Snap,
        code: CODE_DIFF,
        value: 1,
        note: Note::FuzzyMask,
        default: false,
    },
    // 8 Estate：码 0 的全等被申报成码 1 的不一致。
    Claim {
        stage: Stage::Snap,
        code: CODE_DIFF,
        value: 1,
        note: Note::EolStyle,
        default: false,
    },
    // 9 Stage：其余四维全对，闸位推后到封印闸。
    Claim {
        stage: Stage::Snap,
        code: CODE_DIFF,
        value: 1,
        note: Note::LocaleDrift,
        default: false,
    },
    // 10 Code：确有不同被申报成"没法比"。
    Claim {
        stage: Stage::Cfg,
        code: CODE_TROUBLE,
        value: 1,
        note: Note::TrimmedOption,
        default: false,
    },
    // 11 Value：两条漂移被申报成一条。
    Claim {
        stage: Stage::Snap,
        code: CODE_DIFF,
        value: 1,
        note: Note::StampDrift,
        default: false,
    },
    // 12 Note：换行符档被申报成行尾空白。
    Claim {
        stage: Stage::Snap,
        code: CODE_DIFF,
        value: 1,
        note: Note::TrailingBlank,
        default: false,
    },
    // 13 Default：四维全对而一致锚被压灭。
    Claim {
        stage: Stage::Snap,
        code: CODE_SAME,
        value: 0,
        note: Note::Identical,
        default: false,
    },
];

/// 期望表第一种形式：每格**实际**应当落在哪道闸（与申报表交叉校验用）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Snap,
    Stage::Snap,
    Stage::Snap,
    Stage::Snap,
    Stage::Locale,
    Stage::Cfg,
    Stage::Text,
    Stage::Snap,
    Stage::Snap,
    Stage::Locale,
    Stage::Cfg,
    Stage::Snap,
    Stage::Snap,
    Stage::Snap,
];

/// 期望表：每格实际的比对码。
pub const WANT_CODE: [i32; CORPUS] = [
    CODE_SAME,
    CODE_SAME,
    CODE_SAME,
    CODE_DIFF,
    CODE_DIFF,
    CODE_DIFF,
    CODE_TROUBLE,
    CODE_DIFF,
    CODE_SAME,
    CODE_DIFF,
    CODE_DIFF,
    CODE_DIFF,
    CODE_DIFF,
    CODE_SAME,
];

/// 期望表：每格实际的差异项数。
pub const WANT_VALUE: [u32; CORPUS] = [
    0, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 2, 1, 0,
];

/// 期望表：每格实际的差异类别。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::Identical,
    Note::Identical,
    Note::Identical,
    Note::ListOrder,
    Note::LocaleDrift,
    Note::TrimmedOption,
    Note::NoText,
    Note::FuzzyMask,
    Note::Identical,
    Note::LocaleDrift,
    Note::TrimmedOption,
    Note::StampDrift,
    Note::EolStyle,
    Note::Identical,
];

/// 期望表：每格实际的一致锚。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    true, true, true, false, false, false, false, false, true, false, false, false, false, true,
];

/// 期望表：每格申报坏掉的那一维（0 = 活格，1..6 = 死格，序与判序一致）。
pub const WANT_NDX: [u8; CORPUS] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6];

/// 十四格短名（标签必须挂本卡号，别卡的标签在本卡一律判死）。
pub const WANT_TAGS: [&str; CORPUS] = [
    "f11677-touch-help-eq",
    "f11677-cp-forms-eq",
    "f11677-version-eq",
    "f11677-list-order",
    "f11677-locale-drift",
    "f11677-trim-diff",
    "f11677-no-help-text",
    "f11677-ignore-space-masked",
    "f11677-estate-swap",
    "f11677-stage-swap",
    "f11677-code-swap",
    "f11677-value-swap",
    "f11677-note-swap",
    "f11677-default-swap",
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
    HD_ESTATE,
    HD_ESTAGE,
    HD_ECODE,
    HD_EVALUE,
    HD_ENOTE,
    HD_EDEFAULT,
];

/// 前八格的统计值。第 8 格是红档率标定值 929（14 格中 13 时的千分率）。
///
/// 七格统计依次为：语料格数、申报正确槽数、申报错误槽数、一档全等格数、
/// 二档诚实冲突格数、三档无对照面格数、亮一致锚的格数。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 3, 3, 1, 3, 929];

/// 语料全集。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case::blank(); CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        match (SHAPES.get(i), CLAIMS.get(i), WANT_TAGS.get(i)) {
            (Some(sh), Some(cl), Some(tg)) => {
                out[i] = Case {
                    shape: *sh,
                    claim: *cl,
                    tag: *tg,
                };
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// 取一格语料。
pub fn case_at(i: usize) -> Option<Case> {
    if i >= CORPUS {
        return None;
    }
    let sh = SHAPES.get(i)?;
    let cl = CLAIMS.get(i)?;
    let tg = WANT_TAGS.get(i)?;
    Some(Case {
        shape: *sh,
        claim: *cl,
        tag: *tg,
    })
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

/// **判序**：状态 → 阶段 → 比对码 → 差异项数 → 类别 → 锚位。次序固定，不许换。
pub fn judge(c: &Case) -> Fault {
    let a = define(&c.shape);
    let q = &c.claim;
    // Estate：成败互报优先，先于阶段/比对码。
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
// 场景族：语料之外，逐格钉住十一条漂移维与四种比对档
// ---------------------------------------------------------------------------

/// 十二个场景：四条前提漂移 + 三道落流形态 + 两种无对照面 + 三种不可信比对。
pub fn scenes() -> [Shape; SCENE_COUNT] {
    [
        // 0 程序名漂移：同一个二进制软链成 `rmt` 调用。
        Shape {
            applet: Applet::Ls,
            target_name: "rmt",
            base_name: "ls",
            diverg: DIV_NAME,
            first_diff: 1,
            base_bytes: 388,
            target_bytes: 404,
            base_lines: 12,
            target_lines: 12,
            ..BASE
        },
        // 1 裁剪档漂移：Full 对 Trim（说明文字缩短，选项行仍在）。
        Shape {
            applet: Applet::Cat,
            base_name: "cat",
            target_name: "cat",
            target_build: Build::Trim,
            diverg: DIV_TRIM,
            first_diff: 3,
            base_lines: 8,
            target_lines: 8,
            base_bytes: 300,
            target_bytes: 288,
            ..BASE
        },
        // 2 目标机被信号打断：无文本，码却是 1（不是 0）。
        Shape {
            text_target: false,
            target_bytes: 0,
            target_lines: 0,
            target_exit: 1,
            stream: Stream::Silent,
            ..BASE
        },
        // 3 目标机没设 locale：一侧不可复现，另一侧是 C。
        Shape {
            base_locale: Locale::C,
            target_locale: Locale::Unset,
            diverg: DIV_LOCALE,
            first_diff: 2,
            base_bytes: 412,
            target_bytes: 412,
            base_lines: 9,
            target_lines: 9,
            ..BASE
        },
        // 4 列数漂移：80 对 100，长说明在不同列折断。
        Shape {
            target_cols: COLS_WIDE,
            diverg: DIV_WRAP,
            first_diff: 4,
            base_lines: 9,
            target_lines: 8,
            base_bytes: 412,
            target_bytes: 412,
            ..BASE
        },
        // 5 落错流：目标机把帮助全文落进 stderr（F11678 的现场）。
        Shape {
            stream: Stream::TargetStderr,
            ..BASE
        },
        // 6 没冲刷：帮助还在 stdio 缓冲里，快照拿到半截串。
        Shape {
            flushed: false,
            ..BASE
        },
        // 7 合流：`2>&1` 抓在一起，错误与帮助分不开。
        Shape {
            stream: Stream::Merged,
            ..BASE
        },
        // 8 请求形态不同：一边 `-h`、一边 `--help`（GNU 多数 applet 不认 `-h`）。
        Shape {
            applet: Applet::Ls,
            base_name: "ls",
            target_name: "ls",
            base_ask: Ask::HelpShort,
            target_ask: Ask::HelpLong,
            opts: OPT_HELP_SHORT,
            ..BASE
        },
        // 9 不认识的 applet：没有文本源，判不了逐字。
        Shape {
            applet: Applet::Foreign,
            base_name: "foreign",
            target_name: "foreign",
            base_lines: 0,
            target_lines: 0,
            base_bytes: 0,
            target_bytes: 0,
            text_base: false,
            text_target: false,
            base_exit: 1,
            target_exit: 1,
            ..BASE
        },
        // 10 跳过比对：两侧都有文本却直接抄基准结论。
        Shape {
            compare: Compare::Skip,
            opts: OPT_HELP_LONG | OPT_REPORT_SAME,
            ..BASE
        },
        // 11 只比行数：行数相同、字节数不同，且没挂任何差异维（露馅）。
        Shape {
            compare: Compare::LineCountOnly,
            target_bytes: 396,
            opts: OPT_HELP_LONG,
            ..BASE
        },
    ]
}

/// 场景期望闸位。
pub const SCENE_WANT_STAGE: [Stage; SCENE_COUNT] = [
    Stage::Name,
    Stage::Cfg,
    Stage::Text,
    Stage::Locale,
    Stage::Wrap,
    Stage::Stream,
    Stage::Stream,
    Stage::Stream,
    Stage::Ask,
    Stage::Ask,
    Stage::Snap,
    Stage::Snap,
];

/// 场景期望比对码。
pub const SCENE_WANT_CODE: [i32; SCENE_COUNT] = [
    CODE_DIFF,
    CODE_DIFF,
    CODE_TROUBLE,
    CODE_DIFF,
    CODE_DIFF,
    CODE_TROUBLE,
    CODE_TROUBLE,
    CODE_TROUBLE,
    CODE_TROUBLE,
    CODE_TROUBLE,
    CODE_DIFF,
    CODE_DIFF,
];

/// 场景期望差异项数。
pub const SCENE_WANT_VALUE: [u32; SCENE_COUNT] = [1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0];

/// 场景期望类别。
pub const SCENE_WANT_NOTE: [Note; SCENE_COUNT] = [
    Note::ProgramName,
    Note::TrimmedOption,
    Note::NoText,
    Note::LocaleDrift,
    Note::WrapDrift,
    Note::StreamSwap,
    Note::StreamSwap,
    Note::StreamSwap,
    Note::NotComparable,
    Note::NotComparable,
    Note::FuzzyMask,
    Note::ByteDrift,
];

/// 场景期望锚位（全部不亮：一致锚只在逐字全等时亮）。
pub const SCENE_WANT_DEFAULT: [bool; SCENE_COUNT] = [
    false, false, false, false, false, false, false, false, false, false, false, false,
];

/// 场景期望的结构自洽位（最后两格是掩盖档，现场本身不自洽）。
pub const SCENE_WANT_CONSISTENT: [bool; SCENE_COUNT] = [
    true, true, true, true, true, true, true, true, true, true, false, false,
];

/// 场景期望档位（`Tier::of` 应当归档到哪儿）。
pub const SCENE_WANT_TIER: [Tier; SCENE_COUNT] = [
    Tier::Conflict,
    Tier::Conflict,
    Tier::NoText,
    Tier::Conflict,
    Tier::Conflict,
    Tier::Conflict,
    Tier::Conflict,
    Tier::Conflict,
    Tier::NoText,
    Tier::NoText,
    Tier::Masked,
    Tier::Masked,
];

/// 场景实际闸序要求（该场景应当在第几道闸出局）。
pub fn scene_rank_want(i: usize) -> Option<u32> {
    let s = SCENE_WANT_STAGE.get(i)?;
    Some(s.rank())
}

/// 取一格场景。
pub fn scene_at(i: usize) -> Option<(Shape, Stage, i32, u32, Note, bool)> {
    let sh = scenes().get(i).copied()?;
    let st = SCENE_WANT_STAGE.get(i)?;
    let cd = SCENE_WANT_CODE.get(i)?;
    let vl = SCENE_WANT_VALUE.get(i)?;
    let nt = SCENE_WANT_NOTE.get(i)?;
    let df = SCENE_WANT_DEFAULT.get(i)?;
    Some((sh, *st, *cd, *vl, *nt, *df))
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
    /// 一档：逐字全等。
    pub verbatim: Row,
    /// 二档：诚实冲突。
    pub conflict: Row,
    /// 三档：无对照面。
    pub notext: Row,
    /// 四档：掩盖。
    pub masked: Row,
}

impl Matrix {
    /// 零矩阵。
    pub const fn blank() -> Matrix {
        Matrix {
            verbatim: Row::blank(),
            conflict: Row::blank(),
            notext: Row::blank(),
            masked: Row::blank(),
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
                Tier::Verbatim => &mut m.verbatim,
                Tier::Conflict => &mut m.conflict,
                Tier::NoText => &mut m.notext,
                Tier::Masked => &mut m.masked,
            };
            row.total = row.total.saturating_add(1);
            if hit {
                row.good = row.good.saturating_add(1);
            }
            i += 1;
        }
        m
    }

    /// 四行全对的行号（本卡判据要的"逐字一致率 100%"落点之一）。
    pub fn perfect_rows(&self) -> usize {
        let mut n = 0usize;
        if self.verbatim.perfect() {
            n += 1;
        }
        if self.conflict.perfect() {
            n += 1;
        }
        if self.notext.perfect() {
            n += 1;
        }
        if self.masked.perfect() {
            n += 1;
        }
        n
    }

    /// 是否四档都被覆盖（每档至少有一格申报正确）。
    pub fn covered(&self) -> bool {
        self.verbatim.good > 0 && self.conflict.good > 0 && self.notext.good > 0 && self.masked.good > 0
    }

    /// 四档格数合计。
    pub fn cells(&self) -> usize {
        self.verbatim
            .total
            .saturating_add(self.conflict.total)
            .saturating_add(self.notext.total)
            .saturating_add(self.masked.total)
    }

    /// 四档判对合计。
    pub fn goods(&self) -> usize {
        self.verbatim
            .good
            .saturating_add(self.conflict.good)
            .saturating_add(self.notext.good)
            .saturating_add(self.masked.good)
    }

    /// 渲染整个矩阵（逐格稳定）。
    pub fn render(&self) -> String {
        let mut s = String::from("[");
        s.push_str(self.verbatim.render(Tier::Verbatim).as_str());
        s.push(' ');
        s.push_str(self.conflict.render(Tier::Conflict).as_str());
        s.push(' ');
        s.push_str(self.notext.render(Tier::NoText).as_str());
        s.push(' ');
        s.push_str(self.masked.render(Tier::Masked).as_str());
        s.push(']');
        s.push('\n');
        s
    }
}

// ---------------------------------------------------------------------------
// Report：千分率与闸门
// ---------------------------------------------------------------------------

/// 快照汇总。这张表**只认两个事实**：每一格的实际缺陷等于期望缺陷（本卡
/// 本身没走偏），以及活格确实逐字全等/死格确实坏在对的那一维。任何一维
/// 静默退化，`matched` 就会掉，`rate()` 立刻离开 1000。
#[derive(Clone, Copy, Debug)]
pub struct Report {
    /// 总槽数。
    pub total: u32,
    /// 申报正确槽数。
    pub accept: u32,
    /// 申报错误槽数。
    pub reject: u32,
    /// 一档（逐字全等）格数。
    pub verbatim: u32,
    /// 二档（诚实冲突）格数。
    pub conflict: u32,
    /// 三档（无对照面）格数。
    pub notext: u32,
    /// 四档（掩盖）格数。
    pub masked: u32,
    /// 亮一致锚的格数。
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
            Err(HD_ECOUNT)
        }
    }

    /// 四档格数合计（应当等于 `ACCEPT`：只有判对的格才计档）。
    pub fn tier_cells(&self) -> u32 {
        self.verbatim
            .saturating_add(self.conflict)
            .saturating_add(self.notext)
            .saturating_add(self.masked)
    }

    /// 码非全等的档合计（诚实冲突 + 掩盖）。
    pub fn red_cells(&self) -> u32 {
        self.conflict.saturating_add(self.masked)
    }

    /// 渲染整份汇总（逐字稳定，与 `RENDER_OK` 对账）。
    pub fn render(&self) -> String {
        let mut s = String::from("hd-snap corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" verbatim=");
        s.push_str(u32_text(self.verbatim).as_str());
        s.push_str(" conflict=");
        s.push_str(u32_text(self.conflict).as_str());
        s.push_str(" notext=");
        s.push_str(u32_text(self.notext).as_str());
        s.push_str(" masked=");
        s.push_str(u32_text(self.masked).as_str());
        s.push_str(" anchor=");
        s.push_str(u32_text(self.anchored).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(permille_text(self.rate()).as_str());
        s.push('\n');
        s
    }
}

/// 全绿渲染的逐字钉版。
const RENDER_OK: &str = "hd-snap corpus=14 accept=8 reject=6 verbatim=3 conflict=3 notext=1 masked=1 anchor=3 match=14 rate=1000ppm\n";

/// 空报表（供"越界槽位"分支先返回一个形状再落错码时用）。
pub fn empty_report() -> Report {
    Report {
        total: 0,
        accept: 0,
        reject: 0,
        verbatim: 0,
        conflict: 0,
        notext: 0,
        masked: 0,
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
/// 注入的意义是把"期望表"本身也拉进被测面：强行宣称某槽犯了另一种缺陷，
/// 报表必须掉 `matched`、`rate()` 随之离开 1000。若注入后仍报全绿，说明
/// `matched` 是硬编码的而不是算出来的。槽号越界一律 `HD_ESLOT`。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(HD_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut verbatim = 0u32;
    let mut conflict = 0u32;
    let mut notext = 0u32;
    let mut masked = 0u32;
    let mut anchored = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let Some(c) = cs.get(i) else {
            return Err(HD_ENOBS);
        };
        let a = c.actual();
        let mut f = judge(c);
        if let Some((j, forced)) = over {
            if j == i {
                f = forced;
            }
        }
        let Some(want) = expected(i) else {
            return Err(HD_ENOBS);
        };
        total = total.saturating_add(1);
        if f == want {
            matched = matched.saturating_add(1);
        }
        if matches!(f, Fault::Ok) {
            accept = accept.saturating_add(1);
            match Tier::of(&a) {
                Tier::Verbatim => verbatim = verbatim.saturating_add(1),
                Tier::Conflict => conflict = conflict.saturating_add(1),
                Tier::NoText => notext = notext.saturating_add(1),
                Tier::Masked => masked = masked.saturating_add(1),
            }
            if a.default {
                anchored = anchored.saturating_add(1);
            }
        } else {
            reject = reject.saturating_add(1);
        }
        i += 1;
    }
    if total != CORPUS as u32 {
        return Err(HD_ECOUNT);
    }
    Ok(Report {
        total,
        accept,
        reject,
        verbatim,
        conflict,
        notext,
        masked,
        anchored,
        matched,
    })
}
