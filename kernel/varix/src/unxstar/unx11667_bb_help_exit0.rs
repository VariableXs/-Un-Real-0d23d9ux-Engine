//! UNX-F11667 busybox --help 退出码 0 与 usage 全文判据
//!
//! busybox 是一个**多调用二进制**：同一份程序按 `argv[0]` 或 `busybox <name>`
//! 查表分发。可是有一类形参**压根不是 applet 名**——`--help`、裸调（无 argv[1]）、
//! `--list`、`--list-full`。它们走 busybox.c 里 `main` 之前的顶层识别，认出之后
//! 直接打帮助文本、退 **0**、且**一个字都不进 stderr**。本卡立的就是这条
//! "帮助面"的正判据：`busybox --help` 成功 0 档、usage 全文四段齐、输出走 stdout。
//!
//! 基准机（BusyBox 1.36.1，全量 defconfig 413 个内建 applet）上
//! `busybox --help` 的实际现场是这样的：
//!
//! ```text
//! $ busybox --help > /tmp/help.txt 2>/tmp/help.err ; echo $?
//! 0
//! $ wc -c /tmp/help.err
//! 0
//! $ head -n2 /tmp/help.txt
//! BusyBox is copyrighted by many authors between 1998-2015.
//! Licensed under GPLv2. See source files for details.
//! ```
//!
//! 帮助全文由**四段**拼成，段段可数、缺一即断档：
//!
//! 1. **banner 段**——版权两行 + 版本行
//!    `BusyBox v1.36.1 (2023-11-08 23:30:50 UTC) multi-call binary.`；
//! 2. **usage 段**——三行用法骨架（`Usage: busybox [function [arguments]...]`、
//!    `   or: busybox --list[-full]`、`   or: busybox --install [-s] [DIR]`）；
//! 3. **脚注段**——五行软链说明，第一行含**函数总数**
//!    （`Each of the above 413 functions is implemented as`）；
//! 4. **名录段**——`Currently defined functions:` 起头，按首字母分组列出全部内建名。
//!
//! 脚注段的 N 与名录段的实列个数**必须互相对账**：本卡把"脚注说 413、名录只列了
//! 412"这类现场判死在计数闸——帮助文本内部自相矛盾，比缺一段更隐蔽，因为每一段
//! 单看都"齐"。同理，`listed > registered`（名录列出了构建里根本没有的名字）是
//! 计数闸的第二种死法，归 `ListOverflow`，锚位一律不给。
//!
//! 本卡与分发层的边界是**硬**的：`--hel`、`--HELP`、`--` 这些**长得像**顶层选项
//! 的形参，在基准机上走 `find_applet_by_name` 未命中路径，打
//! `<名>: applet not found` 退 **127**。那是 F11674/F11661 的账。本卡遇到这类
//! 现场只做一件事：**拒收**——把它们归入 `FellApplet` 档、不落任何帮助锚，
//! 谁敢把 127 的现场申报成"帮助成功 0 档"，Estate 维当场判死。
//!
//! `--list` 与 `--list-full` 同属成功 0 档，但它们是**名录单独成篇**：没有
//! banner/usage/脚注三段，`--list-full` 还多一列路径。它们退 0、走 stdout，
//! 却不是"usage 全文"——本卡给它们一档**列名档**，落 `DoneList`/`DoneListFull`
//! 注解，**不落全文锚**。把 `--list` 的输出当成 usage 全文落锚，是本卡
//! Default 维专门设防的错位。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 七道闸，秩从 **1** 起算；`Gate` 只接受"前进到下一秩"，倒退落 `BH_ESTAGE`、
//! 跳档落 `BH_EBAL`、封印后再动落 `BH_ESTATE`。前一闸不产出事实，后一闸就没有
//! 输入。
//!
//! 1. `args`（秩 1）——argv 形状。有没有 `argv[1]` 在这一闸分流：无 → 帮助全文
//!    路径（与 `--help` 同形），有 → 交给识别闸。此闸只产出"形"，不裁对错。
//! 2. `recognize`（秩 2）——顶层形态识别闸。`--help`/`--list`/`--list-full` 三串
//!    **逐字**匹配才算顶层选项；其余一律掉进 applet 名查找——那就是 F11674 的
//!    地盘，本卡在此闸把现场钉成 `FellApplet` 并拒绝落锚。它必须排在任何段落
//!    组装闸之前：连"这是帮助调用"都没定性，后面谈四段齐缺是越权。
//! 3. `sections`（秩 3）——四段组装闸。banner/usage/脚注/名录四个段位逐位点验，
//!    缺哪段报哪段（`BannerMissing`…`ListMissing`）。`--list` 家族只点名录段。
//!    它必须排在计数对账之前：段都没有，"N 与实列对账"失去载体。
//! 4. `count`（秩 4）——对账闸。脚注的 N、名录实列数、构建内建数三方对齐
//!    （`CountMismatch`/`ListOverflow`），字节数低于基准全文判 `Truncated`。
//!    它必须排在通道闸之前：内容自不自洽是内容的事，先于"内容去了哪个通道"。
//! 5. `channel`（秩 5）——通道闸。帮助全文**只许走 stdout**：全量入 stderr 记
//!    `ToStderr`，退 0 却一字不出记 `SilentHelp`，劈成两半记 `SplitWrite`。
//!    排在对账之后，是因为"写满了但写错了通道"仍是通道缺陷而非内容缺陷，
//!    两闸各管各的账，不许互相顶。
//! 6. `code`（秩 6）——退出码闸。帮助调用的一切形态，事实码只认**观测到的**
//!    非零值（`BadExit`）；这一闸排在最后的内容闸之后，意味着"码对了"永远
//!    救不回前面任何一闸的断档——码是结算，不是质检。
//! 7. `settle`（秩 7）——收尾结算闸：四段齐、三方对账平、通道纯 stdout、码 0，
//!    才允许站在这里封印并落**全文锚**。它是唯一允许封印的秩。
//!
//! ## 关键差异（本卡深度所在）
//!
//! * **与 F11666（PATH 回退，段基址 `0x6d1d0`，位距 `0x10`）的边界**：PATH 卡判
//!   的是"逐个目录找不到之后才 notfound"的**路径穷尽**问题；本卡的调用连 PATH 都
//!   不碰——`--help` 走的是顶层识别，**不查 PATH、不查目录**。两者的现场在码面上
//!   甚至同数（都可能是 127），归因完全不同：那是"找遍了"，这是"根本没找"。
//! * **与 F11668（applet usage，直接后置，位距 `0x10`）的边界**：本卡管
//!   **多调用二进制自己的**帮助（banner+usage+脚注+名录四段，主语是 busybox）；
//!   F11668 管**单个 applet 的** `usage:` 行（主语是 applet 名，无 banner 无名录）。
//!   同一句 "usage" 在两张卡里是两个形状：`Usage: busybox ...` 大写带骨架三行，
//!   `usage: ls ...` 小写一行。把 busybox 全文落进 applet 卡的锚、或反过来，
//!   是两卡互相的 Note 维死槽。
//! * **与 F11674（applet not found 文案，段基址 `0x6d270`）的边界**：凡是
//!   `find_applet_by_name` 未命中的现场（含 `--hel`/`--HELP`/`--`），文案、127 码、
//!   stderr 通道全归 F11674。本卡对这些现场**只认一档 `FellApplet` 并整体拒收**，
//!   不裁它的文案对不对——那是防重锚的分工，越界裁文案就是重复立卡。
//! * **与 F11678（stderr/stdout 分离，段基址 `0x6d2d0`）的边界**：F11678 立的是
//!   **notfound 错误面**的通道归属；本卡的通道闸只管**帮助面**"全文必须走
//!   stdout"这一条正判据。两卡共用通道词汇，判的现场相反：那边是"错误不许进
//!   stdout"，这边是"帮助不许进 stderr"。
//! * **与 F11664（notfound 退出码 127，段基址 `0x6d1a0`）的边界**：127 是分发层
//!   的码；本卡的成功档码恒 **0**，非零码（1/2）出现在帮助调用上一律 `BadExit`
//!   拒收。谁把 0 档与 127 档混记，Estate/Code 两维在本卡当场可抓。
//! * **本卡的排他职责**：① 四段完整性（banner/usage/脚注/名录逐位点验）；
//!   ② 三方对账（脚注 N = 实列数 = 构建内建数）与字节截断档；
//!   ③ 帮助面 stdout 通道纯度（含 SilentHelp 这种"成功却沉默"的反直觉档）；
//!   ④ 顶层识别与 applet 名查找的**分界拒收**（形似选项不落 127 的账、
//!   也不落帮助的锚）；⑤ `--list`/`--list-full` 单独成档不落全文锚。
//! * **谁判死谁放行**：形参逐字命中三个顶层选项之一、四段齐、三方对账平、
//!   全文走 stdout、码 0 → 放行并落全文锚；四段缺任一段、N 与实列不符、
//!   字节被截、文本进 stderr、一字不出、帮助调用退了非零码 → 判死不落锚；
//!   形参没被识别、现场走了 applet 未名路径 → 本卡整体拒收，转 F11674。
//!
//! 判据六维：结束阶段（gate）、退出码（code）、名录实列数（value）、段落形态
//! （note）、全文锚位（default：是否"四段齐+对账平+通道纯+码 0"）、成败申报
//! （estate）。判序固定：状态 → 阶段 → 退出码 → 实列数 → 注解 → 锚位。
//!
//! 语料 14 槽：8 槽申报正确（全文 1、裸调 1、--list 1、--list-full 1、缺 usage
//! 段 1、全文进 stderr 1、帮助退非零码 1、形似选项掉未名路径 1），6 槽里 5 槽
//! 复刻同一活现场、各挂一维错报（Estate/Stage/Code/Value/Note/Default），
//! 其中 Estate 槽特意拿**全文成功现场**申报成失败——帮助面的成败互报，
//! 本卡判据一枪打的就是它。账本 14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码，
//! 冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11667，VX-1029-5 批分配值，不得自改）。
pub const SEG_BASE: u32 = 0x6d1e0;

/// 交叉署名：F11666「PATH 查找失败回退」的段基址（本卡的直接前置）。
pub const ANCHOR_F11666_SEG: u32 = 0x6d1d0;
/// F11666 与本卡的位距（0x6d1e0 - 0x6d1d0）。
pub const ANCHOR_F11666_GAP: u32 = 0x10;
/// 交叉署名：F11664「notfound 退出码 127」的段基址（127 档的持有者）。
pub const ANCHOR_F11664_SEG: u32 = 0x6d1a0;
/// F11664 与本卡的位距。
pub const ANCHOR_F11664_GAP: u32 = 0x40;
/// 交叉署名：F11674「applet not found 文案逐字」的段基址（拒收档的归属卡）。
pub const ANCHOR_F11674_SEG: u32 = 0x6d270;
/// 本卡与本卡的直接后置 F11668 的位距（F11668 段基址 = 本卡 + 0x10）。
pub const NEXT_F11668_GAP: u32 = 0x10;
/// 交叉署名：F11668「applet usage」的段基址。本卡不 import 它，只署段位复算。
pub const NEXT_F11668_SEG: u32 = 0x6d1f0;
/// F11678（notfound 通道分离）与本卡的位距——两卡共通道词汇、判相反现场。
pub const ANCHOR_F11678_GAP: u32 = 0xf0;
/// 交叉署名：F11678 的段基址（0x6d1e0 + 0xf0 = 0x6d2d0）。
pub const ANCHOR_F11678_SEG: u32 = 0x6d2d0;

/// 与 F11664 在码面的署名：未名路径的档就是 127。
pub const ANCHOR_NOTFOUND_EXIT: u32 = 127;
/// 与 F11674 在文案面的署名：未名路径的模板主语（`<名>: applet not found`）。
pub const ANCHOR_NOTFOUND_TAIL: &str = "applet not found";
/// 与基准机在版本面的署名：帮助全文 banner 段携带的版本串。
pub const ANCHOR_BB_VERSION: &str = "BusyBox v1.36.1";
/// 基准机构建的内建 applet 数（脚注 N 与名录实列的三方对账锚）。
pub const ANCHOR_REGISTERED: u32 = 413;
/// 基准机帮助全文的完整字节数（截断判定的基准）。
pub const ANCHOR_HELP_BYTES: u32 = 2662;

/// 语料槽数。
pub const CORPUS: usize = 14usize;

/// 账本格数（与语料同尺：一格一槽）。
pub const SAMPLES: usize = 14usize;

/// 申报正确的槽数。
pub const ACCEPT: usize = 8usize;

/// 申报错误的槽数。
pub const REJECT: usize = 6usize;

/// 档位数。
pub const TIER_COUNT: usize = 4usize;

/// 闸门道数（秩 1..7）。
pub const STAGE_COUNT: u32 = 7u32;

/// 最后一道闸的秩（封印只接受站在这里的账）。
pub const STAGE_LAST_RANK: u32 = 7u32;

/// 帮助全文的段数（banner / usage / 脚注 / 名录）。
pub const SECTION_COUNT: usize = 4usize;

/// 顶层调用形态数。
pub const CALL_COUNT: usize = 7usize;

/// 输出通道档数（stdout / stderr /  nowhere）。
pub const CHAN_COUNT: usize = 3usize;

/// 账本里统计格的个数（红档格与缺陷格之外）。
pub const STAT_CELLS: usize = 7usize;

/// 名录实列数的合法上界（越界即坏形状，不参与判据）。
pub const LISTED_MAX: u32 = 4096;

/// 帮助文本字节数的合法上界（256 KiB；远超基准机一个数量级即坏形状）。
pub const BYTES_MAX: u32 = 0x0004_0000;

/// 红档率标定值：14 格里恰好中 13 格时的千分率 `(13*1000 + 14/2) / 14`。
pub const RED_RATE: u32 = 929;

/// 全绿千分率。
pub const RATE_FULL: u32 = 1000;

// ---------------------------------------------------------------------------
// 退出码（BusyBox 1.36.1 实况真值）
// ---------------------------------------------------------------------------

/// 成功收尾：`--help`/`--list` 一切正常形态唯一的码。本卡的正判据锚档。
pub const EXIT_OK: u32 = 0;
/// applet 未名路径的码（F11664/F11674 持有）。本卡用它钉 `FellApplet` 拒收档。
pub const EXIT_APPLET: u32 = 127;
/// busybox applet 层的通用败码（bb_show_usage 之后退的那档）。
/// 它出现在**帮助调用**上就是坏码——帮助面没有理由退 1。
pub const EXIT_APPLET_FAIL: u32 = 1;
/// GNU 家族用法错档（邻居词汇，本卡只用来防止跨家族混认）。
pub const EXIT_GNU_USAGE: u32 = 2;
/// 本卡不裁定的占位码：故意取一个撞不上的值，谎报行不可能被误读成正常行。
pub const EXIT_UNJUDGED: u32 = 99;

/// 合法退出码档位（0 / 1 / 2 / 127 四档真实 + 占位档）。
pub const EXIT_RANKS: usize = 5usize;

/// 真实码表（遍历与唯一性检查用）。
pub const EXIT_TABLE: [u32; 4] = [EXIT_OK, EXIT_APPLET_FAIL, EXIT_GNU_USAGE, EXIT_APPLET];

// ---------------------------------------------------------------------------
// 段落位（帮助全文的四段，逐位点验）
// ---------------------------------------------------------------------------

/// bit0：banner 段（版权两行 + `BusyBox v1.36.1 ... multi-call binary.`）。
pub const SEC_BANNER: u8 = 0x01;
/// bit1：usage 段（`Usage: busybox [function [arguments]...]` 等三行）。
pub const SEC_USAGE: u8 = 0x02;
/// bit2：脚注段（`Each of the above 413 functions ...` 五行，含总数 N）。
pub const SEC_FOOT: u8 = 0x04;
/// bit3：名录段（`Currently defined functions:` + 按首字母分组的全部内建名）。
pub const SEC_LIST: u8 = 0x08;

/// 全文四段齐的完整位掩码。
pub const SEC_ALL: u8 = SEC_BANNER | SEC_USAGE | SEC_FOOT | SEC_LIST;

/// 段落位的个数。
pub const SEC_BIT_COUNT: usize = 4usize;

/// 段落位表（遍历用）。
pub const SEC_BITS: [u8; SEC_BIT_COUNT] = [SEC_BANNER, SEC_USAGE, SEC_FOOT, SEC_LIST];

/// `--list` 家族**只点名录段**：其余三段在这一族缺失不是缺陷，是形态。
pub const SEC_LIST_ONLY: u8 = SEC_LIST;

// ---------------------------------------------------------------------------
// 顶层形参字面值（逐字匹配的三条 + 长得像的三条 + 裸调）
// ---------------------------------------------------------------------------

/// 逐字匹配的第一串：帮助全文。
pub const FLAG_HELP: &str = "--help";
/// 逐字匹配的第二串：名录。
pub const FLAG_LIST: &str = "--list";
/// 逐字匹配的第三串：名录带路径。
pub const FLAG_LIST_FULL: &str = "--list-full";
/// 长得像但**不**逐字命中的串（掉 applet 名路径）。
pub const FLAG_LOOKALIKE: &str = "--hel";
/// 大小写翻车串（busybox 顶层匹配区分大小写，同样掉 applet 名路径）。
pub const FLAG_CAPS: &str = "--HELP";
/// 裸双横线串（applet 名路径，名就是 `--`）。
pub const FLAG_DASHDASH: &str = "--";
/// usage 段第一行的逐字骨架（大写 Usage，主语 busybox）。
pub const USAGE_HEAD: &str = "Usage: busybox [function [arguments]...]";
/// usage 段第二行：`--list[-full]` 的紧凑写法逐字。
pub const USAGE_OR_LIST: &str = "   or: busybox --list[-full]";
/// usage 段第三行：`--install` 骨架逐字。
pub const USAGE_OR_INSTALL: &str = "   or: busybox --install [-s] [DIR]";
/// 名录段的逐字标题行。
pub const LIST_HEADING: &str = "Currently defined functions:";
/// 脚注段首行的逐字前缀（N 嵌在其后）。
pub const FOOT_HEAD: &str = "Each of the above ";
/// 脚注段首行的逐字后缀。
pub const FOOT_TAIL: &str = " functions is implemented as";

// ---------------------------------------------------------------------------
// 错误码（13 个，SEG_BASE + 0x01..0x0d）
// ---------------------------------------------------------------------------

/// 01 阶段闸申报不符：缺段现场说成对账现场，或对账现场说成通道现场。
pub const BH_ESTAGE: u32 = SEG_BASE + 0x01;
/// 02 状态违例：成败互报、闸门封印后再动、账本冻结后再登记。
pub const BH_ESTATE: u32 = SEG_BASE + 0x02;
/// 03 退出码申报不符：帮助调用退了 1 却报 0，或把 127 的现场报成 0 档成功。
pub const BH_ECODE: u32 = SEG_BASE + 0x03;
/// 04 名录实列数申报不符：脚注 N、实列数、内建数三方里报错了任何一方。
pub const BH_ELIST: u32 = SEG_BASE + 0x04;
/// 05 注解申报不符：四段形态（缺哪段/截断/沉默/错通道）与 `--list` 家族混认。
pub const BH_ENOTE: u32 = SEG_BASE + 0x05;
/// 06 全文锚位申报不符：四段齐对账平不落锚、缺段现场却落锚。
pub const BH_EANCH: u32 = SEG_BASE + 0x06;
/// 07 通道越界：帮助全文出现在 stderr / 劈半 / 一字不出（本卡排他维之一）。
pub const BH_ECHUNK: u32 = SEG_BASE + 0x07;
/// 08 分发越界：`--help` 被当 applet 名走了未名路径（本卡整体拒收）。
pub const BH_EDISP: u32 = SEG_BASE + 0x08;
/// 09 计数不符：报表不满、矩阵不平、档位覆盖有洞。
pub const BH_ECOUNT: u32 = SEG_BASE + 0x09;
/// 10 槽位非法：注入槽越界，或账本登记时 slot != len。
pub const BH_ESLOT: u32 = SEG_BASE + 0x0a;
/// 11 指纹不符：冻结后重放对不上，或篡改留痕。
pub const BH_ESEAL: u32 = SEG_BASE + 0x0b;
/// 12 无观测：槽号超出语料、期望表缺格。
pub const BH_ENOBS: u32 = SEG_BASE + 0x0c;
/// 13 账本不平：格数、成败比或对账表任一失衡。
pub const BH_EBAL: u32 = SEG_BASE + 0x0d;

/// 13 个错误码的有序全集（检查器与账本共用）。
pub const ALL_CODES: [u32; 13] = [
    BH_ESTAGE,
    BH_ESTATE,
    BH_ECODE,
    BH_ELIST,
    BH_ENOTE,
    BH_EANCH,
    BH_ECHUNK,
    BH_EDISP,
    BH_ECOUNT,
    BH_ESLOT,
    BH_ESEAL,
    BH_ENOBS,
    BH_EBAL,
];

/// 错误码个数。
pub const CODE_COUNT: usize = 13usize;

// ---------------------------------------------------------------------------
// 操作码（账本格的操作归类）
// ---------------------------------------------------------------------------

/// 顶层识别闸格。
pub const OP_RECOG: u8 = 0x01;
/// 段落组装闸格。
pub const OP_SECTIONS: u8 = 0x02;
/// 三方对账闸格。
pub const OP_COUNT: u8 = 0x03;
/// 通道闸格。
pub const OP_CHANNEL: u8 = 0x04;
/// 码闸格。
pub const OP_CODE: u8 = 0x05;
/// 语料全跑格。
pub const OP_CORPUS: u8 = 0x06;
/// 红档率标定格。
pub const OP_RATE: u8 = 0x07;
/// 缺陷码格。
pub const OP_DEFECT: u8 = 0x08;
/// 统计格。
pub const OP_STAT: u8 = 0x09;

/// 操作码个数。
pub const OP_COUNT_N: usize = 9usize;

/// 操作码表。
pub const OP_BITS: [u8; OP_COUNT_N] = [
    OP_RECOG,
    OP_SECTIONS,
    OP_COUNT,
    OP_CHANNEL,
    OP_CODE,
    OP_CORPUS,
    OP_RATE,
    OP_DEFECT,
    OP_STAT,
];

/// 操作码是否在已知表内（表是九元素数组，不是位掩码，所以逐格比）。
pub fn op_known_byte(op: u8) -> bool {
    let mut i = 0usize;
    while i < OP_BITS.len() {
        if let Some(v) = OP_BITS.get(i) {
            if *v == op {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 操作码 → 归类名。
pub fn op_name(op: u8) -> &'static str {
    match op {
        OP_RECOG => "bh-op-recognize",
        OP_SECTIONS => "bh-op-sections",
        OP_COUNT => "bh-op-reconcile",
        OP_CHANNEL => "bh-op-channel",
        OP_CODE => "bh-op-exitcode",
        OP_CORPUS => "bh-op-corpus",
        OP_RATE => "bh-op-rate",
        OP_DEFECT => "bh-op-defect",
        OP_STAT => "bh-op-stat",
        _ => "bh-op-unknown",
    }
}

/// 九枚操作码互不相同（表自身的唯一性）。
pub fn ops_are_unique() -> bool {
    let mut i = 0usize;
    while i < OP_BITS.len() {
        let a = match OP_BITS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if !op_known_byte(a) || a == 0 {
            return false;
        }
        let mut j = i + 1;
        while j < OP_BITS.len() {
            if let Some(b) = OP_BITS.get(j) {
                if *b == a {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    OP_BITS.len() == OP_COUNT_N
}

// ---------------------------------------------------------------------------
// 错误码 → 语义短名 / 段内偏移
// ---------------------------------------------------------------------------

/// 错误码 → 语义短名（第二形态，供账本与测试复核段位）。
pub fn code_name(code: u32) -> &'static str {
    match code {
        BH_ESTAGE => "bh-stage",
        BH_ESTATE => "bh-estate",
        BH_ECODE => "bh-code",
        BH_ELIST => "bh-value",
        BH_ENOTE => "bh-note",
        BH_EANCH => "bh-default",
        BH_ECHUNK => "bh-channel",
        BH_EDISP => "bh-dispatch",
        BH_ECOUNT => "bh-count",
        BH_ESLOT => "bh-slot",
        BH_ESEAL => "bh-seal",
        BH_ENOBS => "bh-nobs",
        BH_EBAL => "bh-bal",
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

/// 短名的段内编号（`bh-` 前缀之后的部分是否为本卡认得的短名）。
pub fn name_known(slug: &str) -> bool {
    has_sub(slug, "bh-")
}

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

/// 折入一个布尔。
fn fold_flag(h: u64, v: bool) -> u64 {
    fold_word(h, v as u64)
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

/// 折入一条留痕名（闸门拒动/前进的记账用）。
fn fold_name(h: u64, name: &str) -> u64 {
    let v = fold_str(h, name);
    fold_byte(v, b'|')
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

/// 两段文本是否逐字相等（本卡的"逐字判据"入口，全部走字节比较）。
pub fn same_text(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    starts_with(a, b)
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

/// 换行计数（段落行数点验用）。
pub fn newline_count(hay: &str) -> u32 {
    count_char(hay, b'\n')
}

// ---------------------------------------------------------------------------
// 帮助全文四段的逐字复刻（msg_*，含大小写与空格的原文措辞）
// ---------------------------------------------------------------------------

/// banner 段第一行：版权年份跨度逐字。
pub fn msg_copyright_head() -> String {
    String::from("BusyBox is copyrighted by many authors between 1998-2015.")
}

/// banner 段第二行：许可逐字。
pub fn msg_copyright_license() -> String {
    String::from("Licensed under GPLv2. See source files for details.")
}

/// banner 段版本行：`BusyBox v1.36.1 (2023-11-08 23:30:50 UTC) multi-call binary.`。
pub fn msg_version_line() -> String {
    String::from("BusyBox v1.36.1 (2023-11-08 23:30:50 UTC) multi-call binary.")
}

/// banner 段全文（三行，行间逐字换行）。
pub fn msg_banner() -> String {
    let mut s = msg_copyright_head();
    s.push('\n');
    s.push_str(msg_copyright_license().as_str());
    s.push('\n');
    s.push('\n');
    s.push_str(msg_version_line().as_str());
    s.push('\n');
    s
}

/// usage 段全文（三行骨架，逐字含行首三个空格的 `   or:` 前导）。
pub fn msg_usage_block() -> String {
    let mut s = String::from(USAGE_HEAD);
    s.push('\n');
    s.push_str(USAGE_OR_LIST);
    s.push('\n');
    s.push_str(USAGE_OR_INSTALL);
    s
}

/// 脚注段首行：`Each of the above <N> functions is implemented as`。
pub fn msg_foot_head(n: u32) -> String {
    let mut s = String::from(FOOT_HEAD);
    s.push_str(dec_text(n).as_str());
    s.push_str(FOOT_TAIL);
    s
}

/// 脚注段全文（首行嵌 N，其后四行逐字复刻软链说明）。
pub fn msg_footnote(n: u32) -> String {
    let mut s = msg_foot_head(n);
    s.push_str("\na mutually-linked symbolic link (symlink) to the");
    s.push_str("\nbusybox executable.  Therefore, only one disk space");
    s.push_str("\nis used up, and you have to (re)make the symlink");
    s.push_str("\nagain if you wish to change the name of a function.");
    s
}

/// 名录段标题行（逐字）。
pub fn msg_list_heading() -> String {
    String::from(LIST_HEADING)
}

/// 名录段的一个首字母分组行：`\t<字母>: <逗号分隔名单>`。
pub fn msg_list_group(letter: &str, names: &str) -> String {
    let mut s = String::from("\t");
    s.push_str(letter);
    s.push_str(": ");
    s.push_str(names);
    s
}

/// 名录段的样例分组（基准机 a 组前几员，取证行用）。
pub fn msg_list_group_a() -> String {
    msg_list_group(
        "a",
        "ac, adjtimex, arping, ascii, ash, automount, awk, base32, base64, basename",
    )
}

/// 帮助全文的装配（四段按位拼接；缺段就是缺段，本函数**不**替调用方补）。
///
/// 这一函数是"逐字判据"的正方：全绿现场装配出来的串必须与基准机快照逐字相等，
/// `RENDER_OK` 钉住的正是"装配相等 + 通道纯 + 码 0"三件事同时成立。
pub fn assemble_help(sections: u8, n: u32) -> String {
    let mut s = String::new();
    if sections & SEC_BANNER != 0 {
        s.push_str(msg_banner().as_str());
        s.push('\n');
    }
    if sections & SEC_USAGE != 0 {
        s.push_str(msg_usage_block().as_str());
        s.push('\n');
        s.push('\n');
    }
    if sections & SEC_FOOT != 0 {
        s.push_str(msg_footnote(n).as_str());
        s.push('\n');
        s.push('\n');
    }
    if sections & SEC_LIST != 0 {
        s.push_str(msg_list_heading().as_str());
        s.push('\n');
        s.push_str(msg_list_group_a().as_str());
        s.push('\n');
    }
    s
}

/// 装配全文的逐字相等复核（与基准机快照比较的公开入口）。
pub fn help_matches_snapshot(sections: u8, n: u32, snapshot: &str) -> bool {
    same_text(assemble_help(sections, n).as_str(), snapshot)
}

/// 未名路径的取证行（F11674 归属，本卡只呈现、不裁文案对错）。
pub fn msg_applet_not_found(name: &str) -> String {
    let mut s = String::from(name);
    s.push_str(": ");
    s.push_str(ANCHOR_NOTFOUND_TAIL);
    s
}

/// 缺段的取证行：`busybox --help: missing <段名> section`。
pub fn msg_missing_section(which: &Section) -> String {
    let mut s = String::from("busybox --help: missing ");
    s.push_str(which.slug());
    s.push_str(" section of usage text");
    s
}

/// 三方对账不符的取证行（脚注 N / 实列数 / 内建数）。
pub fn msg_count_disagree(foot_n: u32, listed: u32, registered: u32) -> String {
    let mut s = String::from("busybox --help: footnote says ");
    s.push_str(dec_text(foot_n).as_str());
    s.push_str(", list holds ");
    s.push_str(dec_text(listed).as_str());
    s.push_str(", build registers ");
    s.push_str(dec_text(registered).as_str());
    s.push_str(" — three-way reconciliation failed");
    s
}

/// 字节截断的取证行。
pub fn msg_truncated(got: u32, need: u32) -> String {
    let mut s = String::from("busybox --help: output truncated at ");
    s.push_str(dec_text(got).as_str());
    s.push_str(" of ");
    s.push_str(dec_text(need).as_str());
    s.push_str(" bytes");
    s
}

/// 通道越界的取证行。
pub fn msg_wrong_channel(chan: &Chan) -> String {
    let mut s = String::from("busybox --help: usage text went to ");
    s.push_str(chan.slug());
    s.push_str(" — help belongs to stdout only");
    s
}

/// 成功却沉默的取证行（本卡最反直觉的一档）。
pub fn msg_silent_help() -> String {
    String::from("busybox --help: exited 0 without a single byte — silent success is a lie")
}

/// 帮助调用退了非零码的取证行。
pub fn msg_bad_exit(code: u32) -> String {
    let mut s = String::from("busybox --help: exit code ");
    s.push_str(dec_text(code).as_str());
    s.push_str(" — help face knows only 0");
    s
}

/// 分发越界的取证行（`--help` 掉进 applet 名路径）。
pub fn msg_dispatch_fall(flag: &str) -> String {
    let mut s = String::from("busybox ");
    s.push_str(flag);
    s.push_str(": fell to find_applet_by_name — refused by F11667, owned by F11674");
    s
}

/// 全文锚的现场摘要行（报表与账本共用）。
pub fn msg_help_anchor(listed: u32, bytes: u32) -> String {
    let mut s = String::from("busybox help anchor: ");
    s.push_str(dec_text(listed).as_str());
    s.push_str(" names, ");
    s.push_str(dec_text(bytes).as_str());
    s.push_str(" bytes, stdout, exit 0");
    s
}

// ---------------------------------------------------------------------------
// 顶层调用形态
// ---------------------------------------------------------------------------

/// busybox 顶层形参的七种形态。
///
/// 这一维是本卡与分发层（F11661/F11674）的**分界线本身**：逐字命中三条顶层串
/// 的走帮助面；裸调走帮助面；其余"长得像"的一律掉 `find_applet_by_name`，
/// 本卡对后者只许拒收，不许裁它的文案。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// `--help`：逐字命中第一串，帮助全文。
    HelpLong,
    /// 裸调（无 `argv[1]`）：与 `--help` 同形态的帮助全文。
    NoArgs,
    /// `--list`：名录单独成篇，无 banner/usage/脚注。
    List,
    /// `--list-full`：名录带路径成篇。
    ListFull,
    /// `--hel`：差一个字母，不逐字命中 → applet 名路径。
    Lookalike,
    /// `--HELP`：大小写翻车，同样不命中 → applet 名路径。
    CapsHelp,
    /// `--`：裸双横线也是"applet 名" → applet 名路径。
    DashDash,
}

impl Call {
    /// 七形态全集。
    pub const ALL: [Call; CALL_COUNT] = [
        Call::HelpLong,
        Call::NoArgs,
        Call::List,
        Call::ListFull,
        Call::Lookalike,
        Call::CapsHelp,
        Call::DashDash,
    ];

    /// 序号（0..=6）。
    pub fn index(&self) -> usize {
        match self {
            Call::HelpLong => 0,
            Call::NoArgs => 1,
            Call::List => 2,
            Call::ListFull => 3,
            Call::Lookalike => 4,
            Call::CapsHelp => 5,
            Call::DashDash => 6,
        }
    }

    /// 由序号还原（越界 → `None`）。
    pub fn from_index(i: usize) -> Option<Call> {
        match Call::ALL.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Call::HelpLong => "--help",
            Call::NoArgs => "(bare)",
            Call::List => "--list",
            Call::ListFull => "--list-full",
            Call::Lookalike => "--hel",
            Call::CapsHelp => "--HELP",
            Call::DashDash => "--",
        }
    }

    /// 长名（取证行主语）。
    pub fn name(&self) -> &'static str {
        match self {
            Call::HelpLong => "busybox --help",
            Call::NoArgs => "busybox（无参裸调）",
            Call::List => "busybox --list",
            Call::ListFull => "busybox --list-full",
            Call::Lookalike => "busybox --hel",
            Call::CapsHelp => "busybox --HELP",
            Call::DashDash => "busybox --",
        }
    }

    /// 是否是**帮助全文**形态（逐字命中或裸调）。
    pub fn is_help_face(&self) -> bool {
        matches!(self, Call::HelpLong | Call::NoArgs)
    }

    /// 是否是**名录成篇**形态（`--list` 家族）。
    pub fn is_list_face(&self) -> bool {
        matches!(self, Call::List | Call::ListFull)
    }

    /// 是否是"长得像但没命中"的 applet 名路径形态。
    pub fn is_applet_path(&self) -> bool {
        matches!(self, Call::Lookalike | Call::CapsHelp | Call::DashDash)
    }

    /// 顶层逐字匹配的串名（applet 路径形态返回它冒充的串，供取证）。
    pub fn flag(&self) -> &'static str {
        match self {
            Call::HelpLong => FLAG_HELP,
            Call::NoArgs => "",
            Call::List => FLAG_LIST,
            Call::ListFull => FLAG_LIST_FULL,
            Call::Lookalike => FLAG_LOOKALIKE,
            Call::CapsHelp => FLAG_CAPS,
            Call::DashDash => FLAG_DASHDASH,
        }
    }

    /// 这一形态**应当**产出的段位掩码（裸调与 `--help` 同为四段齐）。
    pub fn wanted_sections(&self) -> u8 {
        if self.is_help_face() {
            SEC_ALL
        } else if self.is_list_face() {
            SEC_LIST_ONLY
        } else {
            0
        }
    }

    /// 这一形态是否参与三方对账（`--list` 家族没有脚注 N，但仍对账名录数）。
    pub fn reconciles(&self) -> bool {
        !self.is_applet_path()
    }

    /// 折叠进指纹。
    pub fn fold(&self, h: u64) -> u64 {
        fold_word(h, self.index() as u64 + 1)
    }
}

// ---------------------------------------------------------------------------
// 段落
// ---------------------------------------------------------------------------

/// 帮助全文的一段（四段之一；`--list` 家族只有名录段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    /// banner 段（版权 + 版本行）。
    Banner,
    /// usage 段（三行骨架）。
    Usage,
    /// 脚注段（软链说明，首行嵌总数 N）。
    Foot,
    /// 名录段（`Currently defined functions:` + 全名单）。
    Listing,
}

impl Section {
    /// 四段全集（按拼装次序）。
    pub const ALL: [Section; SECTION_COUNT] = [
        Section::Banner,
        Section::Usage,
        Section::Foot,
        Section::Listing,
    ];

    /// 序号（0..=3）。
    pub fn index(&self) -> usize {
        match self {
            Section::Banner => 0,
            Section::Usage => 1,
            Section::Foot => 2,
            Section::Listing => 3,
        }
    }

    /// 由序号还原。
    pub fn from_index(i: usize) -> Option<Section> {
        match Section::ALL.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 段位掩码。
    pub fn bit(&self) -> u8 {
        match self {
            Section::Banner => SEC_BANNER,
            Section::Usage => SEC_USAGE,
            Section::Foot => SEC_FOOT,
            Section::Listing => SEC_LIST,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Section::Banner => "banner",
            Section::Usage => "usage",
            Section::Foot => "footnote",
            Section::Listing => "applet-list",
        }
    }

    /// 段内首行的逐字片段（`has_sub` 复核装配文本时用来点验）。
    pub fn anchor_text(&self) -> &'static str {
        match self {
            Section::Banner => "multi-call binary",
            Section::Usage => USAGE_HEAD,
            Section::Foot => "mutually-linked symbolic link",
            Section::Listing => LIST_HEADING,
        }
    }

    /// 该段落在第几道闸（段位缺失的定性闸；名录段归段落闸）。
    pub fn stage_rank(&self) -> u32 {
        // 四段都在段落闸（秩 3）点验；脚注段的**计数**问题才升级去对账闸。
        let _ = self;
        3
    }

    /// 折叠进指纹。
    pub fn fold(&self, h: u64) -> u64 {
        fold_word(h, self.bit() as u64)
    }
}

/// 掩码里数出的段位数（最多四，逐位扫）。
pub fn section_bits(mask: u8) -> u32 {
    let mut n: u32 = 0;
    let mut i: usize = 0;
    while i < SEC_BIT_COUNT {
        if let Some(b) = SEC_BITS.get(i) {
            if mask & *b != 0 {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 掩码是否只由已知的四位组成（第六位之类 = 坏形状）。
pub fn sections_in_bounds(mask: u8) -> bool {
    mask & !SEC_ALL == 0
}

/// 掩码里**第一个缺失**的段（按拼装次序）；全齐给 `None`。
pub fn first_missing(want: u8, have: u8) -> Option<Section> {
    let mut i: usize = 0;
    while i < SECTION_COUNT {
        let sec = match Section::ALL.get(i) {
            Some(v) => *v,
            None => return None,
        };
        if want & sec.bit() != 0 && have & sec.bit() == 0 {
            return Some(sec);
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 输出通道
// ---------------------------------------------------------------------------

/// 帮助文本的输出通道三档。
///
/// 帮助面的正判据是**纯 stdout**：`ToStderr` 与 `SplitWrite` 都是本卡通道闸的
/// 死档，`Nowhere`（退 0 却一字不出）是最隐蔽的 `SilentHelp` 现场。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chan {
    /// 全文入 stdout——唯一合法档。
    Stdout,
    /// 全文入 stderr——通道越界。
    Stderr,
    /// 一字不出——沉默档。
    Nowhere,
}

impl Chan {
    /// 三档全集。
    pub const ALL: [Chan; CHAN_COUNT] = [Chan::Stdout, Chan::Stderr, Chan::Nowhere];

    /// 序号（0..=2）。
    pub fn index(&self) -> usize {
        match self {
            Chan::Stdout => 0,
            Chan::Stderr => 1,
            Chan::Nowhere => 2,
        }
    }

    /// 由序号还原。
    pub fn from_index(i: usize) -> Option<Chan> {
        match Chan::ALL.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Chan::Stdout => "stdout",
            Chan::Stderr => "stderr",
            Chan::Nowhere => "nowhere",
        }
    }

    /// 是否是合法通道。
    pub fn is_help_channel(&self) -> bool {
        matches!(self, Chan::Stdout)
    }

    /// 折叠进指纹。
    pub fn fold(&self, h: u64) -> u64 {
        fold_word(h, self.index() as u64 + 1)
    }
}

// ---------------------------------------------------------------------------
// 七道闸
// ---------------------------------------------------------------------------

/// busybox 帮助面的纵线阶段（秩 1..=7，与文件头**闸门次序**一一对应）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 秩 1：argv 形状（有无 `argv[1]` 的分流）。
    Args,
    /// 秩 2：顶层形态识别（逐字命中三条串之一才算帮助调用）。
    Recognize,
    /// 秩 3：四段组装（banner/usage/脚注/名录逐位点验）。
    Sections,
    /// 秩 4：三方对账（脚注 N / 名录实列 / 构建内建）与字节截断。
    Reconcile,
    /// 秩 5：通道确认（全文只许走 stdout）。
    Channel,
    /// 秩 6：退出码结算（帮助面只认观测到的非零码为坏码）。
    ExitCode,
    /// 秩 7：收尾封印（唯一允许落全文锚的秩）。
    Settle,
}

impl Stage {
    /// 七闸全集（按秩升序）。
    pub const ALL: [Stage; STAGE_COUNT as usize] = [
        Stage::Args,
        Stage::Recognize,
        Stage::Sections,
        Stage::Reconcile,
        Stage::Channel,
        Stage::ExitCode,
        Stage::Settle,
    ];

    /// 秩（1 起算）。
    pub fn rank(&self) -> u32 {
        match self {
            Stage::Args => 1,
            Stage::Recognize => 2,
            Stage::Sections => 3,
            Stage::Reconcile => 4,
            Stage::Channel => 5,
            Stage::ExitCode => 6,
            Stage::Settle => 7,
        }
    }

    /// 由秩还原（越界 → `None`）。
    pub fn from_rank(r: u32) -> Option<Stage> {
        if r == 0 || r > STAGE_LAST_RANK {
            return None;
        }
        match r {
            1 => Some(Stage::Args),
            2 => Some(Stage::Recognize),
            3 => Some(Stage::Sections),
            4 => Some(Stage::Reconcile),
            5 => Some(Stage::Channel),
            6 => Some(Stage::ExitCode),
            _ => Some(Stage::Settle),
        }
    }

    /// 序号（0..=6）。
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
            Stage::Recognize => "recognize",
            Stage::Sections => "sections",
            Stage::Reconcile => "reconcile",
            Stage::Channel => "channel",
            Stage::ExitCode => "code",
            Stage::Settle => "settle",
        }
    }

    /// 长名。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Args => "argv 形状闸",
            Stage::Recognize => "顶层识别闸",
            Stage::Sections => "四段组装闸",
            Stage::Reconcile => "三方对账闸",
            Stage::Channel => "通道确认闸",
            Stage::ExitCode => "退出码闸",
            Stage::Settle => "收尾结算闸",
        }
    }

    /// 下一闸（第七闸没有下一闸）。
    pub fn next(&self) -> Option<Stage> {
        Stage::from_rank(self.rank() + 1)
    }

    /// 是否严格在前。
    pub fn before(&self, other: &Stage) -> bool {
        self.rank() < other.rank()
    }

    /// 是否属于"识别两闸"（还没组装任何一段文本）。
    pub fn is_intake(&self) -> bool {
        matches!(self, Stage::Args | Stage::Recognize)
    }

    /// 是否属于"内容三闸"（组装/对账/通道）。
    pub fn is_content(&self) -> bool {
        matches!(self, Stage::Sections | Stage::Reconcile | Stage::Channel)
    }

    /// 是否属于"结算两闸"。
    pub fn settles(&self) -> bool {
        matches!(self, Stage::ExitCode | Stage::Settle)
    }

    /// 这一闸是不是本卡的拒收闸（分发越界定死在这里）。
    pub fn is_refuse_gate(&self) -> bool {
        matches!(self, Stage::Recognize)
    }

    /// 折叠进指纹。
    pub fn fold(&self, h: u64) -> u64 {
        fold_word(h, self.rank() as u64)
    }
}

/// 秩 → 闸的一一映射自检用表（与 `Stage::ALL` 同序）。
pub const STAGE_RANKS: [u32; STAGE_COUNT as usize] = [1, 2, 3, 4, 5, 6, 7];

/// 判定次序的固定长度（Estate→Stage→Code→Value→Note→Default→Ok 共七步）。
pub const JUDGE_LEN: usize = 7usize;

/// 判定次序的逐步名（`judge` 与自检共用同一张表，防止两处漂移）。
pub const JUDGE_ORDER: [&'static str; JUDGE_LEN] = [
    "estate",
    "stage",
    "code",
    "value",
    "note",
    "default",
    "ok",
];

/// 判定次序是否严格单调（自检用：名不许重复、不许缺项）。
pub fn judge_order_is_strict() -> bool {
    let mut i = 0usize;
    while i < JUDGE_LEN {
        if let Some(a) = JUDGE_ORDER.get(i) {
            if !name_known_step(a) {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    JUDGE_ORDER.len() == JUDGE_LEN && STEPS.len() == JUDGE_LEN
}

/// 判定步的原始词表（六维加成功位；与 `JUDGE_ORDER` 逐字相同，两处互为锁）。
const STEPS: [&'static str; JUDGE_LEN] = [
    "estate", "stage", "code", "value", "note", "default", "ok",
];

/// 两张步名表是否逐字一致（防止判定次序被单独改动）。
pub fn judge_tables_agree() -> bool {
    let mut i = 0usize;
    while i < JUDGE_LEN {
        if let (Some(a), Some(b)) = (JUDGE_ORDER.get(i), STEPS.get(i)) {
            if !same_text(a, b) {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

/// 判定步名是否是已知步（与本表的其它项互不相同，故可用自身作词表）。
fn name_known_step(s: &str) -> bool {
    let mut hits = 0usize;
    let mut i = 0usize;
    while i < JUDGE_LEN {
        if let Some(w) = JUDGE_ORDER.get(i) {
            if same_text(w, s) {
                hits += 1;
            }
        }
        i += 1;
    }
    hits == 1
}

// ---------------------------------------------------------------------------
// 判据档位（Note）：一档一档手写，档位名直接对应现场报错
// ---------------------------------------------------------------------------

/// 档位总数（成功 4 + 内容 5 + 通道 4 + 出卡 3 = 16）。
pub const NOTE_COUNT: usize = 16usize;

/// 成功四档的下界。
pub const NOTE_FIRST_OK: u32 = 0u32;

/// 成功四档的上界。
pub const NOTE_LAST_OK: u32 = 3u32;

/// 缺段四档的下界。
pub const NOTE_FIRST_MISSING: u32 = 4u32;

/// 缺段四档的上界。
pub const NOTE_LAST_MISSING: u32 = 7u32;

/// 对账一档的秩。
pub const NOTE_RANK_COUNT: u32 = 8u32;

/// 内容族（缺段 + 对账）下界。
pub const NOTE_FIRST_CONTENT: u32 = 4u32;

/// 内容族上界。
pub const NOTE_LAST_CONTENT: u32 = 8u32;

/// 通道族下界。
pub const NOTE_FIRST_CHAN: u32 = 9u32;

/// 通道族上界（`ToStderr`/`SilentHelp`/`SplitWrite` 三档）。
pub const NOTE_LAST_CHAN: u32 = 11u32;

/// 退出码档的秩（单档成族，夹在通道族与出卡族之间）。
pub const NOTE_RANK_EXIT: u32 = 12u32;

/// 出卡族下界（分发/截断/溢出）。
pub const NOTE_FIRST_OUT: u32 = 13u32;

/// 出卡族上界。
pub const NOTE_LAST_OUT: u32 = 15u32;

/// 最后一档的秩。
pub const NOTE_LAST_RANK: u32 = 15u32;

/// 族数（成功 / 内容 / 通道 / 码 / 出卡 = 5 族覆盖 16 档）。
pub const NOTE_FAMILY_COUNT: usize = 5usize;

/// 现场判定的 16 档结论。
///
/// 每一档都是"这一格真机上会长成什么样"的名字，不是抽象错误类：
/// `SilentHelp` 就是"退了 0 但一个字节都没写"，`ToStderr` 就是"usage 全数进了 stderr"，
/// `FellApplet` 就是"形参没被特判、掉进 applet 名查找"。判据要能拒住现场，
/// 档位就必须按现场的差异切，而不是按代码模块切。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Note {
    /// 0：`busybox --help` 全文齐、stdout、退 0（本卡的正面锚）。
    DoneHelp,
    /// 1：裸 `busybox`（无参）同走帮助面，全文齐、退 0。
    DoneNoArgs,
    /// 2：`--list` 只出名录、退 0（名录成篇即合格）。
    DoneList,
    /// 3：`--list-full` 只出名录（带路径形式）、退 0。
    DoneListFull,
    /// 4：版权/版本 banner 段缺失。
    BannerMissing,
    /// 5：`Usage: busybox …` 三行缺失。
    UsageMissing,
    /// 6：脚注五行（含内嵌计数 N）缺失。
    FootMissing,
    /// 7：`Currently defined functions:` 名录段缺失。
    ListMissing,
    /// 8：脚注 N、实列个数、内建注册数三方对账不符。
    CountMismatch,
    /// 9：帮助全文被写到 stderr。
    ToStderr,
    /// 10：退 0 但零字节输出（沉默成功）。
    SilentHelp,
    /// 11：同一份文本同时写 stdout 与 stderr（重复出字）。
    SplitWrite,
    /// 12：帮助面退了非零码。
    BadExit,
    /// 13：形参掉进 applet 名查找路径（本卡拒住，归 F11674）。
    FellApplet,
    /// 14：输出被截断（字节数不足全文）。
    Truncated,
    /// 15：名录列出数超过内建注册数（虚增名录）。
    ListOverflow,
}

impl Note {
    /// 全部 16 档，按秩升序。
    pub const ALL: [Note; NOTE_COUNT] = [
        Note::DoneHelp,
        Note::DoneNoArgs,
        Note::DoneList,
        Note::DoneListFull,
        Note::BannerMissing,
        Note::UsageMissing,
        Note::FootMissing,
        Note::ListMissing,
        Note::CountMismatch,
        Note::ToStderr,
        Note::SilentHelp,
        Note::SplitWrite,
        Note::BadExit,
        Note::FellApplet,
        Note::Truncated,
        Note::ListOverflow,
    ];

    /// 档位秩（0..15，与 `ALL` 下标一致）。
    pub fn rank(&self) -> u32 {
        match self {
            Note::DoneHelp => 0,
            Note::DoneNoArgs => 1,
            Note::DoneList => 2,
            Note::DoneListFull => 3,
            Note::BannerMissing => 4,
            Note::UsageMissing => 5,
            Note::FootMissing => 6,
            Note::ListMissing => 7,
            Note::CountMismatch => 8,
            Note::ToStderr => 9,
            Note::SilentHelp => 10,
            Note::SplitWrite => 11,
            Note::BadExit => 12,
            Note::FellApplet => 13,
            Note::Truncated => 14,
            Note::ListOverflow => 15,
        }
    }

    /// 由秩还档；越界给 `None`（不猜、不兜底）。
    pub fn from_rank(r: u32) -> Option<Note> {
        if r > NOTE_LAST_RANK {
            return None;
        }
        let i = r as usize;
        Note::ALL.get(i).copied()
    }

    /// 数组下标（与秩同值，保留独立方法名以便表侧读起来清楚）。
    pub fn index(&self) -> usize {
        self.rank() as usize
    }

    /// 机器可读短名（进留痕、进指纹）。
    pub fn slug(&self) -> &'static str {
        match self {
            Note::DoneHelp => "bh-done-help",
            Note::DoneNoArgs => "bh-done-noargs",
            Note::DoneList => "bh-done-list",
            Note::DoneListFull => "bh-done-list-full",
            Note::BannerMissing => "bh-banner-missing",
            Note::UsageMissing => "bh-usage-missing",
            Note::FootMissing => "bh-foot-missing",
            Note::ListMissing => "bh-list-missing",
            Note::CountMismatch => "bh-count-mismatch",
            Note::ToStderr => "bh-to-stderr",
            Note::SilentHelp => "bh-silent-help",
            Note::SplitWrite => "bh-split-write",
            Note::BadExit => "bh-bad-exit",
            Note::FellApplet => "bh-fell-applet",
            Note::Truncated => "bh-truncated",
            Note::ListOverflow => "bh-list-overflow",
        }
    }

    /// 人读名（报告用）。
    pub fn name(&self) -> &'static str {
        match self {
            Note::DoneHelp => "全文帮助",
            Note::DoneNoArgs => "无参帮助",
            Note::DoneList => "名录成篇",
            Note::DoneListFull => "全路径名录",
            Note::BannerMissing => "版权段缺失",
            Note::UsageMissing => "usage 段缺失",
            Note::FootMissing => "脚注段缺失",
            Note::ListMissing => "名录段缺失",
            Note::CountMismatch => "三方计数不符",
            Note::ToStderr => "错走 stderr",
            Note::SilentHelp => "沉默成功",
            Note::SplitWrite => "双流重复",
            Note::BadExit => "帮助退非零",
            Note::FellApplet => "掉入 applet",
            Note::Truncated => "输出截断",
            Note::ListOverflow => "名录虚增",
        }
    }

    /// 是否成功档。
    pub fn is_success(&self) -> bool {
        let r = self.rank();
        r >= NOTE_FIRST_OK && r <= NOTE_LAST_OK
    }

    /// 是否缺段四档之一。
    pub fn is_missing(&self) -> bool {
        let r = self.rank();
        r >= NOTE_FIRST_MISSING && r <= NOTE_LAST_MISSING
    }

    /// 是否内容族（缺段 + 对账）。
    pub fn is_content(&self) -> bool {
        let r = self.rank();
        r >= NOTE_FIRST_CONTENT && r <= NOTE_LAST_CONTENT
    }

    /// 是否通道族（`ToStderr`/`SilentHelp`/`SplitWrite`）。
    pub fn is_chan(&self) -> bool {
        let r = self.rank();
        r >= NOTE_FIRST_CHAN && r <= NOTE_LAST_CHAN
    }

    /// 是否退出码档。
    pub fn is_exit(&self) -> bool {
        self.rank() == NOTE_RANK_EXIT
    }

    /// 是否出卡族（分发越界/截断/虚增）。
    pub fn is_out(&self) -> bool {
        let r = self.rank();
        r >= NOTE_FIRST_OUT && r <= NOTE_LAST_OUT
    }

    /// 族序号（0 成功 / 1 内容 / 2 通道 / 3 码 / 4 出卡；越界给 255）。
    pub fn family(&self) -> u32 {
        let r = self.rank();
        if r <= NOTE_LAST_OK {
            0
        } else if r <= NOTE_LAST_CONTENT {
            1
        } else if r <= NOTE_LAST_CHAN {
            2
        } else if r == NOTE_RANK_EXIT {
            3
        } else if r <= NOTE_LAST_OUT {
            4
        } else {
            255
        }
    }

    /// 族名。
    pub fn family_name(&self) -> &'static str {
        match self.family() {
            0 => "bh-fam-ok",
            1 => "bh-fam-content",
            2 => "bh-fam-chan",
            3 => "bh-fam-exit",
            4 => "bh-fam-out",
            _ => "bh-fam-none",
        }
    }

    /// 家族边界是否自洽（自检用：五族不重叠且严丝合缝覆盖 0..15）。
    pub fn families_partition() -> bool {
        let mut i = 0u32;
        while i <= NOTE_LAST_RANK {
            if let Some(n) = Note::from_rank(i) {
                if n.family() == 255 {
                    return false;
                }
            } else {
                return false;
            }
            i += 1;
        }
        NOTE_FIRST_OK == 0
            && NOTE_LAST_OK + 1 == NOTE_FIRST_CONTENT
            && NOTE_LAST_CONTENT + 1 == NOTE_FIRST_CHAN
            && NOTE_LAST_CHAN + 1 == NOTE_RANK_EXIT
            && NOTE_RANK_EXIT + 1 == NOTE_FIRST_OUT
            && NOTE_LAST_OUT == NOTE_LAST_RANK
            && NOTE_FIRST_MISSING == NOTE_FIRST_CONTENT
            && NOTE_RANK_COUNT == NOTE_LAST_CONTENT
            && NOTE_COUNT as u32 == NOTE_LAST_RANK + 1
    }

    /// 这一档由哪一闸提出（闸与档的纵向一致性检查表）。
    pub fn raised_by(&self) -> Stage {
        match self {
            Note::DoneHelp | Note::DoneNoArgs | Note::DoneList | Note::DoneListFull => {
                Stage::Settle
            }
            Note::BannerMissing | Note::UsageMissing | Note::FootMissing | Note::ListMissing => {
                Stage::Sections
            }
            Note::CountMismatch | Note::Truncated | Note::ListOverflow => Stage::Reconcile,
            Note::ToStderr | Note::SilentHelp | Note::SplitWrite => Stage::Channel,
            Note::BadExit => Stage::ExitCode,
            Note::FellApplet => Stage::Recognize,
        }
    }

    /// 这一档在真机上伴随的退出码（本卡的码档只有 0/1/127 三种真值）。
    pub fn exit_hint(&self) -> u32 {
        match self {
            Note::BadExit => EXIT_APPLET_FAIL,
            Note::FellApplet => EXIT_APPLET,
            _ => EXIT_OK,
        }
    }

    /// 是否可以作为全文锚（只有 `busybox --help` 的完整面能当锚，`--list` 不算）。
    pub fn anchor_candidate(&self) -> bool {
        matches!(self, Note::DoneHelp)
    }

    /// 是否属于"通道已经不可信"的一档（这类档位的 stdout 字节数不参与统计）。
    pub fn channel_untrusted(&self) -> bool {
        matches!(self, Note::ToStderr | Note::SplitWrite | Note::SilentHelp)
    }

    /// 是否属于名录面（只出名录，不承诺 banner/usage/foot）。
    pub fn list_face_only(&self) -> bool {
        matches!(self, Note::DoneList | Note::DoneListFull)
    }

    /// 是否要求"段落齐"（名录面只要求名录段，出卡族连段落都不承诺）。
    pub fn requires_full_sections(&self) -> bool {
        !self.list_face_only() && !self.is_out()
    }

    /// 折叠进指纹。
    pub fn fold(&self, h: u64) -> u64 {
        let v = fold_word(h, self.rank() as u64);
        fold_name(v, self.slug())
    }
}

/// 秩表（0..15），用于档位一一映射自检。
pub const NOTE_RANKS: [u32; NOTE_COUNT] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];

/// 闸→档族对照的公开入口：给定闸，返回该闸能提出的最秩档与最小秩档。
pub fn note_bounds_of_stage(st: &Stage) -> (u32, u32) {
    let mut lo = 0u32;
    let mut hi = 0u32;
    let mut first = true;
    let mut i = 0usize;
    while i < NOTE_COUNT {
        if let Some(n) = Note::ALL.get(i) {
            if n.raised_by() == *st {
                let r = n.rank();
                if first {
                    lo = r;
                    hi = r;
                    first = false;
                } else if r < lo {
                    lo = r;
                } else if r > hi {
                    hi = r;
                }
            }
        }
        i += 1;
    }
    if first {
        (NOTE_LAST_RANK + 1, 0)
    } else {
        (lo, hi)
    }
}

/// 闸能否提出某档（纵向一致性入口，供第 5 判据使用）。
pub fn stage_can_raise(st: &Stage, n: &Note) -> bool {
    st == &n.raised_by()
}

/// 全表纵向一致性：每一档都恰好被自己的闸提出，且成功档只落在结算闸。
pub fn notes_align_with_stages() -> bool {
    let mut i = 0usize;
    while i < NOTE_COUNT {
        if let Some(n) = Note::ALL.get(i) {
            let g = n.raised_by();
            if !stage_can_raise(&g, n) {
                return false;
            }
            if n.is_success() && g != Stage::Settle {
                return false;
            }
            if n.is_out() && g != Stage::Recognize && g != Stage::Reconcile {
                return false;
            }
            if n.is_chan() && g != Stage::Channel {
                return false;
            }
            if n.is_missing() && g != Stage::Sections {
                return false;
            }
            if n.is_exit() && g != Stage::ExitCode {
                return false;
            }
            if n.rank() == NOTE_RANK_COUNT && g != Stage::Reconcile {
                return false;
            }
            if n.anchor_candidate() && !n.is_success() {
                return false;
            }
            if n.rank() > NOTE_LAST_RANK {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    NOTE_COUNT == Note::ALL.len() && notes_unique()
}

/// 16 档的秩是否严格互不相同（档位表自身的唯一性）。
pub fn notes_unique() -> bool {
    let mut i = 0usize;
    while i < NOTE_COUNT {
        if let Some(a) = Note::ALL.get(i) {
            if a.rank() != i as u32 {
                return false;
            }
            if Note::from_rank(a.rank()) != Some(*a) {
                return false;
            }
            if !name_known(a.slug()) {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < NOTE_COUNT {
        if let Some(b) = Note::ALL.get(j) {
            let mut k = j + 1;
            while k < NOTE_COUNT {
                if let Some(c) = Note::ALL.get(k) {
                    if same_text(b.slug(), c.slug()) {
                        return false;
                    }
                }
                k += 1;
            }
        }
        j += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 事实档（Tier）：按"这一格现场属于哪一种帮助面"分堆
// ---------------------------------------------------------------------------

/// 四档事实：全文帮助 / 名录面 / 修补面 / 拒收面。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 全文四段齐 + 通道纯 + 码 0（本卡唯一可当锚的档）。
    HelpFull,
    /// 名录成篇（`--list`/`--list-full`），不承诺 banner。
    ListOnly,
    /// 退了 0 但内容有缺（缺段/通道不纯），算"活着的错"。
    Patched,
    /// 直接拒收（掉 applet 路径 / 码非零 / 计数虚增）。
    Refused,
}

impl Tier {
    /// 四档全列，按秩升序。
    pub const ALL: [Tier; TIER_COUNT] = [
        Tier::HelpFull,
        Tier::ListOnly,
        Tier::Patched,
        Tier::Refused,
    ];

    /// 档秩。
    pub fn rank(&self) -> u32 {
        match self {
            Tier::HelpFull => 0,
            Tier::ListOnly => 1,
            Tier::Patched => 2,
            Tier::Refused => 3,
        }
    }

    /// 下标。
    pub fn index(&self) -> usize {
        self.rank() as usize
    }

    /// 由秩还档。
    pub fn from_rank(r: u32) -> Option<Tier> {
        Tier::ALL.get(r as usize).copied()
    }

    /// 由下标还档（与 `from_rank` 等价，表侧读起来更清楚）。
    pub fn from_index(i: usize) -> Option<Tier> {
        Tier::ALL.get(i).copied()
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Tier::HelpFull => "bh-full",
            Tier::ListOnly => "bh-listy",
            Tier::Patched => "bh-patched",
            Tier::Refused => "bh-refused",
        }
    }

    /// 人读名。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::HelpFull => "全文帮助面",
            Tier::ListOnly => "名录面",
            Tier::Patched => "带病放行面",
            Tier::Refused => "拒收面",
        }
    }

    /// 是否可作为全文锚来源。
    pub fn anchors(&self) -> bool {
        matches!(self, Tier::HelpFull)
    }

    /// 是否属于"成功但内容不完整"的档（报表里的灰区，专门用来防"只看退出码"的假绿）。
    pub fn is_greenish(&self) -> bool {
        matches!(self, Tier::Patched)
    }

    /// 该档是否要求 stdout 有全文字节。
    pub fn needs_stdout(&self) -> bool {
        matches!(self, Tier::HelpFull | Tier::Patched)
    }

    /// 由现场结论分档。
    ///
    /// 次序是刻意的：**锚位优先**（只有全文帮助面能被别卡当锚引用），其次看码，
    /// 码非零一律拒收；再看向出卡族（即便侥幸退 0，截断与虚增也按拒收记），
    /// 最后才分名录面与带病面。这样"只看退出码就把 14 格涂绿"的假绿在本卡不成立。
    pub fn of(o: &Outcome) -> Tier {
        if o.default {
            Tier::HelpFull
        } else if !o.ok() {
            Tier::Refused
        } else if o.note.is_out() {
            Tier::Refused
        } else if o.note.list_face_only() {
            Tier::ListOnly
        } else {
            Tier::Patched
        }
    }

    /// 折叠进指纹。
    pub fn fold(&self, h: u64) -> u64 {
        let v = fold_word(h, self.rank() as u64);
        fold_name(v, self.slug())
    }
}

/// 四档秩表。
pub const TIER_RANKS: [u32; TIER_COUNT] = [0, 1, 2, 3];

/// 报表里四档计数的字段名（渲染与自检共用，防字段顺序漂）。
pub const TIER_FIELDS: [&'static str; TIER_COUNT] = ["full", "listy", "patched", "refused"];

/// 分档函数的排他性自检：同一现场只能落一档，且四档覆盖全部可能。
pub fn tiers_are_exclusive() -> bool {
    let mut i = 0usize;
    while i < NOTE_COUNT {
        if let Some(n) = Note::ALL.get(i) {
            let ok = matches!(i, 0usize | 1usize | 2usize | 3usize);
            let shape_default = ok;
            let t = Tier::from_rank(i as u32 % 4);
            if t.is_none() {
                return false;
            }
            if shape_default && n.rank() > NOTE_LAST_OK {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    TIER_RANKS.len() == TIER_COUNT && TIER_FIELDS.len() == TIER_COUNT
}

// ---------------------------------------------------------------------------
// 缺陷类（Fault）：六维判定的结果类
// ---------------------------------------------------------------------------

/// 判定结果类：成功 + 六维缺陷。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 六维全合。
    Ok,
    /// 成败位本身说谎（现场成功与否被声称反了）。
    Estate,
    /// 报错闸口位置不对。
    Stage,
    /// 错误码不在本卡的段位上。
    Code,
    /// 计数/字节数与现场不符。
    Value,
    /// 结论档位与现场表现不符。
    Note,
    /// 默认位（能不能当全文锚）判错。
    Default,
}

/// 缺陷档总数。
pub const FAULT_COUNT: usize = 7usize;

/// 判定维数（不含成功位）。
pub const DIM_COUNT: usize = 6usize;

impl Fault {
    /// 全列，按秩升序。
    pub const ALL: [Fault; FAULT_COUNT] = [
        Fault::Ok,
        Fault::Estate,
        Fault::Stage,
        Fault::Code,
        Fault::Value,
        Fault::Note,
        Fault::Default,
    ];

    /// 秩。
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

    /// 下标。
    pub fn index(&self) -> usize {
        self.rank() as usize
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Fault::Ok => "bh-ok",
            Fault::Estate => "bh-estate",
            Fault::Stage => "bh-stage",
            Fault::Code => "bh-code",
            Fault::Value => "bh-value",
            Fault::Note => "bh-note",
            Fault::Default => "bh-default",
        }
    }

    /// 人读名。
    pub fn name(&self) -> &'static str {
        match self {
            Fault::Ok => "吻合",
            Fault::Estate => "成败位说谎",
            Fault::Stage => "报错闸口错位",
            Fault::Code => "错误码出段",
            Fault::Value => "计数与现场不符",
            Fault::Note => "结论档位不符",
            Fault::Default => "锚位判错",
        }
    }

    /// 该缺陷类在本卡命中的错误码（成功类给 0）。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Estate => BH_ESTATE,
            Fault::Stage => BH_ESTAGE,
            Fault::Code => BH_ECODE,
            Fault::Value => BH_ELIST,
            Fault::Note => BH_ENOTE,
            Fault::Default => BH_EANCH,
        }
    }

    /// 是否缺陷（非成功）。
    pub fn is_fault(&self) -> bool {
        self.rank() > 0
    }

    /// 该缺陷类由哪一维判出（与 `JUDGE_ORDER` 前六项对应）。
    pub fn dim(&self) -> &'static str {
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

    /// 一句话说明（报表与文档共用）。
    pub fn blurb(&self) -> &'static str {
        match self {
            Fault::Ok => "六维全合，该格与基准机逐字一致。",
            Fault::Estate => "现场成功与否被声称反了：先把成败位钉住，后面六维才有意义。",
            Fault::Stage => "报错闸口错位：同样的错误在第四闸报和在第二闸报，是两种实现。",
            Fault::Code => "错误码出段：本卡只认 0/1/127 三档真值码，其余一律算出段。",
            Fault::Value => "计数与现场不符：名录个数、脚注 N、字节数三者任一处对不上实机。",
            Fault::Note => "结论档位不符：现场是沉默成功却记成错走 stderr，档位判错。",
            Fault::Default => "锚位判错：只有全文帮助面能当锚，名录面与带病面都不许当锚。",
        }
    }

    /// 折叠进指纹。
    pub fn fold(&self, h: u64) -> u64 {
        let v = fold_word(h, self.rank() as u64);
        fold_name(v, self.slug())
    }
}

/// 六档缺陷（不含成功）的顺序表，与死槽一一对应。
pub const REJECT_FAULTS: [Fault; DIM_COUNT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

/// 六档缺陷命中的错误码（互不相同，全部在本卡段内）。
pub const REJECT_CODES: [u32; DIM_COUNT] = [
    BH_ESTATE,
    BH_ESTAGE,
    BH_ECODE,
    BH_ELIST,
    BH_ENOTE,
    BH_EANCH,
];

/// 缺陷码表自检：六码互不相同且全部落在本卡段内。
pub fn reject_codes_distinct() -> bool {
    let mut i = 0usize;
    while i < DIM_COUNT {
        if let Some(c) = REJECT_CODES.get(i) {
            if *c < SEG_BASE || *c >= SEG_BASE + 0x10 {
                return false;
            }
        } else {
            return false;
        }
        let mut j = i + 1;
        while j < DIM_COUNT {
            if let (Some(a), Some(b)) = (REJECT_CODES.get(i), REJECT_CODES.get(j)) {
                if a == b {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 由错误码反查缺陷类（越段给 `None`）。
pub fn fault_of_code(code: u32) -> Option<Fault> {
    let mut i = 0usize;
    while i < FAULT_COUNT {
        if let Some(f) = Fault::ALL.get(i) {
            if f.code() == code {
                return Some(*f);
            }
        }
        i += 1;
    }
    None
}

/// 由维名反查缺陷类（`JUDGE_ORDER` 侧的入口）。
pub fn fault_of_dim(dim: &str) -> Option<Fault> {
    let mut i = 0usize;
    while i < FAULT_COUNT {
        if let Some(f) = Fault::ALL.get(i) {
            if same_text(f.dim(), dim) {
                return Some(*f);
            }
        }
        i += 1;
    }
    None
}

/// 判定次序与缺陷类是否互相覆盖（第七判据的机器面）。
pub fn judge_maps_faults() -> bool {
    let mut i = 0usize;
    while i < JUDGE_LEN {
        if let Some(step) = JUDGE_ORDER.get(i) {
            if fault_of_dim(step).is_none() {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < DIM_COUNT {
        if let Some(f) = REJECT_FAULTS.get(k) {
            if f.code() == 0 {
                return false;
            }
            if fault_of_code(f.code()) != Some(*f) {
                return false;
            }
        } else {
            return false;
        }
        k += 1;
    }
    true
}

/// 缺陷档的秩表（0..6），与 `Fault::ALL` 同序。
pub const FAULT_RANKS: [u32; FAULT_COUNT] = [0, 1, 2, 3, 4, 5, 6];

/// 六档缺陷的短名表（不含成功），与 `REJECT_FAULTS` 同序。
pub const REJECT_SLUGS: [&'static str; DIM_COUNT] = [
    "bh-estate",
    "bh-stage",
    "bh-code",
    "bh-value",
    "bh-note",
    "bh-default",
];

/// 缺陷类与错误码、短名三表是否同源（第 4 判据的机器面）。
pub fn fault_tables_agree() -> bool {
    let mut i = 0usize;
    while i < DIM_COUNT {
        let f = match REJECT_FAULTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let c = match REJECT_CODES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let s = match REJECT_SLUGS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if f.code() != c {
            return false;
        }
        if !same_text(f.slug(), s) {
            return false;
        }
        if fault_of_code(c) != Some(f) {
            return false;
        }
        if !same_text(code_name(c), f.slug()) {
            return false;
        }
        i += 1;
    }
    FAULT_RANKS.len() == FAULT_COUNT
}

// ---------------------------------------------------------------------------
// 现场（Shape）：一次 busybox 顶层调用的事实面
// ---------------------------------------------------------------------------

/// 一次顶层调用的**事实面**：观测到什么就记什么，不含任何结论。
///
/// 十一个字段分三组：
/// - 形参组（`call`）：这次调用长得像帮助、像名录，还是已经掉进 applet 名查找；
/// - 内容组（`sections`/`foot_n`/`listed`/`registered`）：装配出的段、脚注 N、
///   名录实际条数、内建注册数——本卡最硬的逐字判据全押在这一组；
/// - 出字组（`chan`/`split_out`/`bytes_out`/`need_bytes`/`code_seen`/`fell_applet`）：
///   走哪个通道、有没有重复出字、stdout 实际字节数、全文应有字节数、退出码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 顶层形参形态（七种之一）。
    pub call: Call,
    /// 实际装配出的段位（`SEC_*` 的并集）。
    pub sections: u8,
    /// 脚注里内嵌的那个 N（"Each of the above 413 functions …"）。
    pub foot_n: u32,
    /// 名录段实际列出的函数名条数。
    pub listed: u32,
    /// 该 build 实际注册的 applet 数（基准机 413）。
    pub registered: u32,
    /// 帮助文本走的主通道。
    pub chan: Chan,
    /// 观测到的退出码（可能与事实不符，本卡只作对照）。
    pub code_seen: u32,
    /// stdout 实际吐出的字节数。
    pub bytes_out: u32,
    /// 全文应有的字节数（基准机快照长度；0 表示本槽不承诺字节数）。
    pub need_bytes: u32,
    /// 形参是否掉进了 `find_applet_by_name`。
    pub fell_applet: bool,
    /// 是否同一份文本同时写了 stdout 与 stderr。
    pub split_out: bool,
}

impl Shape {
    /// 全参数构造（十一参数，语料表之外的场合用）。
    pub const fn new(
        call: Call,
        sections: u8,
        foot_n: u32,
        listed: u32,
        registered: u32,
        chan: Chan,
        code_seen: u32,
        bytes_out: u32,
        need_bytes: u32,
        fell_applet: bool,
        split_out: bool,
    ) -> Shape {
        Shape {
            call,
            sections,
            foot_n,
            listed,
            registered,
            chan,
            code_seen,
            bytes_out,
            need_bytes,
            fell_applet,
            split_out,
        }
    }

    /// 基准现场：`busybox --help` 在基准机上的全绿形态（四段齐、三方一致、
    /// stdout、退 0、字节数等于快照）。语料每一格都由它做结构更新派生，
    /// 这样"只改一个维度"在语料表里是显式的，不会被整行抄写掩盖。
    pub const BASE: Shape = Shape {
        call: Call::HelpLong,
        sections: SEC_ALL,
        foot_n: ANCHOR_REGISTERED,
        listed: ANCHOR_REGISTERED,
        registered: ANCHOR_REGISTERED,
        chan: Chan::Stdout,
        code_seen: EXIT_OK,
        bytes_out: ANCHOR_HELP_BYTES,
        need_bytes: ANCHOR_HELP_BYTES,
        fell_applet: false,
        split_out: false,
    };

    /// 名录面基准现场：`busybox --list` 的基准形态（只有名录段）。
    pub const BASE_LIST: Shape = Shape {
        call: Call::List,
        sections: SEC_LIST_ONLY,
        foot_n: 0,
        bytes_out: ANCHOR_REGISTERED,
        need_bytes: ANCHOR_REGISTERED,
        ..Shape::BASE
    };

    /// 本格的形参是否走帮助面。
    pub fn is_help_face(&self) -> bool {
        self.call.is_help_face() && !self.fell_applet
    }

    /// 本格的形参是否走名录面。
    pub fn is_list_face(&self) -> bool {
        self.call.is_list_face() && !self.fell_applet
    }

    /// 本格是否已经掉进 applet 名路径。
    pub fn applet_path(&self) -> bool {
        self.call.is_applet_path() || self.fell_applet
    }

    /// 本格应装配的段位掩码。
    pub fn want_sections(&self) -> u8 {
        self.call.wanted_sections()
    }

    /// 实际段是否含所需全部段。
    pub fn has_wanted(&self) -> bool {
        let want = self.want_sections();
        (self.sections & want) == want
    }

    /// 第一个缺失的所需段（齐则 `None`）。
    pub fn first_missing(&self) -> Option<Section> {
        first_missing(self.want_sections(), self.sections)
    }

    /// 三方对账（脚注 N == 实列数 == 内建注册数）是否成立。
    ///
    /// 名录面没有脚注，所以只查实列数与注册数两方；`foot_n == 0` 在这里
    /// 被显式当作"本格不承诺脚注"（由 `Call::reconciles` 决定），
    /// 而不是"脚注说 0 条"。
    pub fn reconciles(&self) -> bool {
        if self.call.reconciles() {
            self.foot_n == self.listed && self.listed == self.registered
        } else {
            self.listed == self.registered
        }
    }

    /// 名录是否虚增（实列数超过注册数）。
    pub fn overflow(&self) -> bool {
        self.listed > self.registered
    }

    /// 脚注 N 是否与注册数不符（对账失败的另一半）。
    pub fn foot_disagrees(&self) -> bool {
        self.call.reconciles() && self.foot_n != self.registered
    }

    /// 输出是否被截断（实际字节数不够全文）。
    pub fn truncated(&self) -> bool {
        self.need_bytes > 0 && self.bytes_out < self.need_bytes
    }

    /// 是否零字节（沉默）。
    pub fn silent(&self) -> bool {
        self.bytes_out == 0
    }

    /// 通道是否纯净（只走 stdout，且没有双流重复出字）。
    pub fn channel_pure(&self) -> bool {
        self.chan.is_help_channel() && !self.split_out
    }

    /// 本格的 stdout 文本（逐字判据的对象）。
    pub fn stdout_text(&self) -> String {
        if self.applet_path() {
            msg_applet_not_found(self.call.name())
        } else {
            assemble_help(self.sections, self.foot_n)
        }
    }

    /// 折叠十一维进指纹（顺序固定：形参→段→三计数→通道→码→字节→两布尔）。
    pub fn fold(&self, h: u64) -> u64 {
        let h = self.call.fold(h);
        let h = fold_word(h, self.sections as u64);
        let h = fold_word(h, self.foot_n as u64);
        let h = fold_word(h, self.listed as u64);
        let h = fold_word(h, self.registered as u64);
        let h = self.chan.fold(h);
        let h = fold_word(h, self.code_seen as u64);
        let h = fold_word(h, self.bytes_out as u64);
        let h = fold_word(h, self.need_bytes as u64);
        let h = fold_flag(h, self.fell_applet);
        fold_flag(h, self.split_out)
    }

    /// 一行渲染（取证用）。
    pub fn render(&self) -> String {
        let mut s = String::from("shape ");
        s.push_str(self.call.slug());
        sep(&mut s);
        kv(&mut s, "sec", self.sections as u32);
        sep(&mut s);
        kv(&mut s, "foot", self.foot_n);
        sep(&mut s);
        kv(&mut s, "listed", self.listed);
        sep(&mut s);
        kv(&mut s, "reg", self.registered);
        sep(&mut s);
        s.push_str("chan=");
        s.push_str(self.chan.slug());
        sep(&mut s);
        kv(&mut s, "exit", self.code_seen);
        sep(&mut s);
        kv(&mut s, "bytes", self.bytes_out);
        sep(&mut s);
        kv(&mut s, "need", self.need_bytes);
        sep(&mut s);
        s.push_str("fell=");
        s.push_str(bool_text(self.fell_applet).as_str());
        sep(&mut s);
        s.push_str("split=");
        s.push_str(bool_text(self.split_out).as_str());
        s
    }

    /// 现场是否自相矛盾（段掩码越界，或名录虚增）。
    pub fn contradictory(&self) -> bool {
        !sections_in_bounds(self.sections) || self.overflow()
    }
}

/// 缺段档位的映射：段 → 该段缺失时的档位名（`define` 与报表共用）。
pub fn note_of_missing(sec: &Section) -> Note {
    match sec {
        Section::Banner => Note::BannerMissing,
        Section::Usage => Note::UsageMissing,
        Section::Foot => Note::FootMissing,
        Section::Listing => Note::ListMissing,
    }
}

/// 反向映射：缺段档 → 段（非缺段档给 `None`）。
pub fn section_of_missing(n: &Note) -> Option<Section> {
    match n {
        Note::BannerMissing => Some(Section::Banner),
        Note::UsageMissing => Some(Section::Usage),
        Note::FootMissing => Some(Section::Foot),
        Note::ListMissing => Some(Section::Listing),
        _ => None,
    }
}

/// 缺段映射是否双向一致（第 5 判据的机器面）。
pub fn missing_map_is_inverse() -> bool {
    let mut i = 0usize;
    while i < SEC_BIT_COUNT {
        let sec = match Section::ALL.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let n = note_of_missing(&sec);
        if !n.is_missing() {
            return false;
        }
        if section_of_missing(&n) != Some(sec) {
            return false;
        }
        if n.raised_by() != Stage::Sections {
            return false;
        }
        i += 1;
    }
    section_of_missing(&Note::BadExit).is_none()
}

// ---------------------------------------------------------------------------
// 结论（Outcome）：本卡对现场算出的事实结论
// ---------------------------------------------------------------------------

/// 本卡对一次调用算出的**事实结论**（六维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 定性落在第几道闸。
    pub stage: Stage,
    /// 这一现场**应当**有的退出码（由闸序算出，不是抄观测）。
    pub code: u32,
    /// 交付给报表的计数（默认是名录条数，截断档换成已吐字节数）。
    pub value: u32,
    /// 现场档位。
    pub note: Note,
    /// 锚位：本格能否被后续卡当全文锚引用。
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

    /// 成败位：退 0 才是成。1、127 都是败。
    pub fn ok(&self) -> bool {
        self.code == EXIT_OK
    }

    /// 是否本卡拒收（掉 applet / 码非零 / 出卡族）。
    pub fn refused(&self) -> bool {
        self.note.is_out() || !self.ok()
    }

    /// 是否属于"沉默成功"这一类最危险的现场。
    pub fn is_silent(&self) -> bool {
        matches!(self.note, Note::SilentHelp)
    }

    /// 是否需要 stderr 出字（成功四档不写 stderr）。
    pub fn prints(&self) -> bool {
        !self.note.is_success()
    }

    /// 本结论允许被当锚吗（与 `note.anchor_candidate` 同步才成立）。
    pub fn can_anchor(&self) -> bool {
        self.default && self.note.anchor_candidate()
    }

    /// 该结论对应的码档在 `EXIT_TABLE` 里的下标（越界给 `None`）。
    pub fn exit_rank(&self) -> Option<u32> {
        let mut i = 0usize;
        while i < EXIT_TABLE.len() {
            if let Some(v) = EXIT_TABLE.get(i) {
                if *v == self.code {
                    return Some(i as u32);
                }
            }
            i += 1;
        }
        None
    }

    /// 折叠五维进指纹（固定次序：闸→码→计数→档位→锚位）。
    pub fn fold(&self, h: u64) -> u64 {
        let h = self.stage.fold(h);
        let h = fold_word(h, self.code as u64);
        let h = fold_word(h, self.value as u64);
        let h = fold_word(h, self.note.rank() as u64);
        fold_flag(h, self.default)
    }

    /// 一行渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("stage=");
        s.push_str(self.stage.slug());
        sep(&mut s);
        s.push_str("exit=");
        s.push_str(dec_text(self.code).as_str());
        sep(&mut s);
        s.push_str("count=");
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

/// 本卡的全部判理集中在这一函数里：按闸序逐闸提问，第一处不合规即定案。
///
/// 闸序与真机的执行序一致（识别 → 段落 → 对账 → 通道 → 码 → 结算），
/// 因此"错误在哪一闸报"不是叙事选择，而是现场因果；把闸序写反，
/// 同一次 `--help` 缺 usage 会被误记成"码不对"，本卡就失去了区分力。
pub fn define(s: &Shape) -> Outcome {
    // 闸 1/2：形参识别。掉进 applet 名查找的一律在本卡第二闸定死，
    // 码是 127，档位是 FellApplet，锚位一律不给（未名路径归 F11674）。
    if s.applet_path() {
        return Outcome::new(Stage::Recognize, EXIT_APPLET, 0, Note::FellApplet, false);
    }

    // 名录面（--list / --list-full）：只承诺名录段，不承诺 banner/usage/脚注。
    if s.is_list_face() {
        if s.sections & SEC_LIST == 0 {
            return Outcome::new(Stage::Sections, EXIT_OK, s.listed, Note::ListMissing, false);
        }
        if s.overflow() {
            return Outcome::new(Stage::Reconcile, EXIT_OK, s.listed, Note::ListOverflow, false);
        }
        if s.listed != s.registered {
            return Outcome::new(Stage::Reconcile, EXIT_OK, s.listed, Note::CountMismatch, false);
        }
        if !s.channel_pure() {
            let n = if s.split_out {
                Note::SplitWrite
            } else if s.silent() {
                Note::SilentHelp
            } else {
                Note::ToStderr
            };
            return Outcome::new(Stage::Channel, EXIT_OK, s.listed, n, false);
        }
        if s.code_seen != EXIT_OK {
            return Outcome::new(Stage::ExitCode, s.code_seen, s.listed, Note::BadExit, false);
        }
        let n = if s.call == Call::ListFull {
            Note::DoneListFull
        } else {
            Note::DoneList
        };
        return Outcome::new(Stage::Settle, EXIT_OK, s.listed, n, false);
    }

    // 帮助面（--help / 裸调用）：四段必须齐。
    if let Some(sec) = s.first_missing() {
        return Outcome::new(Stage::Sections, EXIT_OK, s.listed, note_of_missing(&sec), false);
    }

    // 内容闸：三方对账（脚注 N / 实列数 / 注册数）。虚增优先于一般不符：
    // 虚增意味着名录被编造，是一般不符里最严重的一种。
    if s.overflow() {
        return Outcome::new(Stage::Reconcile, EXIT_OK, s.listed, Note::ListOverflow, false);
    }
    if !s.reconciles() {
        return Outcome::new(Stage::Reconcile, EXIT_OK, s.listed, Note::CountMismatch, false);
    }

    // 内容闸的第二问：字节数。全文被截断时段位掩码可以仍是齐的
    // （实现按段拼接、末段被写断），所以这一问排在对账之后、通道之前。
    if s.truncated() {
        return Outcome::new(Stage::Reconcile, EXIT_OK, s.bytes_out, Note::Truncated, false);
    }

    // 通道闸：帮助全文只许进 stdout，且只许进一次。
    if s.split_out {
        return Outcome::new(Stage::Channel, EXIT_OK, s.listed, Note::SplitWrite, false);
    }
    match s.chan {
        Chan::Nowhere => {
            return Outcome::new(Stage::Channel, EXIT_OK, s.listed, Note::SilentHelp, false);
        }
        Chan::Stderr => {
            return Outcome::new(Stage::Channel, EXIT_OK, s.listed, Note::ToStderr, false);
        }
        Chan::Stdout => {}
    }

    // 码闸：帮助面只认 0。
    if s.code_seen != EXIT_OK {
        return Outcome::new(Stage::ExitCode, s.code_seen, s.listed, Note::BadExit, false);
    }

    // 结算闸：全绿。裸调用与 `--help` 在这一档分开记，
    // 因为"无参也出帮助"是 BusyBox 的额外承诺，不能与显式 `--help` 混成一档。
    let n = if s.call == Call::NoArgs {
        Note::DoneNoArgs
    } else {
        Note::DoneHelp
    };
    Outcome::new(Stage::Settle, EXIT_OK, s.listed, n, true)
}

/// 该现场在基准机上应打印的 stderr 文本（成功档为空串）。
pub fn message(s: &Shape, o: &Outcome) -> String {
    match o.note {
        Note::DoneHelp | Note::DoneNoArgs | Note::DoneList | Note::DoneListFull => String::new(),
        Note::BannerMissing | Note::UsageMissing | Note::FootMissing | Note::ListMissing => {
            match section_of_missing(&o.note) {
                Some(sec) => msg_missing_section(&sec),
                None => msg_missing_section(&Section::Usage),
            }
        }
        Note::CountMismatch | Note::ListOverflow => {
            msg_count_disagree(s.foot_n, s.listed, s.registered)
        }
        Note::Truncated => msg_truncated(s.bytes_out, s.need_bytes),
        Note::ToStderr => msg_wrong_channel(&Chan::Stderr),
        Note::SplitWrite => msg_split_write(),
        Note::SilentHelp => msg_silent_help(),
        Note::BadExit => msg_bad_exit(o.code),
        Note::FellApplet => msg_applet_not_found(s.call.flag()),
    }
}

/// 双流重复出字的取证行（`msg_wrong_channel` 只描述"走错"，重复出字是另一种坏法）。
pub fn msg_split_write() -> String {
    let mut s = String::from("busybox --help: usage text written twice (stdout + stderr) — ");
    s.push_str("one face, one channel");
    s
}

/// 该现场在 stdout 上应出现的逐字文本（名录面与帮助面共用装配器）。
///
/// 掉 applet 路径、错走 stderr、沉默成功三档的 stdout 一律为空——
/// 这一句就是本卡"stdout 通道判据"的全部含义。
pub fn stdout_message(s: &Shape, o: &Outcome) -> String {
    match o.note {
        Note::FellApplet | Note::ToStderr | Note::SilentHelp => String::new(),
        _ => assemble_help(s.sections, s.foot_n),
    }
}

/// 结论 + 现场的完整取证行（报表与账本共用）。
pub fn evidence(s: &Shape, o: &Outcome) -> String {
    let mut m = message(s, o);
    if m.is_empty() {
        m = String::from("busybox --help: (no stderr)");
    }
    m.push_str(" | ");
    m.push_str(o.render().as_str());
    m
}

/// 通道的逐字留痕：先 stdout 后 stderr，末行写码。本卡"与基准机一致"
/// 比较的就是这一串，比较函数是 `same_text`，不做任何归一化。
pub fn transcript(s: &Shape, o: &Outcome) -> String {
    let mut t = String::from("OUT<");
    t.push_str(stdout_message(s, o).as_str());
    t.push_str(">|ERR<");
    t.push_str(message(s, o).as_str());
    t.push_str(">|EXIT=");
    t.push_str(dec_text(o.code).as_str());
    t
}

// ---------------------------------------------------------------------------
// 申报（Claim）与案例（Case）
// ---------------------------------------------------------------------------

/// 被复核的那一份结论：六维申报外加一个显式成败位。
///
/// 成败位必须单独存在：本卡最典型的一类事故不是码抄错，而是**验收记录里写着
/// "help 成功"**，而现场明明一个字都没出（`SilentHelp`）。只比五维的话这一格会漏网。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 申报的成败（本次调用是否交付了完整的帮助面）。
    pub success: bool,
    /// 申报的定性闸。
    pub stage: Stage,
    /// 申报的退出码。
    pub code: u32,
    /// 申报的计数（名录条数或已吐字节数）。
    pub value: u32,
    /// 申报的档位。
    pub note: Note,
    /// 申报的锚位（能否当全文锚）。
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

    /// 申报折算出的退出码（只区分 0/非 0）。
    pub fn exit_hint(&self) -> u32 {
        if self.success {
            EXIT_OK
        } else {
            EXIT_APPLET_FAIL
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
        let h = fold_word(h, self.code as u64);
        let h = fold_word(h, self.value as u64);
        let h = fold_word(h, self.note.rank() as u64);
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
        s.push_str("count=");
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
        Claim { stage: v, ..*self }
    }

    /// 只翻退出码维。
    pub fn flip_code(&self, v: u32) -> Claim {
        Claim { code: v, ..*self }
    }

    /// 只翻计数维。
    pub fn flip_value(&self, v: u32) -> Claim {
        Claim { value: v, ..*self }
    }

    /// 只翻档位维。
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

    /// 该申报是否把带病现场申报成了可锚（本卡最狠的一种谎）。
    pub fn claims_anchor_without_full(&self) -> bool {
        self.default && !self.note.anchor_candidate()
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

    /// 本案例的事实结论。
    pub fn actual(&self) -> Outcome {
        define(&self.shape)
    }

    /// 本案例的判定。
    pub fn verdict(&self) -> Fault {
        judge(self)
    }

    /// 取证行。
    pub fn evidence(&self) -> String {
        evidence(&self.shape, &self.actual())
    }
}

/// 六维比对（判序固定：成败 → 闸 → 码 → 计数 → 档位 → 锚位）。
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

/// 第 i 槽的判定（越界给 `None`）。
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

/// 第 i 槽的事实结论（越界给 `None`）。
pub fn outcome_at(i: usize) -> Option<Outcome> {
    match SHAPES.get(i) {
        Some(v) => Some(define(v)),
        None => None,
    }
}

/// 判定是否**只**由被翻的那一维决定（死槽的构造性自检）。
pub fn fault_is_localized(i: usize) -> bool {
    let want = match WANT_NDX.get(i) {
        Some(v) => *v as usize,
        None => return false,
    };
    let got = match verdict_at(i) {
        Some(v) => v,
        None => return false,
    };
    let expect = match Fault::ALL.get(want) {
        Some(v) => *v,
        None => return false,
    };
    if got != expect {
        return false;
    }
    if want == 0 {
        return true;
    }
    let shape = match SHAPES.get(i) {
        Some(v) => *v,
        None => return false,
    };
    let claim = match CLAIMS.get(i) {
        Some(v) => *v,
        None => return false,
    };
    if claim.claims_anchor_without_full() {
        return false;
    }
    let actual = define(&shape);
    let healed = Claim::from_outcome(&actual, actual.ok());
    judge(&Case::new(shape, healed)) == Fault::Ok
}

/// `hay` 是否以 `needle` 结尾（死槽 `*-swap` 尾缀点验用，逐字节比较）。
pub fn ends_with(hay: &str, needle: &str) -> bool {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.len() > h.len() {
        return false;
    }
    let off = h.len() - n.len();
    let mut i = 0usize;
    while i < n.len() {
        let a = match h.get(off + i) {
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

/// `hay` 中 `needle` 出现的次数（重叠窗口计数，文案点验用）。
pub fn count_sub(hay: &str, needle: &str) -> u32 {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || n.len() > h.len() {
        return 0;
    }
    let last = h.len() - n.len();
    let mut hits = 0u32;
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
            hits = hits.saturating_add(1);
        }
        i += 1;
    }
    hits
}

/// 表里字符串是否互不相同（标签唯一性点验用）。
pub fn strs_distinct(items: &[&str]) -> bool {
    let mut i = 0usize;
    while i < items.len() {
        let mut j = i + 1;
        while j < items.len() {
            if let (Some(a), Some(b)) = (items.get(i), items.get(j)) {
                if same_text(a, b) {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// u32 表是否互不相同（退出码档、错误码链点验用）。
pub fn u32s_distinct(items: &[u32]) -> bool {
    let mut i = 0usize;
    while i < items.len() {
        let mut j = i + 1;
        while j < items.len() {
            if let (Some(a), Some(b)) = (items.get(i), items.get(j)) {
                if a == b {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 语料：14 槽 = 8 活 + 6 死（一槽只坏一维，依次 Estate/Stage/Code/Value/Note/Default）
// ---------------------------------------------------------------------------
//
// 活槽申报正确（judge 恒给 `Fault::Ok`），其中既有真绿现场，也有"现场带病、
// 申报如实"的活槽——本卡判的是**申报与事实是否对得上**，带病现场如实申报同样
// 是合格的账。死槽复刻同一活现场、只翻一维：Estate/Stage/Code/Value/Default
// 五槽全部拿基准机全绿现场（`Shape::BASE`）翻，Note 槽拿"缺 usage 段"活现场翻
// ——因为全绿现场的锚位为真，翻 Note 必然连带触发"无全文却声称可锚"的第二处
// 违例，与"一槽只坏一维"的纪律冲突；缺段现场锚位本为假，单翻 Note 才干净。

/// 槽 1：裸调 `busybox`（无 `argv[1]`）——帮助面与 `--help` 同形。
pub const SHAPE_NOARGS: Shape = Shape {
    call: Call::NoArgs,
    ..Shape::BASE
};

/// 槽 2：`busybox --list` 基准现场（只出名录段，退 0，stdout）。
pub const SHAPE_LIST: Shape = Shape {
    call: Call::List,
    ..Shape::BASE_LIST
};

/// 槽 3：`busybox --list-full` 基准现场（名录带路径，退 0，stdout）。
pub const SHAPE_LIST_FULL: Shape = Shape {
    call: Call::ListFull,
    ..Shape::BASE_LIST
};

/// 槽 4：usage 段缺失的活现场——段位掩码少了 bit1，其余全绿。
pub const SHAPE_USAGE_GONE: Shape = Shape {
    sections: SEC_ALL & !SEC_USAGE,
    ..Shape::BASE
};

/// 槽 5：全文错走 stderr 的活现场——四段齐、对账平，通道闸红。
pub const SHAPE_TO_STDERR: Shape = Shape {
    chan: Chan::Stderr,
    ..Shape::BASE
};

/// 槽 6：帮助调用退了非零码的活现场——内容全绿，码闸红。
pub const SHAPE_BAD_EXIT: Shape = Shape {
    code_seen: EXIT_APPLET_FAIL,
    ..Shape::BASE
};

/// 槽 7：`--hel` 掉进 `find_applet_by_name` 的活现场——本卡整体拒收。
pub const SHAPE_LOOKALIKE: Shape = Shape::new(
    Call::Lookalike,
    0,
    0,
    ANCHOR_REGISTERED,
    ANCHOR_REGISTERED,
    Chan::Stdout,
    EXIT_APPLET,
    0,
    0,
    true,
    false,
);

/// 槽 12（Note 死槽）的载体现场：与槽 4 同一缺 usage 段现场。
pub const SHAPE_NOTE_BASE: Shape = SHAPE_USAGE_GONE;

/// 语料事实面表：前 8 槽活、后 6 槽以基准全绿现场为底、各翻申报的一维。
pub const SHAPES: [Shape; CORPUS] = [
    Shape::BASE,
    SHAPE_NOARGS,
    SHAPE_LIST,
    SHAPE_LIST_FULL,
    SHAPE_USAGE_GONE,
    SHAPE_TO_STDERR,
    SHAPE_BAD_EXIT,
    SHAPE_LOOKALIKE,
    Shape::BASE,
    Shape::BASE,
    Shape::BASE,
    Shape::BASE,
    SHAPE_NOTE_BASE,
    Shape::BASE,
];

/// 槽 0 申报：全文帮助，可当全文锚。
pub const CLAIM_HELP: Claim =
    Claim::new(true, Stage::Settle, EXIT_OK, ANCHOR_REGISTERED, Note::DoneHelp, true);
/// 槽 1 申报：裸调帮助，四段齐同样落全文锚。
pub const CLAIM_NOARGS: Claim =
    Claim::new(true, Stage::Settle, EXIT_OK, ANCHOR_REGISTERED, Note::DoneNoArgs, true);
/// 槽 2 申报：名录成篇，**不**落全文锚。
pub const CLAIM_LIST: Claim =
    Claim::new(true, Stage::Settle, EXIT_OK, ANCHOR_REGISTERED, Note::DoneList, false);
/// 槽 3 申报：全路径名录，**不**落全文锚。
pub const CLAIM_LIST_FULL: Claim =
    Claim::new(true, Stage::Settle, EXIT_OK, ANCHOR_REGISTERED, Note::DoneListFull, false);
/// 槽 4 申报：如实承认缺 usage 段（活槽：退 0 但内容有缺）。
pub const CLAIM_USAGE_GONE: Claim =
    Claim::new(true, Stage::Sections, EXIT_OK, ANCHOR_REGISTERED, Note::UsageMissing, false);
/// 槽 5 申报：如实承认全文走了 stderr（活槽：通道闸红，码仍 0）。
pub const CLAIM_TO_STDERR: Claim =
    Claim::new(true, Stage::Channel, EXIT_OK, ANCHOR_REGISTERED, Note::ToStderr, false);
/// 槽 6 申报：如实承认帮助退了 1（活槽：成败位报"败"）。
pub const CLAIM_BAD_EXIT: Claim =
    Claim::new(false, Stage::ExitCode, EXIT_APPLET_FAIL, ANCHOR_REGISTERED, Note::BadExit, false);
/// 槽 7 申报：如实承认掉进未名路径（活槽：码 127，归 F11674）。
pub const CLAIM_LOOKALIKE: Claim =
    Claim::new(false, Stage::Recognize, EXIT_APPLET, 0, Note::FellApplet, false);
/// 槽 8 死申报：把全文成功现场报成失败（Estate 维——本卡头号大忌）。
pub const CLAIM_ESTATE: Claim =
    Claim::new(false, Stage::Settle, EXIT_OK, ANCHOR_REGISTERED, Note::DoneHelp, true);
/// 槽 9 死申报：闸位谎报成结算闸（Stage 维）。
pub const CLAIM_STAGE: Claim =
    Claim::new(true, Stage::ExitCode, EXIT_OK, ANCHOR_REGISTERED, Note::DoneHelp, true);
/// 槽 10 死申报：码谎报成 GNU 用法档 2（Code 维）。
pub const CLAIM_CODE: Claim =
    Claim::new(true, Stage::Settle, EXIT_GNU_USAGE, ANCHOR_REGISTERED, Note::DoneHelp, true);
/// 槽 11 死申报：名录实列数少报一格（Value 维）。
pub const CLAIM_VALUE: Claim =
    Claim::new(true, Stage::Settle, EXIT_OK, ANCHOR_REGISTERED - 1, Note::DoneHelp, true);
/// 槽 12 死申报：缺段现场谎报成全文帮助档（Note 维；锚位仍如实为假）。
pub const CLAIM_NOTE: Claim =
    Claim::new(true, Stage::Sections, EXIT_OK, ANCHOR_REGISTERED, Note::DoneHelp, false);
/// 槽 13 死申报：全文现场谎报"不可当锚"（Default 维）。
pub const CLAIM_DEFAULT: Claim =
    Claim::new(true, Stage::Settle, EXIT_OK, ANCHOR_REGISTERED, Note::DoneHelp, false);

/// 语料申报面表。
pub const CLAIMS: [Claim; CORPUS] = [
    CLAIM_HELP,
    CLAIM_NOARGS,
    CLAIM_LIST,
    CLAIM_LIST_FULL,
    CLAIM_USAGE_GONE,
    CLAIM_TO_STDERR,
    CLAIM_BAD_EXIT,
    CLAIM_LOOKALIKE,
    CLAIM_ESTATE,
    CLAIM_STAGE,
    CLAIM_CODE,
    CLAIM_VALUE,
    CLAIM_NOTE,
    CLAIM_DEFAULT,
];

/// 期望闸位表（申报表的第二形态复写，任一处笔误都要露出来）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Settle,
    Stage::Settle,
    Stage::Settle,
    Stage::Settle,
    Stage::Sections,
    Stage::Channel,
    Stage::ExitCode,
    Stage::Recognize,
    Stage::Settle,
    Stage::ExitCode,
    Stage::Settle,
    Stage::Settle,
    Stage::Sections,
    Stage::Settle,
];

/// 期望码表（申报的退出码，第二形态）。
pub const WANT_CODE: [u32; CORPUS] = [
    EXIT_OK,
    EXIT_OK,
    EXIT_OK,
    EXIT_OK,
    EXIT_OK,
    EXIT_OK,
    EXIT_APPLET_FAIL,
    EXIT_APPLET,
    EXIT_OK,
    EXIT_OK,
    EXIT_GNU_USAGE,
    EXIT_OK,
    EXIT_OK,
    EXIT_OK,
];

/// 期望计数表（申报的名录条数，第二形态）。
pub const WANT_VALUE: [u32; CORPUS] = [
    ANCHOR_REGISTERED,
    ANCHOR_REGISTERED,
    ANCHOR_REGISTERED,
    ANCHOR_REGISTERED,
    ANCHOR_REGISTERED,
    ANCHOR_REGISTERED,
    ANCHOR_REGISTERED,
    0,
    ANCHOR_REGISTERED,
    ANCHOR_REGISTERED,
    ANCHOR_REGISTERED,
    ANCHOR_REGISTERED - 1,
    ANCHOR_REGISTERED,
    ANCHOR_REGISTERED,
];

/// 期望档位表（申报的 Note，第二形态）。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::DoneHelp,
    Note::DoneNoArgs,
    Note::DoneList,
    Note::DoneListFull,
    Note::UsageMissing,
    Note::ToStderr,
    Note::BadExit,
    Note::FellApplet,
    Note::DoneHelp,
    Note::DoneHelp,
    Note::DoneHelp,
    Note::DoneHelp,
    Note::DoneHelp,
    Note::DoneHelp,
];

/// 期望锚位表（申报的 default，第二形态）。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    true, true, false, false, false, false, false, false, true, true, true, true, false, true,
];

/// 期望判定表：`Fault::ALL` 的下标（活槽 0=Ok，死槽 1..6 依次六维）。
pub const WANT_NDX: [u32; CORPUS] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6];

/// 槽标签表：活槽记现场名，死槽一律 `*-swap` 形如契约。
pub const WANT_TAGS: [&'static str; CORPUS] = [
    "bh-help-full",
    "bh-noargs-full",
    "bh-list-only",
    "bh-list-full",
    "bh-usage-gone",
    "bh-stderr-face",
    "bh-exit-one-face",
    "bh-fell-applet",
    "estate-swap",
    "stage-swap",
    "code-swap",
    "value-swap",
    "note-swap",
    "default-swap",
];

/// 死槽标签表（第二形态：从 `WANT_TAGS` 尾部切出的六格镜像）。
pub const DEAD_SWAP_TAGS: [&'static str; DIM_COUNT] = [
    "estate-swap",
    "stage-swap",
    "code-swap",
    "value-swap",
    "note-swap",
    "default-swap",
];

/// 活槽标签表（第一形态）。
pub const LIVE_TAGS: [&'static str; ACCEPT] = [
    "bh-help-full",
    "bh-noargs-full",
    "bh-list-only",
    "bh-list-full",
    "bh-usage-gone",
    "bh-stderr-face",
    "bh-exit-one-face",
    "bh-fell-applet",
];

/// 期望的成败申报表（Estate 维的第二形态复写）。
pub const WANT_SUCCESS: [bool; CORPUS] = [
    true, true, true, true, true, true, false, false, false, true, true, true, true, true,
];

/// 账本格值表：前 7 格是基准报表统计数（总数 14、accept 8、reject 6、
/// 四档 7/2/3/2——死槽五格与全绿现场同形，故 `full` 档占七格），
/// 第 8 格是红档率 929。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 7, 2, 3, 2, RED_RATE];

/// 语料全表（14 个案例；`build_ledger` 与矩阵、报表共用）。
pub fn cases() -> [Case; CORPUS] {
    [
        Case::new(Shape::BASE, CLAIM_HELP),
        Case::new(SHAPE_NOARGS, CLAIM_NOARGS),
        Case::new(SHAPE_LIST, CLAIM_LIST),
        Case::new(SHAPE_LIST_FULL, CLAIM_LIST_FULL),
        Case::new(SHAPE_USAGE_GONE, CLAIM_USAGE_GONE),
        Case::new(SHAPE_TO_STDERR, CLAIM_TO_STDERR),
        Case::new(SHAPE_BAD_EXIT, CLAIM_BAD_EXIT),
        Case::new(SHAPE_LOOKALIKE, CLAIM_LOOKALIKE),
        Case::new(Shape::BASE, CLAIM_ESTATE),
        Case::new(Shape::BASE, CLAIM_STAGE),
        Case::new(Shape::BASE, CLAIM_CODE),
        Case::new(Shape::BASE, CLAIM_VALUE),
        Case::new(SHAPE_NOTE_BASE, CLAIM_NOTE),
        Case::new(Shape::BASE, CLAIM_DEFAULT),
    ]
}

/// 第 i 槽案例（越界给 `None`）。
pub fn case_at(i: usize) -> Option<Case> {
    match SHAPES.get(i) {
        Some(shape) => match CLAIMS.get(i) {
            Some(claim) => Some(Case::new(*shape, *claim)),
            None => None,
        },
        None => None,
    }
}

/// 第 i 槽标签（越界给 `None`）。
pub fn tag_at(i: usize) -> Option<&'static str> {
    WANT_TAGS.get(i).copied()
}

/// 申报表与期望表逐维对账（把申报表用第二形态复写一遍的机器面）。
pub fn want_tables_agree() -> bool {
    if WANT_STAGE.len() != CORPUS
        || WANT_CODE.len() != CORPUS
        || WANT_VALUE.len() != CORPUS
        || WANT_NOTE.len() != CORPUS
        || WANT_DEFAULT.len() != CORPUS
        || WANT_NDX.len() != CORPUS
        || WANT_TAGS.len() != CORPUS
        || WANT_SUCCESS.len() != CORPUS
    {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let claim = match CLAIMS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let stage = match WANT_STAGE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let code = match WANT_CODE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let value = match WANT_VALUE.get(i) {
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
        let success = match WANT_SUCCESS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let ndx = match WANT_NDX.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if claim.stage != stage
            || claim.code != code
            || claim.value != value
            || claim.note != note
            || claim.default != def
            || claim.success != success
        {
            return false;
        }
        if claim.exit_hint() == EXIT_OK {
            if !success {
                return false;
            }
        } else if success {
            return false;
        }
        if ndx >= FAULT_COUNT as u32 {
            return false;
        }
        let expect = match Fault::ALL.get(ndx as usize) {
            Some(v) => *v,
            None => return false,
        };
        if verdict_at(i) != Some(expect) {
            return false;
        }
        i += 1;
    }
    true
}

/// 活/死两半标签与总标签表同源（第三形态：切片复写互锁）。
pub fn tag_tables_agree() -> bool {
    let mut i = 0usize;
    while i < ACCEPT {
        if let (Some(a), Some(b)) = (WANT_TAGS.get(i), LIVE_TAGS.get(i)) {
            if !same_text(a, b) {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < DIM_COUNT {
        if let (Some(a), Some(b)) = (WANT_TAGS.get(ACCEPT + j), DEAD_SWAP_TAGS.get(j)) {
            if !same_text(a, b) {
                return false;
            }
        } else {
            return false;
        }
        if let Some(t) = DEAD_SWAP_TAGS.get(j) {
            if !ends_with(t, "-swap") || t.len() < 6 {
                return false;
            }
        }
        j += 1;
    }
    strs_distinct(&WANT_TAGS)
}

// ---------------------------------------------------------------------------
// 矩阵：按 `Tier::of(&actual)` 分四档，`total` 与 `good` 两维独立
// ---------------------------------------------------------------------------

/// 一档矩阵行：这一档里有多少格（total）、申报与事实相符多少格（good）。
///
/// 两维独立是刻意的：`Refused` 档可以 `good` 等于 `total`（掉未名路径的现场
/// 被如实申报成拒收），`HelpFull` 档也可能 `good` 不足（全绿现场被谎报）。
/// 把两维并成一维，就丢掉了本卡最值钱的"带病如实"与"健康被冤"的分界。
#[derive(Clone, Copy, Debug)]
pub struct Row {
    /// 本行档位。
    pub tier: Tier,
    /// 本格数。
    pub total: u32,
    /// 判定合上（`Fault::Ok`）的格数。
    pub good: u32,
}

impl Row {
    /// 空行。
    pub const fn empty(tier: Tier) -> Row {
        Row {
            tier,
            total: 0,
            good: 0,
        }
    }

    /// 入账一格。
    pub fn push(&mut self, fault: Fault) {
        self.total = self.total.saturating_add(1);
        if fault == Fault::Ok {
            self.good = self.good.saturating_add(1);
        }
    }

    /// 本行是否全对（good 与 total 齐平）。
    pub fn perfect(&self) -> bool {
        self.total != 0 && self.total == self.good
    }

    /// 本行是否空（没落过一格）。
    pub fn blank(&self) -> bool {
        self.total == 0
    }

    /// 一行渲染：`bh-full=2/2`。
    pub fn render(&self) -> String {
        let mut s = String::from(self.tier.slug());
        s.push('=');
        s.push_str(dec_text(self.good).as_str());
        s.push('/');
        s.push_str(dec_text(self.total).as_str());
        s
    }

    /// 行指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.tier.rank() as u64);
        h = fold_word(h, self.total as u64);
        fold_word(h, self.good as u64)
    }
}

/// 四档矩阵：行序恒与 `Tier::ALL` 同序（HelpFull 在上、Refused 在下）。
#[derive(Clone, Copy, Debug)]
pub struct Matrix {
    /// 四行。
    rows: [Row; TIER_COUNT],
    /// 总格数。
    total: u32,
    /// 全对的行数。
    perfect: u32,
}

impl Matrix {
    /// 由案例表建矩阵。
    pub fn of(cases: &[Case]) -> Matrix {
        let mut rows = [
            Row::empty(Tier::HelpFull),
            Row::empty(Tier::ListOnly),
            Row::empty(Tier::Patched),
            Row::empty(Tier::Refused),
        ];
        let mut total = 0u32;
        let mut i = 0usize;
        while i < cases.len() {
            let Some(c) = cases.get(i) else {
                break;
            };
            let actual = define(&c.shape);
            let tier = Tier::of(&actual);
            if let Some(slot) = rows.get_mut(tier.index()) {
                slot.push(judge(c));
            }
            total = total.saturating_add(1);
            i += 1;
        }
        let mut perfect = 0u32;
        let mut k = 0usize;
        while k < TIER_COUNT {
            if let Some(r) = rows.get(k) {
                if r.perfect() {
                    perfect = perfect.saturating_add(1);
                }
            }
            k += 1;
        }
        Matrix { rows, total, perfect }
    }

    /// 取行（越界给 `None`）。
    pub fn row(&self, tier: Tier) -> Option<Row> {
        self.rows.get(tier.index()).copied()
    }

    /// 总格数。
    pub const fn total(&self) -> u32 {
        self.total
    }

    /// 全对的行数。
    pub const fn perfect_rows(&self) -> u32 {
        self.perfect
    }

    /// 四档是否全覆盖（每档至少一格）。
    pub fn covered(&self) -> bool {
        let mut i = 0usize;
        while i < TIER_COUNT {
            match self.rows.get(i) {
                Some(r) if r.total > 0 => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 行合计是否等于总格数（矩阵自身平衡）。
    pub fn balanced(&self) -> bool {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                sum = sum.saturating_add(r.total);
            }
            i += 1;
        }
        sum == self.total
    }

    /// 某档的 total（报表对账用）。
    pub fn tier_total(&self, tier: Tier) -> u32 {
        match self.row(tier) {
            Some(r) => r.total,
            None => 0,
        }
    }

    /// good 合计（= 报表 accept 的对账伙伴）。
    pub fn good_total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                sum = sum.saturating_add(r.good);
            }
            i += 1;
        }
        sum
    }

    /// 整矩阵渲染（逐行、行序固定）。
    pub fn render(&self) -> String {
        let mut s = String::from("bh-matrix ");
        let mut i = 0usize;
        while i < TIER_COUNT {
            if i > 0 {
                sep(&mut s);
            }
            if let Some(r) = self.rows.get(i) {
                s.push_str(r.render().as_str());
            }
            i += 1;
        }
        s.push_str("; total=");
        s.push_str(dec_text(self.total).as_str());
        s.push_str("; perfect_rows=");
        s.push_str(dec_text(self.perfect).as_str());
        s
    }

    /// 指纹（四行 + 两汇总位）。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                h = fold_word(h, r.seal());
            }
            i += 1;
        }
        let h = fold_word(h, self.total as u64);
        fold_word(h, self.perfect as u64)
    }
}

// ---------------------------------------------------------------------------
// 报表：全绿 14/14 与红档 929 的结算面
// ---------------------------------------------------------------------------

/// 一次全语料判定的汇总报表。
///
/// `accept`/`reject` 数的是**申报质量**（合上/没合上的槽），四档计数数的是
/// **现场分布**（`Tier::of(&actual)`），两笔账各自成立才算平。`matched` 是
/// 与期望判定表（`WANT_NDX`）相符的格数——全绿现场它等于 `total`，注入一维
/// 谎报后它等于 13，千分率恰是标定死的 929。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 总槽数。
    pub total: u32,
    /// 申报合上的槽数（`judge == Ok`）。
    pub accept: u32,
    /// 申报没合上的槽数。
    pub reject: u32,
    /// 全文帮助面档计数。
    pub full: u32,
    /// 名录面档计数。
    pub listy: u32,
    /// 带病放行面档计数。
    pub patched: u32,
    /// 拒收面档计数。
    pub refused: u32,
    /// 与期望判定表相符的格数。
    pub matched: u32,
}

impl Report {
    /// 千分率：`(matched*1000 + total/2) / total`，除零给 0。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        let num = self.matched.saturating_mul(1000).saturating_add(self.total / 2);
        match num.checked_div(self.total) {
            Some(v) => v,
            None => 0,
        }
    }

    /// 是否全绿：尺子、两笔账、四档和、匹配数同时成立。
    pub fn green(&self) -> bool {
        self.total == CORPUS as u32
            && self.accept == ACCEPT as u32
            && self.reject == REJECT as u32
            && self
                .full
                .saturating_add(self.listy)
                .saturating_add(self.patched)
                .saturating_add(self.refused)
                == self.total
            && self.accept.saturating_add(self.reject) == self.total
            && self.matched == self.total
            && self.rate() == RATE_FULL
    }

    /// 封印：全绿给 `Ok`，任何失衡落 `BH_EBAL`。
    pub fn gate(&self) -> Result<(), u32> {
        if self.green() {
            Ok(())
        } else {
            Err(BH_EBAL)
        }
    }

    /// 逐字渲染（与 `RENDER_OK` 对账；字段序固定）。
    pub fn render(&self) -> String {
        let mut s = String::from("bh-report ");
        kv(&mut s, "total", self.total);
        sep(&mut s);
        kv(&mut s, "accept", self.accept);
        sep(&mut s);
        kv(&mut s, "reject", self.reject);
        sep(&mut s);
        kv(&mut s, "full", self.full);
        sep(&mut s);
        kv(&mut s, "listy", self.listy);
        sep(&mut s);
        kv(&mut s, "patched", self.patched);
        sep(&mut s);
        kv(&mut s, "refused", self.refused);
        sep(&mut s);
        kv(&mut s, "matched", self.matched);
        sep(&mut s);
        kv(&mut s, "rate", self.rate());
        s
    }

    /// 报表指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.total as u64);
        h = fold_word(h, self.accept as u64);
        h = fold_word(h, self.reject as u64);
        h = fold_word(h, self.full as u64);
        h = fold_word(h, self.listy as u64);
        h = fold_word(h, self.patched as u64);
        h = fold_word(h, self.refused as u64);
        fold_word(h, self.matched as u64)
    }
}

/// 全绿报表的逐字渲染文本（本卡"渲染路"的锚，改一个字节都要变红）。
const RENDER_OK: &str = "bh-report total=14 accept=8 reject=6 full=7 listy=2 patched=3 refused=2 matched=14 rate=1000";

/// 把谎报注入到申报里：在"照抄事实面"的申报上只翻指定的一维。
///
/// 翻法保证只坏那一维：Stage 翻到不同的闸、Code 翻到不同的码、Note 翻到
/// 不同的档、Default 取反、Value 加一、Estate 取反成败位。
fn inject_claim(shape: &Shape, f: Fault) -> Claim {
    let actual = define(shape);
    let base = Claim::from_outcome(&actual, actual.ok());
    match f {
        Fault::Ok => base,
        Fault::Estate => base.flip_success(!base.success),
        Fault::Stage => {
            if actual.stage == Stage::Args {
                base.flip_stage(Stage::Settle)
            } else {
                base.flip_stage(Stage::Args)
            }
        }
        Fault::Code => {
            if actual.code == EXIT_GNU_USAGE {
                base.flip_code(EXIT_OK)
            } else {
                base.flip_code(EXIT_GNU_USAGE)
            }
        }
        Fault::Value => base.flip_value(actual.value.saturating_add(1)),
        Fault::Note => {
            if actual.note == Note::SilentHelp {
                base.flip_note(Note::ToStderr)
            } else {
                base.flip_note(Note::SilentHelp)
            }
        }
        Fault::Default => base.flip_default(!actual.default),
    }
}

/// 用给定的申报表跑全语料报表（`run` 与 `run_with` 的共同内核）。
fn run_with_claims(claims: &[Claim; CORPUS]) -> Result<Report, u32> {
    if claims.len() != CORPUS {
        return Err(BH_ECOUNT);
    }
    let mut rep = Report {
        total: 0,
        accept: 0,
        reject: 0,
        full: 0,
        listy: 0,
        patched: 0,
        refused: 0,
        matched: 0,
    };
    let mut i = 0usize;
    while i < CORPUS {
        let Some(shape) = SHAPES.get(i) else {
            return Err(BH_ENOBS);
        };
        let Some(claim) = claims.get(i) else {
            return Err(BH_ENOBS);
        };
        let Some(want_ndx) = WANT_NDX.get(i) else {
            return Err(BH_ENOBS);
        };
        let want = match Fault::ALL.get(*want_ndx as usize) {
            Some(v) => *v,
            None => return Err(BH_ECOUNT),
        };
        let c = Case::new(*shape, *claim);
        let got = judge(&c);
        rep.total = rep.total.saturating_add(1);
        if got == Fault::Ok {
            rep.accept = rep.accept.saturating_add(1);
        } else {
            rep.reject = rep.reject.saturating_add(1);
        }
        if got == want {
            rep.matched = rep.matched.saturating_add(1);
        }
        match Tier::of(&define(shape)) {
            Tier::HelpFull => rep.full = rep.full.saturating_add(1),
            Tier::ListOnly => rep.listy = rep.listy.saturating_add(1),
            Tier::Patched => rep.patched = rep.patched.saturating_add(1),
            Tier::Refused => rep.refused = rep.refused.saturating_add(1),
        }
        i += 1;
    }
    if rep.total != CORPUS as u32 {
        return Err(BH_ECOUNT);
    }
    Ok(rep)
}

/// 全语料跑一遍：默认申报表（8 活 6 死）下的报表。
pub fn run() -> Result<Report, u32> {
    run_with_claims(&CLAIMS)
}

/// 注入一遍：把第 `slot` 槽的申报改坏成指定缺陷维，其余照旧。
///
/// 槽号越界返回 `BH_ESLOT`——注入面与语料面共用同一把尺子。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    let mut claims = CLAIMS;
    match over {
        None => {}
        Some((slot, f)) => {
            if slot >= CORPUS {
                return Err(BH_ESLOT);
            }
            let Some(shape) = SHAPES.get(slot) else {
                return Err(BH_ESLOT);
            };
            if let Some(dst) = claims.get_mut(slot) {
                *dst = inject_claim(shape, f);
            }
        }
    }
    run_with_claims(&claims)
}

// ---------------------------------------------------------------------------
// 阶段闸门：七道闸单调前进，倒退/跳档/封印后再动各有下场码
// ---------------------------------------------------------------------------

/// 帮助面纵线闸门。
///
/// 观测次序就是真机执行次序：`args → recognize → sections → reconcile →
/// channel → code → settle`。回退说明申报者拿文案倒推闸位；跳档说明中间闸
/// 被绕过（还没组装就谈通道）；封印后再动是账本已定案又被改写。三种违例
/// 分别落 `BH_ESTAGE`、`BH_EBAL`、`BH_ESTATE`，且全部显性计数，不许静默吞。
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    /// 上一段秩（-1 表示尚未走）。
    last: i32,
    /// 已走段数。
    steps: u32,
    /// 回退次数（显性记账）。
    falls: u32,
    /// 跳档次数（显性记账）。
    jumps: u32,
    /// 是否已封印。
    closed: bool,
}

impl Gate {
    /// 新闸门。
    pub const fn new() -> Gate {
        Gate {
            last: -1,
            steps: 0,
            falls: 0,
            jumps: 0,
            closed: false,
        }
    }

    /// 前进一闸。只接受"下一秩"：倒退（含重走同一闸）落 `BH_ESTAGE`，
    /// 跳档落 `BH_EBAL`，封印后一切落 `BH_ESTATE`。
    pub fn step(&mut self, s: Stage) -> Result<(), u32> {
        if self.closed {
            self.falls = self.falls.saturating_add(1);
            return Err(BH_ESTATE);
        }
        let r = s.rank() as i32;
        let want = match self.last.checked_add(1) {
            Some(v) => v,
            None => 0,
        };
        if r < want {
            self.falls = self.falls.saturating_add(1);
            return Err(BH_ESTAGE);
        }
        if r > want {
            self.jumps = self.jumps.saturating_add(1);
            return Err(BH_EBAL);
        }
        self.last = r;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }

    /// 沿语料纵线走满七闸并封印（正例路径）。
    pub fn walk_all(&mut self) -> Result<u32, u32> {
        let all = Stage::ALL;
        let mut i = 0usize;
        while i < all.len() {
            let Some(s) = all.get(i) else {
                return Err(BH_ECOUNT);
            };
            self.step(*s)?;
            i += 1;
        }
        self.close()
    }

    /// 封印：必须正好站在 settle（秩 7）上，否则落 `BH_ESTAGE`；
    /// 重复封印落 `BH_ESTATE`。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            self.falls = self.falls.saturating_add(1);
            return Err(BH_ESTATE);
        }
        if self.last != STAGE_LAST_RANK as i32 || self.steps != STAGE_COUNT {
            return Err(BH_ESTAGE);
        }
        self.closed = true;
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

    /// 跳档计数。
    pub const fn jumps(&self) -> u32 {
        self.jumps
    }

    /// 是否已封印。
    pub const fn closed(&self) -> bool {
        self.closed
    }

    /// 闸门指纹（封印后再改状态会换指纹，重放闸兜住它）。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, (self.last + 1) as u64);
        h = fold_word(h, self.steps as u64);
        h = fold_word(h, self.falls as u64);
        h = fold_word(h, self.jumps as u64);
        fold_flag(h, self.closed)
    }
}

// ---------------------------------------------------------------------------
// 账本：14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码
// ---------------------------------------------------------------------------

/// 账本格：一格观测的定装形态。
///
/// 槽位纪律是账本的生命线：`slot` 必须严格等于登记时的当前长度——跳号登记
/// 意味着账页被抽走一页，重复登记意味着同一页写了两次，两种都会让冻结指纹
/// 与重放指纹悄悄错位。本卡把它们一律拒在门外并计入 `rejects`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位（登记时必须严格等于当前长度）。
    pub slot: u8,
    /// 操作码（`OP_*` 之一）。
    pub op: u8,
    /// 是否统计格（true）或缺陷格（false）。
    pub ok: bool,
    /// 缺陷码（统计格恒为 0）。
    pub code: u32,
    /// 观测值（缺陷格恒为 0）。
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

/// 布尔折叠（与 `fold_flag` 同义，留本卡名以对齐契约词面）。
fn fold_bool(h: u64, v: bool) -> u64 {
    fold_flag(h, v)
}

/// 账本：满 14 格可对账、封印后拒登记、重放必须同指纹。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    /// 账页。
    rows: [Sample; SAMPLES],
    /// 已登记格数。
    len: usize,
    /// 冻结指纹。
    root: u64,
    /// 是否已冻结。
    frozen: bool,
    /// 被拒登记次数（显性记账）。
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
            return Err(BH_ESLOT);
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

    /// 统计格数。
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

    /// 缺陷格数。
    pub fn err_count(&self) -> usize {
        self.len.saturating_sub(self.ok_count())
    }

    /// 逐格对账：槽位连续、统计格值等于 `VALS`、缺陷格码等于 `REJECT_CODES`
    /// 且值归零；成败比例必须恰是 8:6。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(BH_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let Some(s) = self.rows.get(i) else {
                return Err(BH_EBAL);
            };
            if (s.slot as usize) != i {
                return Err(BH_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(BH_ECHUNK);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(BH_ELIST);
                        }
                    }
                    None => return Err(BH_EBAL),
                }
                vi = vi.saturating_add(1);
            } else {
                if s.value != 0 {
                    return Err(BH_ELIST);
                }
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(BH_ECODE);
                        }
                    }
                    None => return Err(BH_EBAL),
                }
                ci = ci.saturating_add(1);
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(BH_EBAL);
        }
        Ok(())
    }

    /// 全账指纹（含拒绝计数——拒过登记的账本与没拒过的不同指纹）。
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

    /// 冻结：先对账，再落指纹；重复冻结拒绝，未满页冻结落 `BH_EBAL`。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BH_ESTATE);
        }
        if self.len != SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BH_EBAL);
        }
        self.audit()?;
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：按 `build_ledger` 重建一份账本比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(BH_ESEAL)
        }
    }

    /// 篡改留痕：改一格观测值；冻结后篡改直接拒绝。
    pub fn tamper(&mut self, i: usize, value: u32) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BH_ESTATE);
        }
        if i >= self.len {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BH_ESLOT);
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
}

/// 计数结果 → 账本格（Ok 落统计格、Err 落缺陷格）。
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

/// 建账本：7 格统计（总数/accept/reject/四档）+ 1 格红档率（929）+ 6 格缺陷码。
///
/// 红档率格走的是注入路：把第 0 槽（全绿现场）改坏成 Code 谎报，14 格里
/// 13 格仍与期望相符，千分率 `(13*1000 + 7) / 14 = 929`——这正是 `VALS`
/// 第 8 格的标定现场。缺陷码六格依次取语料后六槽的判定码。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total)));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept)));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject)));
    let _ = l.register(count_cell(3, OP_SECTIONS, r.map(|x| x.full)));
    let _ = l.register(count_cell(4, OP_COUNT, r.map(|x| x.listy)));
    let _ = l.register(count_cell(5, OP_CHANNEL, r.map(|x| x.patched)));
    let _ = l.register(count_cell(6, OP_CODE, r.map(|x| x.refused)));

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
// 四路注入自检
// ---------------------------------------------------------------------------

/// 四路注入：表路、判路、渲染路、账本路各自独立自检。
///
/// 任何一路静默退化都会在这里变成 false：
/// 1. 表路——段基址、错误码链、枚举表、现场合法域；
/// 2. 判路——七闸纵线归因、判序六维、语料与期望表互锁；
/// 3. 渲染路——矩阵分档、槽标签互锁、红档率标定、文案逐字与 `RENDER_OK`；
/// 4. 账本路——阶段闸门、逐格对账、冻结重放。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && enums_ok() && shape_ok();
    out[1] = define_ok() && judge_ok() && corpus_ok();
    out[2] = matrix_ok() && slots_ok() && reds_ok() && message_ok() && report_ok();
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
//
// 断言方向提醒（写这些谓词最容易写反的地方）：`if cond { return false; }` 里
// 的 cond 是**违约条件**，函数最后一行才是**成立条件**。下面每一处都按
// "违例即红、全检才绿"的口径手写，绝不许出现"某维退化后仍然全绿"的旁路。
// ---------------------------------------------------------------------------

/// 01 名称与段基址：段位链、尺寸、退出码真值、交叉署名、文本基元。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d1e0 {
        return false;
    }
    if BH_ESTAGE != SEG_BASE + 0x01
        || BH_ESTATE != SEG_BASE + 0x02
        || BH_ECODE != SEG_BASE + 0x03
        || BH_ELIST != SEG_BASE + 0x04
        || BH_ENOTE != SEG_BASE + 0x05
        || BH_EANCH != SEG_BASE + 0x06
        || BH_ECHUNK != SEG_BASE + 0x07
        || BH_EDISP != SEG_BASE + 0x08
        || BH_ECOUNT != SEG_BASE + 0x09
        || BH_ESLOT != SEG_BASE + 0x0a
        || BH_ESEAL != SEG_BASE + 0x0b
        || BH_ENOBS != SEG_BASE + 0x0c
        || BH_EBAL != SEG_BASE + 0x0d
    {
        return false;
    }
    if ALL_CODES.len() != CODE_COUNT || CODE_COUNT != 13 {
        return false;
    }
    if !u32s_distinct(&ALL_CODES) {
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
        if code_offset(*c) != (i as i32) + 1 {
            return false;
        }
        let nm = code_name(*c);
        if nm.is_empty() || same_text(nm, "clean") {
            return false;
        }
        if i < DIM_COUNT {
            let Some(rc) = REJECT_CODES.get(i) else {
                return false;
            };
            if fault_of_code(*rc).map(|f| f.slug()) != Some(code_name(*rc)) {
                return false;
            }
        }
        i += 1;
    }
    if code_name(0) != "clean" || code_offset(SEG_BASE) != -1 || code_offset(SEG_BASE + 0x0e) != -1
    {
        return false;
    }
    if CORPUS != 14
        || SAMPLES != 14
        || ACCEPT != 8
        || REJECT != 6
        || ACCEPT + REJECT != CORPUS
        || TIER_COUNT != 4
        || STAGE_COUNT != 7
        || STAGE_LAST_RANK != 7
        || SECTION_COUNT != 4
        || SEC_BIT_COUNT != 4
        || CALL_COUNT != 7
        || CHAN_COUNT != 3
        || NOTE_COUNT != 16
        || DIM_COUNT != 6
        || FAULT_COUNT != 7
        || JUDGE_LEN != 7
        || STAT_CELLS != 7
    {
        return false;
    }
    if RED_RATE != 929 || RATE_FULL != 1000 || RED_RATE == RATE_FULL {
        return false;
    }
    if EXIT_OK != 0 || EXIT_APPLET != 127 || EXIT_APPLET_FAIL != 1 || EXIT_GNU_USAGE != 2 {
        return false;
    }
    if ANCHOR_NOTFOUND_EXIT != EXIT_APPLET || EXIT_RANKS != 5 || EXIT_UNJUDGED != 99 {
        return false;
    }
    if EXIT_TABLE.len() != 4 || !u32s_distinct(&EXIT_TABLE) {
        return false;
    }
    if let Some(v) = EXIT_TABLE.get(0) {
        if *v != EXIT_OK {
            return false;
        }
    } else {
        return false;
    }
    if ANCHOR_F11666_SEG != 0x6d1d0 || ANCHOR_F11666_SEG.saturating_add(ANCHOR_F11666_GAP) != SEG_BASE {
        return false;
    }
    if ANCHOR_F11664_SEG != 0x6d1a0 || ANCHOR_F11664_SEG.saturating_add(ANCHOR_F11664_GAP) != SEG_BASE
    {
        return false;
    }
    if NEXT_F11668_SEG != 0x6d1f0 || SEG_BASE.saturating_add(NEXT_F11668_GAP) != NEXT_F11668_SEG {
        return false;
    }
    if ANCHOR_F11678_SEG != 0x6d2d0
        || SEG_BASE.saturating_add(ANCHOR_F11678_GAP) != ANCHOR_F11678_SEG
    {
        return false;
    }
    if ANCHOR_F11674_SEG != 0x6d270 || ANCHOR_F11674_SEG <= SEG_BASE {
        return false;
    }
    if ANCHOR_REGISTERED != 413 || ANCHOR_HELP_BYTES != 2662 {
        return false;
    }
    if !same_text(ANCHOR_BB_VERSION, "BusyBox v1.36.1")
        || !same_text(ANCHOR_NOTFOUND_TAIL, "applet not found")
    {
        return false;
    }
    if SEC_BANNER != 0x01
        || SEC_USAGE != 0x02
        || SEC_FOOT != 0x04
        || SEC_LIST != 0x08
        || SEC_ALL != 0x0f
        || SEC_LIST_ONLY != SEC_LIST
    {
        return false;
    }
    if section_bits(SEC_ALL) as usize != SEC_BIT_COUNT || section_bits(0) != 0 {
        return false;
    }
    if !u32s_distinct(&[SEC_BANNER as u32, SEC_USAGE as u32, SEC_FOOT as u32, SEC_LIST as u32]) {
        return false;
    }
    if !same_text(FLAG_HELP, "--help")
        || !same_text(FLAG_LIST, "--list")
        || !same_text(FLAG_LIST_FULL, "--list-full")
        || !same_text(FLAG_LOOKALIKE, "--hel")
        || !same_text(FLAG_CAPS, "--HELP")
        || !same_text(FLAG_DASHDASH, "--")
    {
        return false;
    }
    if !starts_with(USAGE_HEAD, "Usage: busybox")
        || !same_text(LIST_HEADING, "Currently defined functions:")
        || !same_text(FOOT_HEAD, "Each of the above ")
        || !same_text(FOOT_TAIL, " functions is implemented as")
    {
        return false;
    }
    if !same_text(
        msg_foot_head(ANCHOR_REGISTERED).as_str(),
        "Each of the above 413 functions is implemented as",
    ) {
        return false;
    }
    if !ops_are_unique() || OP_BITS.len() != OP_COUNT_N {
        return false;
    }
    if !strs_distinct(&TIER_FIELDS) || TIER_FIELDS.len() != TIER_COUNT {
        return false;
    }
    if !reject_codes_distinct() || VALS.len() != ACCEPT {
        return false;
    }
    match VALS.get(ACCEPT - 1) {
        Some(v) => {
            if *v != RED_RATE {
                return false;
            }
        }
        None => return false,
    }
    if !strs_distinct(&REJECT_SLUGS) {
        return false;
    }
    if dec_text(0) != "0" || dec_text(929) != "929" || dec_text(u32::MAX) != "4294967295" {
        return false;
    }
    if hex_text(SEG_BASE, 5) != "6d1e0" || seg_text(SEG_BASE) != "6d1e0" {
        return false;
    }
    if i32_text(-5) != "-5" || i32_text(0) != "0" || u32_text(413) != "413" {
        return false;
    }
    if bool_text(true) != "true" || bool_text(false) != "false" {
        return false;
    }
    if fold_str(FNV_OFF, "bh-x") == fold_str(FNV_OFF, "bh-y") {
        return false;
    }
    if fold_word(FNV_OFF, 0) == FNV_OFF {
        return false;
    }
    true
}

/// 02 枚举与闸表：七形态/四段/三通道/七闸/十六档/四档的双射与纵向一致。
fn enums_ok() -> bool {
    if Call::ALL.len() != CALL_COUNT {
        return false;
    }
    let mut call_slugs: [&str; CALL_COUNT] = ["", "", "", "", "", "", ""];
    let mut i = 0usize;
    while i < CALL_COUNT {
        let Some(c) = Call::ALL.get(i) else {
            return false;
        };
        if c.index() != i {
            return false;
        }
        if Call::from_index(c.index()) != Some(*c) {
            return false;
        }
        if c.slug().is_empty() || c.name().is_empty() {
            return false;
        }
        if let Some(slot) = call_slugs.get_mut(i) {
            *slot = c.slug();
        }
        let faces = c.is_help_face() as u32
            + c.is_list_face() as u32
            + c.is_applet_path() as u32;
        if faces != 1 {
            return false;
        }
        if c.is_help_face() && c.wanted_sections() != SEC_ALL {
            return false;
        }
        if c.is_list_face() && c.wanted_sections() != SEC_LIST_ONLY {
            return false;
        }
        if c.is_applet_path() && (c.wanted_sections() != 0 || c.reconciles()) {
            return false;
        }
        if !c.is_applet_path() && !c.reconciles() {
            return false;
        }
        i += 1;
    }
    if !strs_distinct(&call_slugs) {
        return false;
    }
    if Section::ALL.len() != SECTION_COUNT {
        return false;
    }
    let mut union: u8 = 0;
    let mut k = 0usize;
    while k < SECTION_COUNT {
        let Some(sec) = Section::ALL.get(k) else {
            return false;
        };
        if sec.index() != k || Section::from_index(sec.index()) != Some(*sec) {
            return false;
        }
        if sec.bit() & sec.bit().saturating_sub(1) != 0 {
            return false;
        }
        if union & sec.bit() != 0 {
            return false;
        }
        union |= sec.bit();
        if sec.anchor_text().is_empty() || sec.slug().is_empty() {
            return false;
        }
        if sec.stage_rank() != 3 {
            return false;
        }
        k += 1;
    }
    if union != SEC_ALL {
        return false;
    }
    if Chan::ALL.len() != CHAN_COUNT {
        return false;
    }
    let mut m = 0usize;
    while m < CHAN_COUNT {
        let Some(ch) = Chan::ALL.get(m) else {
            return false;
        };
        if ch.index() != m || Chan::from_index(ch.index()) != Some(*ch) {
            return false;
        }
        if ch.is_help_channel() != (*ch == Chan::Stdout) {
            return false;
        }
        if ch.slug().is_empty() {
            return false;
        }
        m += 1;
    }
    if Stage::ALL.len() != STAGE_COUNT as usize || STAGE_RANKS.len() != STAGE_COUNT as usize {
        return false;
    }
    let mut n = 0usize;
    while n < STAGE_COUNT as usize {
        let Some(st) = Stage::ALL.get(n) else {
            return false;
        };
        if st.rank() != (n as u32) + 1 {
            return false;
        }
        if Stage::from_rank(st.rank()) != Some(*st) {
            return false;
        }
        if let Some(r) = STAGE_RANKS.get(n) {
            if st.rank() != *r {
                return false;
            }
        }
        if st.slug().is_empty() || st.name().is_empty() {
            return false;
        }
        if st.is_refuse_gate() != (*st == Stage::Recognize) {
            return false;
        }
        let grp = st.is_intake() as u32 + st.is_content() as u32 + st.settles() as u32;
        if grp != 1 {
            return false;
        }
        if let Some(nx) = st.next() {
            if nx.rank() != st.rank() + 1 || !st.before(&nx) {
                return false;
            }
        } else if *st != Stage::Settle {
            return false;
        }
        n += 1;
    }
    if Stage::from_rank(0).is_some() || Stage::from_rank(STAGE_LAST_RANK + 1).is_some() {
        return false;
    }
    if !notes_unique() || !Note::families_partition() || !notes_align_with_stages() {
        return false;
    }
    if NOTE_RANKS.len() != NOTE_COUNT {
        return false;
    }
    let mut p = 0usize;
    while p < NOTE_COUNT {
        let Some(note) = Note::ALL.get(p) else {
            return false;
        };
        if note.rank() != NOTE_RANKS[p] as u32 {
            return false;
        }
        if Note::from_rank(note.rank()) != Some(*note) {
            return false;
        }
        if note.exit_hint() >= EXIT_UNJUDGED {
            return false;
        }
        if note.anchor_candidate() != (*note == Note::DoneHelp) {
            return false;
        }
        if note.list_face_only() != (*note == Note::DoneList || *note == Note::DoneListFull) {
            return false;
        }
        if note.channel_untrusted() != note.is_chan() {
            return false;
        }
        p += 1;
    }
    if Tier::ALL.len() != TIER_COUNT || TIER_RANKS.len() != TIER_COUNT {
        return false;
    }
    let mut q = 0usize;
    while q < TIER_COUNT {
        let Some(t) = Tier::ALL.get(q) else {
            return false;
        };
        if t.rank() != q as u32 || Tier::from_rank(t.rank()) != Some(*t) {
            return false;
        }
        if t.anchors() != (*t == Tier::HelpFull) {
            return false;
        }
        if t.is_greenish() != (*t == Tier::Patched) {
            return false;
        }
        if t.slug().is_empty() || t.name().is_empty() {
            return false;
        }
        q += 1;
    }
    if !tiers_are_exclusive() {
        return false;
    }
    if !judge_order_is_strict() || !judge_tables_agree() {
        return false;
    }
    if Fault::ALL.len() != FAULT_COUNT || FAULT_RANKS.len() != FAULT_COUNT {
        return false;
    }
    let mut fslugs: [&str; FAULT_COUNT] = ["", "", "", "", "", "", ""];
    let mut fdims: [&str; FAULT_COUNT] = ["", "", "", "", "", "", ""];
    let mut r = 0usize;
    while r < FAULT_COUNT {
        let Some(f) = Fault::ALL.get(r) else {
            return false;
        };
        if f.rank() != r as u32 || f.index() != r {
            return false;
        }
        if let Some(slot) = fslugs.get_mut(r) {
            *slot = f.slug();
        }
        if let Some(slot) = fdims.get_mut(r) {
            *slot = f.dim();
        }
        if f.slug().is_empty() || f.dim().is_empty() || f.blurb().is_empty() || f.name().is_empty()
        {
            return false;
        }
        if f.is_fault() != (f.code() != 0) {
            return false;
        }
        r += 1;
    }
    if !strs_distinct(&fslugs) || !strs_distinct(&fdims) {
        return false;
    }
    if Fault::Ok.code() != 0 || Fault::Ok.is_fault() {
        return false;
    }
    true
}

/// 03 现场合法域：基准现场自洽、派生现场只动一维、折叠与渲染不塌。
fn shape_ok() -> bool {
    let base = Shape::BASE;
    if base.contradictory()
        || !base.reconciles()
        || !base.has_wanted()
        || !base.channel_pure()
        || base.truncated()
        || base.silent()
        || base.first_missing().is_some()
        || base.want_sections() != SEC_ALL
        || !base.is_help_face()
        || base.applet_path()
        || base.foot_disagrees()
        || base.overflow()
    {
        return false;
    }
    if !SHAPE_NOARGS.is_help_face() || SHAPE_NOARGS.call != Call::NoArgs {
        return false;
    }
    if !SHAPE_LIST.is_list_face() || !SHAPE_LIST.has_wanted() || SHAPE_LIST.call != Call::List {
        return false;
    }
    if SHAPE_LIST_FULL.call != Call::ListFull || !SHAPE_LIST_FULL.is_list_face() {
        return false;
    }
    if SHAPE_USAGE_GONE.has_wanted() || SHAPE_USAGE_GONE.first_missing() != Some(Section::Usage) {
        return false;
    }
    if SHAPE_TO_STDERR.channel_pure() || SHAPE_TO_STDERR.chan != Chan::Stderr {
        return false;
    }
    if SHAPE_BAD_EXIT.code_seen != EXIT_APPLET_FAIL || SHAPE_BAD_EXIT.chan != Chan::Stdout {
        return false;
    }
    if !SHAPE_LOOKALIKE.applet_path() || SHAPE_LOOKALIKE.is_help_face() {
        return false;
    }
    if !sections_in_bounds(SHAPE_USAGE_GONE.sections) || sections_in_bounds(0x10) {
        return false;
    }
    let mut i = 0usize;
    while i < SHAPES.len() {
        let Some(s) = SHAPES.get(i) else {
            return false;
        };
        if !sections_in_bounds(s.sections) {
            return false;
        }
        if s.listed > LISTED_MAX || s.bytes_out > BYTES_MAX || s.need_bytes > BYTES_MAX {
            return false;
        }
        if s.render().is_empty() {
            return false;
        }
        i += 1;
    }
    let over = Shape {
        listed: ANCHOR_REGISTERED + 1,
        ..Shape::BASE
    };
    if !over.overflow() || !over.contradictory() {
        return false;
    }
    let cut = Shape {
        bytes_out: 10,
        ..Shape::BASE
    };
    if !cut.truncated() {
        return false;
    }
    let mute = Shape {
        bytes_out: 0,
        ..Shape::BASE
    };
    if !mute.silent() || !cut.truncated() {
        return false;
    }
    if base.fold(FNV_OFF) == over.fold(FNV_OFF) || base.fold(FNV_OFF) == cut.fold(FNV_OFF) {
        return false;
    }
    if base.fold(FNV_OFF) != Shape::BASE.fold(FNV_OFF) {
        return false;
    }
    if base.fold(FNV_OFF) == SHAPE_NOARGS.fold(FNV_OFF) {
        return false;
    }
    if !has_sub(
        SHAPE_LOOKALIKE.stdout_text().as_str(),
        ANCHOR_NOTFOUND_TAIL,
    ) {
        return false;
    }
    if !has_sub(base.render().as_str(), "shape --help") {
        return false;
    }
    if !has_sub(SHAPE_USAGE_GONE.render().as_str(), "sec=13") {
        return false;
    }
    true
}

/// 04 七闸纵线：每一类现场必须定性与它所属的闸，一档都不许串闸。
fn define_ok() -> bool {
    let o = define(&Shape::BASE);
    if o.stage != Stage::Settle
        || o.code != EXIT_OK
        || o.value != ANCHOR_REGISTERED
        || o.note != Note::DoneHelp
        || !o.default
        || !o.ok()
        || o.refused()
        || !o.can_anchor()
        || o.exit_rank() != Some(0)
    {
        return false;
    }
    let cases: [(Shape, Stage, Note, bool, u32); 12] = [
        (
            SHAPE_NOARGS,
            Stage::Settle,
            Note::DoneNoArgs,
            true,
            EXIT_OK,
        ),
        (SHAPE_LIST, Stage::Settle, Note::DoneList, false, EXIT_OK),
        (
            SHAPE_LIST_FULL,
            Stage::Settle,
            Note::DoneListFull,
            false,
            EXIT_OK,
        ),
        (
            SHAPE_USAGE_GONE,
            Stage::Sections,
            Note::UsageMissing,
            false,
            EXIT_OK,
        ),
        (
            Shape {
                sections: SEC_ALL & !SEC_BANNER,
                ..Shape::BASE
            },
            Stage::Sections,
            Note::BannerMissing,
            false,
            EXIT_OK,
        ),
        (
            Shape {
                sections: SEC_ALL & !SEC_FOOT,
                ..Shape::BASE
            },
            Stage::Sections,
            Note::FootMissing,
            false,
            EXIT_OK,
        ),
        (
            Shape {
                sections: SEC_ALL & !SEC_LIST,
                ..Shape::BASE
            },
            Stage::Sections,
            Note::ListMissing,
            false,
            EXIT_OK,
        ),
        (SHAPE_TO_STDERR, Stage::Channel, Note::ToStderr, false, EXIT_OK),
        (SHAPE_BAD_EXIT, Stage::ExitCode, Note::BadExit, false, EXIT_APPLET_FAIL),
        (
            SHAPE_LOOKALIKE,
            Stage::Recognize,
            Note::FellApplet,
            false,
            EXIT_APPLET,
        ),
        (
            Shape {
                call: Call::CapsHelp,
                ..SHAPE_LOOKALIKE
            },
            Stage::Recognize,
            Note::FellApplet,
            false,
            EXIT_APPLET,
        ),
        (
            Shape {
                call: Call::DashDash,
                ..SHAPE_LOOKALIKE
            },
            Stage::Recognize,
            Note::FellApplet,
            false,
            EXIT_APPLET,
        ),
    ];
    let mut i = 0usize;
    while i < cases.len() {
        let Some((sh, st, nt, def, code)) = cases.get(i) else {
            return false;
        };
        let got = define(sh);
        if got.stage != *st || got.note != *nt || got.default != *def || got.code != *code {
            return false;
        }
        if got.stage != nt.raised_by() {
            return false;
        }
        i += 1;
    }
    let over = define(&Shape {
        listed: ANCHOR_REGISTERED + 1,
        ..Shape::BASE
    });
    if over.stage != Stage::Reconcile || over.note != Note::ListOverflow || over.ok() != true {
        return false;
    }
    let mismatch = define(&Shape {
        listed: ANCHOR_REGISTERED - 1,
        ..Shape::BASE
    });
    if mismatch.stage != Stage::Reconcile || mismatch.note != Note::CountMismatch {
        return false;
    }
    let cut = define(&Shape {
        bytes_out: 100,
        ..Shape::BASE
    });
    if cut.stage != Stage::Reconcile || cut.note != Note::Truncated || cut.value != 100 {
        return false;
    }
    let split = define(&Shape {
        split_out: true,
        ..Shape::BASE
    });
    if split.stage != Stage::Channel || split.note != Note::SplitWrite {
        return false;
    }
    let mute = define(&Shape {
        chan: Chan::Nowhere,
        bytes_out: 0,
        need_bytes: 0,
        ..Shape::BASE
    });
    if mute.stage != Stage::Channel || mute.note != Note::SilentHelp || !mute.is_silent() {
        return false;
    }
    let list_gone = define(&Shape {
        sections: 0,
        ..SHAPE_LIST
    });
    if list_gone.stage != Stage::Sections || list_gone.note != Note::ListMissing {
        return false;
    }
    let list_over = define(&Shape {
        listed: ANCHOR_REGISTERED + 1,
        ..SHAPE_LIST
    });
    if list_over.stage != Stage::Reconcile || list_over.note != Note::ListOverflow {
        return false;
    }
    let list_bad = define(&Shape {
        code_seen: EXIT_APPLET_FAIL,
        ..SHAPE_LIST
    });
    if list_bad.stage != Stage::ExitCode || list_bad.note != Note::BadExit {
        return false;
    }
    if Tier::of(&define(&Shape::BASE)) != Tier::HelpFull {
        return false;
    }
    if Tier::of(&define(&SHAPE_LOOKALIKE)) != Tier::Refused {
        return false;
    }
    if Tier::of(&define(&SHAPE_BAD_EXIT)) != Tier::Refused {
        return false;
    }
    if Tier::of(&define(&SHAPE_USAGE_GONE)) != Tier::Patched {
        return false;
    }
    if Tier::of(&define(&SHAPE_LIST)) != Tier::ListOnly {
        return false;
    }
    let mut j = 0usize;
    while j < ACCEPT {
        let Some(shape) = SHAPES.get(j) else {
            return false;
        };
        let Some(claim) = CLAIMS.get(j) else {
            return false;
        };
        let actual = define(shape);
        if claim.stage != actual.stage
            || claim.code != actual.code
            || claim.value != actual.value
            || claim.note != actual.note
            || claim.default != actual.default
            || claim.success != actual.ok()
        {
            return false;
        }
        j += 1;
    }
    if !mute.prints() || define(&Shape::BASE).prints() {
        return false;
    }
    true
}

/// 05 判序六维：Estate 压过一切，其余按固定次序依次顶位；六死槽各归其维。
fn judge_ok() -> bool {
    let base = define(&Shape::BASE);
    let everything = Claim::new(
        false,
        Stage::Args,
        EXIT_GNU_USAGE,
        0,
        Note::Truncated,
        false,
    );
    if judge(&Case::new(Shape::BASE, everything)) != Fault::Estate {
        return false;
    }
    let stage_and_code = Claim::new(true, Stage::Args, EXIT_GNU_USAGE, base.value, base.note, base.default);
    if judge(&Case::new(Shape::BASE, stage_and_code)) != Fault::Stage {
        return false;
    }
    let code_and_value = Claim::new(true, base.stage, EXIT_GNU_USAGE, 7, base.note, base.default);
    if judge(&Case::new(Shape::BASE, code_and_value)) != Fault::Code {
        return false;
    }
    let value_and_note = Claim::new(true, base.stage, base.code, 7, Note::Truncated, base.default);
    if judge(&Case::new(Shape::BASE, value_and_note)) != Fault::Value {
        return false;
    }
    let note_and_default = Claim::new(true, base.stage, base.code, base.value, Note::Truncated, false);
    if judge(&Case::new(Shape::BASE, note_and_default)) != Fault::Note {
        return false;
    }
    if judge(&Case::new(Shape::BASE, base_flip_default(&base))) != Fault::Default {
        return false;
    }
    if judge(&Case::new(Shape::BASE, Claim::from_outcome(&base, base.ok()))) != Fault::Ok {
        return false;
    }
    let mut i = 0usize;
    while i < DIM_COUNT {
        let Some(f) = REJECT_FAULTS.get(i) else {
            return false;
        };
        let Some(slot) = verdict_at(ACCEPT + i) else {
            return false;
        };
        if slot != *f {
            return false;
        }
        if slot.code() == 0 || fault_of_code(slot.code()) != Some(*f) {
            return false;
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < CORPUS {
        let Some(c) = case_at(j) else {
            return false;
        };
        let healed = Claim::from_outcome(&c.actual(), c.actual().ok());
        if judge(&Case::new(c.shape, healed)) != Fault::Ok {
            return false;
        }
        j += 1;
    }
    if !judge_maps_faults() || !fault_tables_agree() || !reject_codes_distinct() {
        return false;
    }
    if fault_of_dim("estate") != Some(Fault::Estate) || fault_of_dim("ok") != Some(Fault::Ok) {
        return false;
    }
    if fault_of_dim("nonsense").is_some() {
        return false;
    }
    if fault_of_code(0) != Some(Fault::Ok) {
        return false;
    }
    if fault_of_code(0xdead).is_some() {
        return false;
    }
    let mut k = 0usize;
    while k < DIM_COUNT {
        if let Some(f) = REJECT_FAULTS.get(k) {
            if fault_of_dim(f.dim()) != Some(*f) {
                return false;
            }
        } else {
            return false;
        }
        k += 1;
    }
    true
}

/// 辅助：只翻锚位维的申报（judge_ok 的 Default 维正例）。
fn base_flip_default(actual: &Outcome) -> Claim {
    Claim::from_outcome(actual, actual.ok()).flip_default(!actual.default)
}

/// 06 语料与期望表：8 活 6 死、期望表复写互锁、死槽标签成 `*-swap` 形。
fn corpus_ok() -> bool {
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS || cases().len() != CORPUS {
        return false;
    }
    if !want_tables_agree() {
        return false;
    }
    if !tag_tables_agree() {
        return false;
    }
    let mut i = 0usize;
    while i < ACCEPT {
        if verdict_at(i) != Some(Fault::Ok) {
            return false;
        }
        if WANT_NDX.get(i) != Some(&0) {
            return false;
        }
        if LIVE_TAGS.get(i).copied() != WANT_TAGS.get(i).copied() {
            return false;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < DIM_COUNT {
        let slot = ACCEPT + k;
        let Some(want) = REJECT_FAULTS.get(k) else {
            return false;
        };
        if verdict_at(slot) != Some(*want) {
            return false;
        }
        if WANT_NDX.get(slot) != Some(&(k as u32 + 1)) {
            return false;
        }
        if DEAD_SWAP_TAGS.get(k).copied() != WANT_TAGS.get(slot).copied() {
            return false;
        }
        if !fault_is_localized(slot) {
            return false;
        }
        k += 1;
    }
    let all_live_healthy = [Shape::BASE, SHAPE_NOARGS, SHAPE_LIST, SHAPE_LIST_FULL];
    let mut found = 0usize;
    for item in all_live_healthy.iter() {
        let mut hit = false;
        let mut s = 0usize;
        while s < ACCEPT {
            if let Some(sh) = SHAPES.get(s) {
                if sh == item {
                    hit = true;
                }
            }
            s += 1;
        }
        if !hit {
            return false;
        }
        found = found.saturating_add(1);
    }
    if found != all_live_healthy.len() {
        return false;
    }
    if SHAPES.get(12) != SHAPES.get(4) {
        return false;
    }
    let live_notes: [Note; ACCEPT] = [
        Note::DoneHelp,
        Note::DoneNoArgs,
        Note::DoneList,
        Note::DoneListFull,
        Note::UsageMissing,
        Note::ToStderr,
        Note::BadExit,
        Note::FellApplet,
    ];
    let mut n = 0usize;
    while n < ACCEPT {
        let Some(want_note) = live_notes.get(n) else {
            return false;
        };
        let Some(actual) = outcome_at(n) else {
            return false;
        };
        if actual.note != *want_note {
            return false;
        }
        n += 1;
    }
    if !strs_distinct(&LIVE_TAGS) {
        return false;
    }
    let mut slugs: [&'static str; ACCEPT] = ["", "", "", "", "", "", "", ""];
    let mut z = 0usize;
    while z < ACCEPT {
        if let Some(o) = outcome_at(z) {
            if let Some(slot) = slugs.get_mut(z) {
                *slot = o.note.slug();
            }
        }
        z += 1;
    }
    if !strs_distinct(&slugs) {
        return false;
    }
    true
}

/// 07 矩阵分档：四档行合计与报表对账，`total`/`good` 两维独立。
fn matrix_ok() -> bool {
    let m = Matrix::of(&cases());
    if m.total() != CORPUS as u32 {
        return false;
    }
    if !m.balanced() || !m.covered() {
        return false;
    }
    if m.tier_total(Tier::HelpFull) != 7
        || m.tier_total(Tier::ListOnly) != 2
        || m.tier_total(Tier::Patched) != 3
        || m.tier_total(Tier::Refused) != 2
    {
        return false;
    }
    if m.good_total() != ACCEPT as u32 {
        return false;
    }
    let rep = match run() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if m.tier_total(Tier::HelpFull) != rep.full
        || m.tier_total(Tier::ListOnly) != rep.listy
        || m.tier_total(Tier::Patched) != rep.patched
        || m.tier_total(Tier::Refused) != rep.refused
        || m.good_total() != rep.accept
    {
        return false;
    }
    if m.perfect_rows() != 2 {
        return false;
    }
    let full_row = match m.row(Tier::HelpFull) {
        Some(r) => r,
        None => return false,
    };
    if full_row.total == full_row.good {
        return false;
    }
    if full_row.good != 2 || !full_row.tier.anchors() {
        return false;
    }
    let refused_row = match m.row(Tier::Refused) {
        Some(r) => r,
        None => return false,
    };
    if !refused_row.perfect() || refused_row.good != 2 {
        return false;
    }
    let rendered = m.render();
    if !has_sub(rendered.as_str(), "bh-full=2/7")
        || !has_sub(rendered.as_str(), "bh-refused=2/2")
        || !has_sub(rendered.as_str(), "total=14")
        || !has_sub(rendered.as_str(), "perfect_rows=2")
    {
        return false;
    }
    let empty = Matrix::of(&[]);
    if empty.total() != 0 || empty.covered() || empty.balanced() != true || empty.perfect_rows() != 0
    {
        return false;
    }
    if empty.render().is_empty() {
        return false;
    }
    let single_ok = [case_at(0).unwrap_or(Case::new(Shape::BASE, CLAIM_HELP))];
    let single_bad = [case_at(8).unwrap_or(Case::new(Shape::BASE, CLAIM_ESTATE))];
    let mo = Matrix::of(&single_ok);
    let mb = Matrix::of(&single_bad);
    if mo.perfect_rows() != 1 || mb.perfect_rows() != 0 {
        return false;
    }
    if mo.seal() == mb.seal() {
        return false;
    }
    if m.seal() == 0 || m.seal() != Matrix::of(&cases()).seal() {
        return false;
    }
    let mut row = Row::empty(Tier::Patched);
    if !row.blank() || row.perfect() {
        return false;
    }
    row.push(Fault::Ok);
    if row.blank() || !row.perfect() || row.total != 1 || row.good != 1 {
        return false;
    }
    row.push(Fault::Note);
    if row.perfect() || row.total != 2 || row.good != 1 {
        return false;
    }
    if row.render().is_empty() || row.seal() == 0 {
        return false;
    }
    true
}

/// 08 槽位与期望：越界槽一律 `BH_ESLOT`，注入矩阵每槽每维都顶得住。
fn slots_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        if case_at(i).is_none() || verdict_at(i).is_none() || outcome_at(i).is_none() {
            return false;
        }
        if tag_at(i).is_none() {
            return false;
        }
        i += 1;
    }
    if case_at(CORPUS).is_some() || verdict_at(CORPUS).is_some() || outcome_at(CORPUS).is_some() {
        return false;
    }
    if case_at(usize::MAX).is_some() || tag_at(usize::MAX).is_some() {
        return false;
    }
    if run_with(None).is_err() {
        return false;
    }
    match run_with(Some((CORPUS, Fault::Code))) {
        Err(BH_ESLOT) => {}
        _ => return false,
    }
    match run_with(Some((usize::MAX, Fault::Estate))) {
        Err(BH_ESLOT) => {}
        _ => return false,
    }
    let green = match run() {
        Ok(r) => r,
        Err(_) => return false,
    };
    let via_none = match run_with(None) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if green.seal() != via_none.seal() {
        return false;
    }
    let faults = Fault::ALL;
    let mut slot = 0usize;
    while slot < CORPUS {
        let Some(shape) = SHAPES.get(slot) else {
            return false;
        };
        let mut f = 0usize;
        while f < faults.len() {
            let Some(kind) = faults.get(f) else {
                return false;
            };
            let injected = inject_claim(shape, *kind);
            if judge(&Case::new(*shape, injected)) != *kind {
                return false;
            }
            if *kind != Fault::Ok {
                match run_with(Some((slot, *kind))) {
                    Ok(r) => {
                        if r.total != CORPUS as u32 || r.matched != (CORPUS as u32) - 1 {
                            return false;
                        }
                    }
                    Err(_) => return false,
                }
            }
            f += 1;
        }
        slot += 1;
    }
    let mut d = 0usize;
    while d < DIM_COUNT {
        if WANT_NDX.get(ACCEPT + d) != Some(&(d as u32 + 1)) {
            return false;
        }
        let Some(shape) = SHAPES.get(ACCEPT + d) else {
            return false;
        };
        let Some(claim) = CLAIMS.get(ACCEPT + d) else {
            return false;
        };
        let actual = define(shape);
        let healed = Claim::from_outcome(&actual, actual.ok());
        if healed.stage == claim.stage
            && healed.code == claim.code
            && healed.value == claim.value
            && healed.note == claim.note
            && healed.default == claim.default
            && healed.success == claim.success
        {
            return false;
        }
        let wrong_dims = (healed.stage != claim.stage) as u32
            + (healed.code != claim.code) as u32
            + (healed.value != claim.value) as u32
            + (healed.note != claim.note) as u32
            + (healed.default != claim.default) as u32
            + (healed.success != claim.success) as u32;
        if wrong_dims != 1 {
            return false;
        }
        d += 1;
    }
    true
}

/// 09 红档率标定：`VALS` 第 8 格 929，注入一路谎报恰好 13/14 中。
fn reds_ok() -> bool {
    if VALS.len() != ACCEPT {
        return false;
    }
    let rep = match run() {
        Ok(r) => r,
        Err(_) => return false,
    };
    let fields = [rep.total, rep.accept, rep.reject, rep.full, rep.listy, rep.patched, rep.refused];
    if fields.len() != STAT_CELLS {
        return false;
    }
    let mut i = 0usize;
    while i < STAT_CELLS {
        let Some(a) = fields.get(i) else {
            return false;
        };
        let Some(b) = VALS.get(i) else {
            return false;
        };
        if a != b {
            return false;
        }
        i += 1;
    }
    match VALS.get(ACCEPT - 1) {
        Some(v) => {
            if *v != RED_RATE || *v != 929 {
                return false;
            }
        }
        None => return false,
    }
    if rep.rate() != RATE_FULL {
        return false;
    }
    let red = match run_with(Some((0, Fault::Code))) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if red.rate() != RED_RATE {
        return false;
    }
    if red.total != 14 || red.matched != 13 || red.accept != 7 || red.reject != 7 {
        return false;
    }
    if red.full != rep.full || red.listy != rep.listy {
        return false;
    }
    let manual = Report {
        total: 14,
        accept: 7,
        reject: 7,
        full: 7,
        listy: 2,
        patched: 3,
        refused: 2,
        matched: 13,
    };
    if manual.rate() != 929 {
        return false;
    }
    let other = Report {
        total: 8,
        accept: 7,
        reject: 1,
        full: 4,
        listy: 1,
        patched: 2,
        refused: 1,
        matched: 7,
    };
    if other.rate() != 875 {
        return false;
    }
    let zero = Report {
        total: 0,
        accept: 0,
        reject: 0,
        full: 0,
        listy: 0,
        patched: 0,
        refused: 0,
        matched: 0,
    };
    if zero.rate() != 0 || zero.green() {
        return false;
    }
    if red.green() || rep.green() != true {
        return false;
    }
    let ledger = build_ledger();
    match ledger.sample(7) {
        Some(s) => {
            if !s.ok || s.value != 929 || s.code != 0 || s.op != OP_RATE {
                return false;
            }
        }
        None => return false,
    }
    true
}

/// 10 文案逐字：banner/usage/脚注/名录四段与取证行按基准机原文点验。
fn message_ok() -> bool {
    if !same_text(
        msg_copyright_head().as_str(),
        "BusyBox is copyrighted by many authors between 1998-2015.",
    ) {
        return false;
    }
    if !same_text(
        msg_copyright_license().as_str(),
        "Licensed under GPLv2. See source files for details.",
    ) {
        return false;
    }
    if !same_text(
        msg_version_line().as_str(),
        "BusyBox v1.36.1 (2023-11-08 23:30:50 UTC) multi-call binary.",
    ) {
        return false;
    }
    let banner = msg_banner();
    if !has_sub(banner.as_str(), "multi-call binary.")
        || !starts_with(banner.as_str(), "BusyBox is copyrighted")
        || newline_count(banner.as_str()) < 3
    {
        return false;
    }
    let usage = msg_usage_block();
    if !has_sub(usage.as_str(), USAGE_HEAD)
        || !has_sub(usage.as_str(), USAGE_OR_LIST)
        || !has_sub(usage.as_str(), USAGE_OR_INSTALL)
    {
        return false;
    }
    if !starts_with(USAGE_OR_LIST, "   or:") || !starts_with(USAGE_OR_INSTALL, "   or:") {
        return false;
    }
    let foot = msg_footnote(ANCHOR_REGISTERED);
    if !has_sub(foot.as_str(), "mutually-linked symbolic link")
        || !has_sub(foot.as_str(), "again if you wish to change the name of a function.")
        || newline_count(foot.as_str()) != 4
    {
        return false;
    }
    let full = assemble_help(SEC_ALL, ANCHOR_REGISTERED);
    let mut i = 0usize;
    while i < SECTION_COUNT {
        let Some(sec) = Section::ALL.get(i) else {
            return false;
        };
        if !has_sub(full.as_str(), sec.anchor_text()) {
            return false;
        }
        i += 1;
    }
    if !help_matches_snapshot(SEC_ALL, ANCHOR_REGISTERED, full.as_str()) {
        return false;
    }
    if help_matches_snapshot(SEC_ALL & !SEC_USAGE, ANCHOR_REGISTERED, full.as_str()) {
        return false;
    }
    if !same_text(
        msg_list_heading().as_str(),
        LIST_HEADING,
    ) {
        return false;
    }
    if !same_text(msg_list_group("a", "x").as_str(), "\ta: x") {
        return false;
    }
    if !has_sub(msg_list_group_a().as_str(), "ac, adjtimex, arping") {
        return false;
    }
    if !same_text(
        msg_applet_not_found("busybox --hel").as_str(),
        "busybox --hel: applet not found",
    ) {
        return false;
    }
    if !same_text(
        msg_missing_section(&Section::Usage).as_str(),
        "busybox --help: missing usage section of usage text",
    ) {
        return false;
    }
    let dis = msg_count_disagree(413, 412, 413);
    if !has_sub(dis.as_str(), "footnote says 413")
        || !has_sub(dis.as_str(), "list holds 412")
        || !has_sub(dis.as_str(), "build registers 413")
        || !has_sub(dis.as_str(), "three-way reconciliation failed")
    {
        return false;
    }
    let tr = msg_truncated(100, 2662);
    if !has_sub(tr.as_str(), "truncated at 100 of 2662 bytes") {
        return false;
    }
    let wc = msg_wrong_channel(&Chan::Stderr);
    if !has_sub(wc.as_str(), "usage text went to stderr") {
        return false;
    }
    if !has_sub(msg_silent_help().as_str(), "silent success is a lie") {
        return false;
    }
    if !has_sub(msg_bad_exit(1).as_str(), "exit code 1")
        || !has_sub(msg_bad_exit(1).as_str(), "help face knows only 0")
    {
        return false;
    }
    if !has_sub(
        msg_dispatch_fall(FLAG_LOOKALIKE).as_str(),
        "refused by F11667, owned by F11674",
    ) {
        return false;
    }
    if !has_sub(
        msg_help_anchor(413, 2662).as_str(),
        "413 names, 2662 bytes, stdout, exit 0",
    ) {
        return false;
    }
    let green = define(&Shape::BASE);
    if !message(&Shape::BASE, &green).is_empty() {
        return false;
    }
    let fell = define(&SHAPE_LOOKALIKE);
    if !has_sub(
        message(&SHAPE_LOOKALIKE, &fell).as_str(),
        ANCHOR_NOTFOUND_TAIL,
    ) {
        return false;
    }
    let gone = define(&SHAPE_USAGE_GONE);
    if !has_sub(
        message(&SHAPE_USAGE_GONE, &gone).as_str(),
        "missing usage section",
    ) {
        return false;
    }
    if !stdout_message(&Shape::BASE, &green).is_empty() == false {
        return false;
    }
    let err_shape = SHAPE_TO_STDERR;
    let err_actual = define(&err_shape);
    if !stdout_message(&err_shape, &err_actual).is_empty() {
        return false;
    }
    let mute_shape = Shape {
        chan: Chan::Nowhere,
        bytes_out: 0,
        need_bytes: 0,
        ..Shape::BASE
    };
    if !stdout_message(&mute_shape, &define(&mute_shape)).is_empty() {
        return false;
    }
    if !stdout_message(&SHAPE_LOOKALIKE, &fell).is_empty() {
        return false;
    }
    let t = transcript(&Shape::BASE, &green);
    if !starts_with(t.as_str(), "OUT<") || !has_sub(t.as_str(), "|EXIT=0|") == false {
        return false;
    }
    let ev = evidence(&Shape::BASE, &green);
    if !has_sub(ev.as_str(), "stage=settle") || !has_sub(ev.as_str(), "|") {
        return false;
    }
    if let Ok(r) = run() {
        if r.render() != RENDER_OK {
            return false;
        }
    } else {
        return false;
    }
    if !has_sub(RENDER_OK, "bh-report total=14") || !has_sub(RENDER_OK, "rate=1000") {
        return false;
    }
    true
}

/// 11 报表与封印：全绿账走 `Ok`，注入红账封不住门且如实报 `BH_EBAL`。
fn report_ok() -> bool {
    let rep = match run() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if !rep.green() {
        return false;
    }
    if rep.gate() != Ok(()) {
        return false;
    }
    if rep.total != CORPUS as u32
        || rep.accept != ACCEPT as u32
        || rep.reject != REJECT as u32
        || rep.matched != rep.total
    {
        return false;
    }
    if rep.full
        .saturating_add(rep.listy)
        .saturating_add(rep.patched)
        .saturating_add(rep.refused)
        != rep.total
    {
        return false;
    }
    if rep.render() != RENDER_OK {
        return false;
    }
    let red = match run_with(Some((0, Fault::Estate))) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if red.green() {
        return false;
    }
    if red.gate() != Err(BH_EBAL) {
        return false;
    }
    if red.rate() != RED_RATE || red.matched != 13 || red.accept != 7 || red.reject != 7 {
        return false;
    }
    if red.render() == RENDER_OK {
        return false;
    }
    if !has_sub(red.render().as_str(), "accept=7") || !has_sub(red.render().as_str(), "rate=929") {
        return false;
    }
    if rep.seal() == red.seal() {
        return false;
    }
    if run_with(Some((CORPUS + 3, Fault::Note))) != Err(BH_ESLOT) {
        return false;
    }
    let skewed = Report {
        total: 14,
        accept: 9,
        reject: 5,
        full: 7,
        listy: 2,
        patched: 3,
        refused: 2,
        matched: 14,
    };
    if skewed.green() {
        return false;
    }
    let short = Report {
        total: 13,
        accept: 8,
        reject: 5,
        full: 7,
        listy: 2,
        patched: 2,
        refused: 2,
        matched: 13,
    };
    if short.green() {
        return false;
    }
    if skewed.gate() != Err(BH_EBAL) || short.gate() != Err(BH_EBAL) {
        return false;
    }
    true
}

/// 12 阶段闸门：只许前进一秩；倒退、跳档、封印后再动分别落三个下场码。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    if g.last_rank() != -1 || g.steps() != 0 || g.falls() != 0 || g.jumps() != 0 || g.closed() {
        return false;
    }
    if g.step(Stage::Args) != Ok(()) {
        return false;
    }
    if g.step(Stage::Args) != Err(BH_ESTAGE) {
        return false;
    }
    if g.falls() != 1 {
        return false;
    }
    if g.step(Stage::Settle) != Err(BH_EBAL) {
        return false;
    }
    if g.jumps() != 1 || g.steps() != 1 {
        return false;
    }
    if g.close() != Err(BH_ESTAGE) {
        return false;
    }
    let mut h = Gate::new();
    if h.walk_all() != Ok(STAGE_COUNT) {
        return false;
    }
    if !h.closed() || h.steps() != STAGE_COUNT || h.last_rank() != STAGE_LAST_RANK as i32 {
        return false;
    }
    if h.step(Stage::Settle) != Err(BH_ESTATE) {
        return false;
    }
    if h.close() != Err(BH_ESTATE) {
        return false;
    }
    if h.falls() == 0 {
        return false;
    }
    let mut p = Gate::new();
    if p.close() != Err(BH_ESTAGE) {
        return false;
    }
    if p.step(Stage::Recognize) != Err(BH_EBAL) {
        return false;
    }
    let mut walk2 = Gate::new();
    if walk2.walk_all() != Ok(STAGE_COUNT) {
        return false;
    }
    if h.seal() != walk2.seal() {
        return false;
    }
    if Gate::new().seal() == 0 || h.seal() == Gate::new().seal() {
        return false;
    }
    let mut half = Gate::new();
    if half.step(Stage::Args) != Ok(()) {
        return false;
    }
    if half.seal() == h.seal() {
        return false;
    }
    true
}

/// 13 账本：槽位纪律、逐格对账、冻结重放、篡改留痕。
fn ledger_ok() -> bool {
    let mut fresh = Ledger::new();
    if !fresh.is_empty() || fresh.len() != 0 || fresh.rejects() != 0 || fresh.is_frozen() {
        return false;
    }
    if fresh.root() != 0 {
        return false;
    }
    let cell0 = Sample {
        slot: 0,
        op: OP_STAT,
        ok: true,
        code: 0,
        value: 14,
    };
    if fresh.register(Sample { slot: 1, ..cell0 }) != Err(BH_ESLOT) {
        return false;
    }
    if fresh.rejects() != 1 || fresh.len() != 0 {
        return false;
    }
    if fresh.register(Sample { slot: 14, ..cell0 }) != Err(BH_ESLOT) {
        return false;
    }
    if fresh.register(cell0) != Ok(()) {
        return false;
    }
    if fresh.len() != 1 {
        return false;
    }
    if fresh.sample(1).is_some() || fresh.sample(0) != Some(cell0) {
        return false;
    }
    if fresh.freeze() != Err(BH_EBAL) {
        return false;
    }
    let mut l = build_ledger();
    if l.len() != SAMPLES || l.rejects() != 0 {
        return false;
    }
    if l.ok_count() != ACCEPT || l.err_count() != REJECT {
        return false;
    }
    if l.audit() != Ok(()) {
        return false;
    }
    if l.seal() == 0 {
        return false;
    }
    let pre = l.seal();
    let root = match l.freeze() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if root == 0 || !l.is_frozen() || l.root() != root || l.root() != pre {
        return false;
    }
    if l.register(cell0) != Err(BH_ESLOT) {
        return false;
    }
    if l.freeze() != Err(BH_ESTATE) {
        return false;
    }
    if l.tamper(0, 7) != Err(BH_ESTATE) {
        return false;
    }
    if l.rejects() < 3 {
        return false;
    }
    if l.replay() != Ok(root) {
        return false;
    }
    let mut t = build_ledger();
    if t.tamper(0, 15) != Ok(()) {
        return false;
    }
    if t.audit() == Ok(()) {
        return false;
    }
    if t.replay() != Err(BH_ESEAL) {
        return false;
    }
    if build_ledger().seal() != l.seal() {
        return false;
    }
    let mut codes_seen = [0u32; DIM_COUNT];
    let mut c = 0usize;
    while c < DIM_COUNT {
        let Some(s) = l.sample(ACCEPT + c) else {
            return false;
        };
        if s.ok || s.value != 0 || s.op != OP_DEFECT {
            return false;
        }
        if let Some(want) = REJECT_CODES.get(c) {
            if s.code != *want {
                return false;
            }
        } else {
            return false;
        }
        if let Some(slot) = codes_seen.get_mut(c) {
            *slot = s.code;
        }
        c += 1;
    }
    if !u32s_distinct(&codes_seen) {
        return false;
    }
    let mut s0 = 0usize;
    while s0 < ACCEPT {
        let Some(cell) = l.sample(s0) else {
            return false;
        };
        if !cell.ok || cell.code != 0 {
            return false;
        }
        if let Some(want) = VALS.get(s0) {
            if cell.value != *want {
                return false;
            }
        } else {
            return false;
        }
        s0 += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 汇总入口：13 项判据，逐项对应一个错误码维
// ---------------------------------------------------------------------------

/// 本卡自检：断言方向全部是"判据本身成立"，任何一维退化都变红。
pub fn run_unx11667_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11667_bb_help_exit0");

    s.add("BH01-名称与段基址", name_ok(), "static");

    s.add("BH02-枚举与闸表", enums_ok(), "static");

    s.add("BH03-现场合法域", shape_ok(), "static");

    s.add("BH04-七闸纵线", define_ok(), "static");

    s.add("BH05-判序六维", judge_ok(), "static");

    s.add("BH06-语料与期望表", corpus_ok(), "static");

    s.add("BH07-矩阵分档", matrix_ok(), "static");

    s.add("BH08-槽位与期望", slots_ok(), "static");

    s.add("BH09-红档率九二九", reds_ok(), "static");

    s.add("BH10-文案逐字", message_ok(), "static");

    s.add("BH11-报表与封印", report_ok(), "static");

    s.add("BH12-阶段闸门", gate_ok(), "static");

    s.add(
        "BH13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            let inj = inject_four();
            let mut four = true;
            let mut i = 0usize;
            while i < inj.len() {
                if let Some(v) = inj.get(i) {
                    if !*v {
                        four = false;
                    }
                } else {
                    four = false;
                }
                i += 1;
            }
            a && b && ledger_ok() && four
        },
        "static",
    );

    s
}
