//! UNX-F11673 applet 表完整性自检判据（build 清单 vs --list）
//!
//! 这张卡管的是**两张名单是不是同一张名单**。busybox 是单一多路复用二进制：
//! 编译期由 `CONFIG_APPLET_*` 展开成一份**清单**（applet_arr-v.h 里的名字序列 +
//! `applet_name[]` / `applet_main[]` 两条等长数组），运行期 `busybox --list`
//! 把**编进这个二进制里的**名字逐行吐出来。判据原文要求的是一件事：
//! **清单 ↔ --list 零偏差**，并且把**重名**钉成锚（防重锚）。
//!
//! ```text
//! $ busybox --list | wc -l
//! 20
//! $ grep -c '^CONFIG_APPLET.*=y' .config | ...      ← 清单侧口径
//! 20
//! $ busybox --install /tmp/bin                       ← 两者不一致时后果最重
//! ```
//!
//! 为什么"零偏差"必须是**集合 + 序 + 基数 + 唯一性**四件事一起，而不是"条数
//! 相等"一件事？因为条数相等是最容易假绿的判据：清单里有 `ls` 缺 `cat`，
//! `--list` 里有 `cat` 缺 `ls`，两边都报 20 行——**基数闸全绿，集合却是错的**。
//! 这种"互缺互补"的现场在裁剪矩阵里天天发生（`make allnoconfig` 之后手工补
//! 两个开关，`.config` 与生成的表就分家了）。本卡把偏差拆成四个互不相干的量：
//! `missing`（清单有、表里没有）、`ghost`（表里有、清单没打开）、
//! `dup`（同一名字占了两个表项）、`inv`（名字序列不再严格字典序），
//! 并把它们**加成一个数**落在 `value` 维上，任何一维单独退化都逃不掉。
//!
//! **防重锚**为什么单独一道闸（`Dup`）而不是并进零偏差里算一个数？因为重名的
//! 后果与漏名完全不同：漏名让人调不到工具（`applet not found`，那是后置卡
//! F11674 的文案面），重名却让 `--install` **对同一个链接名建两次链**——第二次的
//! 语义取决于实现，要么静默跳过、要么 `File exists` 报错、要么把前一次的真链
//! 覆盖成假链。更阴的是重名在 `--list` 里表现为"多一行"，基数偏差恰好被
//! "清单也算两次"抵消，**只有唯一性闸能看见它**。所以本卡把唯一性判成
//! `EEXIST`（17）而不是 `ENOENT`（2），并把"落没落防重锚"独立成 `default` 维。
//!
//! 另一层深度是**清单一侧本身有三种口径**，它们互相之间也会不一致：
//! `.config` 展开（人写的意图）、生成的 `applet_name[]`（脚本算出的事实）、
//! 链接期符号表（真正躺在二进制里的东西）。`applet_name[]` 与 `applet_main[]`
//! **不等长**是构建系统的账没平，不是运行期的账——这一档本卡判死在 `Gen` 闸，
//! 且**不许**给它落任何运行期锚。别名（alt name，如 `dir`/`pg`/`[`/`[[`）也
//! 在这一层处理：别名**不是独立表项**，它只是宿主 applet 的另一个链接名；
//! 把别名当 applet 登进表，`--list` 就会比清单多行，而 `--install` 建出来的
//! 是同一条链——这正是"基数对、集合错"的另一副面孔。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 八道闸，秩从 **1** 起算；`Gate` 只接受"前进到下一秩"，倒退 → `AT_ESTAGE`，
//! `close()` 后一切 `step`/`close` → `AT_ESTATE`。前一闸不产出事实，后一闸
//! 就没有输入，倒过来必先产生假绿。
//!
//! 1. `cfg`（秩 1）——清单来源闸。`.config` 的开关名到 applet 名的映射非法
//!    （开关打开但名字不在全集里、同一开关出现两次取值不同、清单为空）都在
//!    这里出局。排在最前是因为**没有清单就没有比对对象**：拿一份不存在的
//!    清单去比 `--list`，任何结果都能自称零偏差。
//! 2. `gen`（秩 2）——生成表闸。`applet_name[]` 与 `applet_main[]` 必须等长，
//!    且长度等于清单条数。这是**构建系统的账**，它红了说明生成的头文件与
//!    配置分家，后面所有运行期观测都不可信。
//! 3. `link`（秩 3）——链接配对闸。表项的 main 符号在二进制里必须可达
//!    （非 `None`、非占位）。名字有、函数没链进来 = 调用时段错误，比调用时
//!    报 not found 更危险，所以它排在成员比对**之前**。
//! 4. `dup`（秩 4）——防重闸。对观测序列做真实重名扫描：同一个下标出现两次、
//!    同一个名字被两个表项占据、别名以独立表项身份混进来，都在这里落 `EEXIST`。
//!    它排在集合比对之前：重名会污染基数，先清掉重复再谈"缺了谁、多了谁"。
//! 5. `usage`（秩 5）——文案配对闸。usage 串表与名字表必须一一配对：
//!    孤儿串（有文案无表项）白占只读段，缺失串（有表项无文案）让 `--help`
//!    打空行。本卡只判**配对**，串的内容逐字归 F11667/F11668。
//! 6. `list`（秩 6）——零偏差主闸。清单成员集 ↔ `--list` 成员集，逐位比
//!    `missing`/`ghost`。排在唯一性之后：序列去重前，集合差是算不准的。
//! 7. `sort`（秩 7）——序闸。成员齐了才谈序：观测序列必须严格字典序递增
//!    （等长、无缺口、无相邻交换）。倒在这里说明生成脚本的排序步被跳过，
//!    人读的 `--list` 与 `--help` 名录会分组错乱。
//! 8. `exit`（秩 8）——结算闸。前七闸全绿才允许落防重锚；锚只能落在
//!    "唯一性闸已实扫"的行上。申报的退出码与档位不一致在这里判死。
//!
//! ## 关键差异（本卡深度所在）
//!
//! * **与前置 F11672（busybox 对未知名 `--install` 降级，段基址 `0x6d250`，
//!   位距 `0x10`）的边界**：F11672 判**建链那一刻**对未知名字怎么办（跳过、
//!   不建伪链）；本卡判**建链之前**两张名单是不是一回事。同一个 `--install`
//!   现场，F11672 数"跳过了几条"，本卡数"偏差几格、锚落没落"。两卡不互相
//!   import，段位由 `anchors_agree()` 复算对齐。
//! * **与 F11671（applet 名大小写与别名匹配，`0x6d230`，位距 `0x30`）的边界**：
//!   F11671 判**一次查找**能不能按大小写/别名命中宿主；本卡判**别名有没有被
//!   误登记成独立表项**——别名出现在表里是完整性缺陷，不是查找缺陷。反过来说，
//!   `LS` 查不到这种大小写问题归 F11671，本卡不管查找命中，只管表内唯一。
//! * **与后置 F11674（未知 applet 文案逐字，`0x6d270`，位距 `0x10`）的边界**：
//!   本卡是它的**上游闸门**——表里少了名字，运行期就产生 `applet not found`
//!   那一行；但本卡不判那一行的文案、前缀、通道，只判"这一行本该不该出现"。
//!   文案面判死权在 F11674，名单面判死权在本卡。
//! * **与 F11676（裁剪矩阵重建抽样，`0x6d2a0`，位距 `0x40`）的边界**：F11676
//!   在**三档 CONFIG** 之间横向抽样比较（同一份源码三种裁剪结果），本卡在
//!   **单一档内**纵向比较（一份裁剪的清单与它的 `--list`）。裁剪档让名单变短
//!   本身不是偏差——清单与表一起变短才是本卡的绿；只短了一边才是本卡的红。
//! * **与 F11667（busybox --help 退出码 0 与 usage 全文，`0x6d1e0`，位距 `0x80`）
//!   的边界**：F11667 判帮助全文逐字与退出码；本卡的 `usage` 闸只判 usage 串与
//!   表项**是否配对**。名录分组行的内容对不对归 F11667，行数是够归本卡。
//! * **本卡的排他职责**：① 偏差四维（missing/ghost/dup/inv）分开计算再合成，
//!   杜绝"基数相等即零偏差"的假绿；② 清单三口径（config/gen/link）各自成闸，
//!   构建系统的账与运行期的账不许混；③ 防重锚作为 `default` 维，且只有真实
//!   扫过唯一性的行才允许落锚；④ 别名不占表项；⑤ `--list` 序为严格字典序。
//! * **谁判死谁放行**：三口径同长、成员集零偏差、序列严格递增、无重名无别名
//!   混登、usage 配对完整 → 放行并落防重锚；把"互缺互补"读成零偏差、把重名
//!   计入基数、把裁剪后的短名单报成漏名、把别名当 applet 登表、数组长度不等
//!   却落运行期锚 → 一律判死。
//!
//! 判据六维：结束阶段（stage）、偏差 errno（code）、偏差格数（value）、
//! 偏差档与口径（note）、防重锚（default）、成败申报（estate）。
//! 判序固定：状态 → 阶段 → 错误码 → 偏差数 → 注解 → 锚位。
//!
//! 语料 14 槽：8 槽申报正确（零偏差 3、落锚 2、裁剪短名单 1、argv0 宿主反查 1、
//! 未观测 1），6 槽一槽只坏一维，依次挂 Estate/Stage/Code/Value/Note/Default。
//! 账本 14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码，冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11673）。
pub const SEG_BASE: u32 = 0x6d260;

// ---------------------------------------------------------------------------
// 交叉署名：相邻卡的段基址与位距（不 import，只复算对齐）
// ---------------------------------------------------------------------------

/// 前置卡 F11672「busybox 对未知名 --install 降级」的段基址。
pub const ANCHOR_F11672_SEG: u32 = 0x6d250;
/// F11672 与本卡的位距（0x6d260 - 0x6d250）。
pub const ANCHOR_F11672_GAP: u32 = 0x10;
/// 前置卡 F11671「applet 名大小写与别名匹配」的段基址。
pub const ANCHOR_F11671_SEG: u32 = 0x6d230;
/// F11671 与本卡的位距。
pub const ANCHOR_F11671_GAP: u32 = 0x30;
/// 后置卡 F11674「未知 applet 文案逐字」的段基址。
pub const ANCHOR_F11674_SEG: u32 = 0x6d270;
/// 本卡与 F11674 的位距。
pub const ANCHOR_F11674_GAP: u32 = 0x10;
/// 后置卡 F11676「裁剪矩阵重建抽样」的段基址。
pub const ANCHOR_F11676_SEG: u32 = 0x6d2a0;
/// 本卡与 F11676 的位距。
pub const ANCHOR_F11676_GAP: u32 = 0x40;
/// 远端卡 F11667「busybox --help 退出码 0 与 usage 全文」的段基址。
pub const ANCHOR_F11667_SEG: u32 = 0x6d1e0;
/// F11667 与本卡的位距。
pub const ANCHOR_F11667_GAP: u32 = 0x80;

/// 交叉署名：`--list` 每行一个名字的逐字尾巴（无空格、无引号）。
pub const ANCHOR_LIST_LINE_TAIL: &str = "\n";
/// 交叉署名：帮助文本名录段标题逐字（busybox --help）。
pub const ANCHOR_LIST_HEADING: &str = "Currently defined functions:";
/// 交叉署名：usage 首行逐字。
pub const ANCHOR_USAGE_HEAD: &str = "Usage: busybox [function [arguments]...]";
/// 交叉署名：usage 第二行逐字（三空格缩进）。
pub const ANCHOR_USAGE_OR_LIST: &str = "   or: busybox --list[-full]";
/// 交叉署名：usage 第三行逐字。
pub const ANCHOR_USAGE_OR_INSTALL: &str = "   or: busybox --install [-s] [DIR]";
/// 交叉署名：下游 F11674 主锚尾巴（本卡只声明，不判文案）。
pub const ANCHOR_NOTFOUND_TAIL: &str = "applet not found";
/// 交叉署名：`--list` 成功后退 0。
pub const ANCHOR_LIST_EXIT_OK: u32 = 0;
/// 交叉署名：表检红（名单不齐）时的**工具面**码，与 F11672 的跳过档同源。
pub const ANCHOR_SKIP_EXIT: u32 = 1;

// ---------------------------------------------------------------------------
// 规模常量
// ---------------------------------------------------------------------------

/// 语料槽数。
pub const CORPUS: usize = 14;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 申报正确槽数。
pub const ACCEPT: usize = 8;
/// 申报错误槽数。
pub const REJECT: usize = 6;
/// 档位数量。
pub const TIER_COUNT: usize = 4;
/// 闸门数量。
pub const STAGE_COUNT: u32 = 8;
/// 最后一道闸的秩。
pub const STAGE_LAST_RANK: u32 = 8;
/// 账本统计格数（红档格之外）。
pub const STAT_CELLS: usize = 7;
/// 红档率标定值（千分率）：一格被强行改坏后的对判率。
pub const RED_RATE: u32 = 929;
/// 全绿千分率。
pub const RATE_FULL: u32 = 1000;

/// 全集（applet 名册）大小：本卡比对域内的规范名字数。
pub const UNIVERSE: usize = 20;
/// 观测序列容量（与全集等宽，尾用哨兵填充）。
pub const SEQ_CAP: usize = UNIVERSE;
/// 观测序列结束哨兵。
pub const SEQ_END: u8 = 0xff;
/// 别名登记对数。
pub const ALIAS_COUNT: usize = 4;
/// applet 名长度上限（与 busybox 的 applet 名字段一致）。
pub const APPLET_NAME_MAX: usize = 30;
/// CONFIG 前缀逐字。
pub const CONFIG_PREFIX: &str = "CONFIG_";
/// CONFIG_APPLET_ 前缀逐字。
pub const CONFIG_APPLET_PREFIX: &str = "CONFIG_APPLET_";

// ---------------------------------------------------------------------------
// errno：本卡的偏差归因码（真实 Linux 值 + glibc/musl 逐字文案）
// ---------------------------------------------------------------------------

/// 无errno。
pub const ERR_NONE: i32 = 0;
/// 表里没有这个名字（漏名/幽灵名）。
pub const ERR_ENOENT: i32 = 2;
/// 名字被两个表项占据（防重闸）。
pub const ERR_EEXIST: i32 = 17;
/// 口径本身非法（映射不存在、清单为空、长度不配对）。
pub const ERR_EINVAL: i32 = 22;
/// 表项函数不可达（链接期符号缺失）。
pub const ERR_ENOSYS: i32 = 38;
/// 序错（非严格字典序）。
pub const ERR_ENOTDIR: i32 = 20;
/// usage 串缺失/孤儿：文案面邻居码，本卡只归因不判文案。
pub const ERR_ENOMSG: i32 = 42;
/// 越界索引：观测序列里出现全集外的下标。
pub const ERR_ERANGE: i32 = 34;
/// 邻居：ENOEXEC（清单里的名字在二进制里根本不是 applet）。
pub const ERR_ENOEXEC: i32 = 8;
/// 邻居：ENOTRECOVERABLE 家族里的 EILSEQ，别名混登用。
pub const ERR_EILSEQ: i32 = 84;
/// errno 全集大小。
pub const ERRNO_SET_SIZE: usize = 10;
/// 本卡可归因的偏差码数量（missing/ghost/dup/inv/orphan/len/alias/usage）。
pub const DEVIATION_CODE_COUNT: usize = 4;

/// 本卡归因用的 errno 全集（顺序即 `errno_known` 的查表顺序）。
pub const KNOWN_ERRNOS: [i32; ERRNO_SET_SIZE] = [
    ERR_NONE,
    ERR_ENOENT,
    ERR_ENOEXEC,
    ERR_EEXIST,
    ERR_ENOTDIR,
    ERR_EINVAL,
    ERR_ERANGE,
    ERR_ENOSYS,
    ERR_ENOMSG,
    ERR_EILSEQ,
];

/// 邻居码表：本卡**不判死**、只登记存在的码。
pub const NEIGHBOUR_ERRNOS: [i32; 6] = [2, 13, 20, 22, 38, 42];

// ---------------------------------------------------------------------------
// 退出码档
// ---------------------------------------------------------------------------

/// 成功。
pub const EXIT_OK: u32 = 0;
/// 工具面失败（表检红）。
pub const EXIT_FILE: u32 = 1;
/// 用法错。
pub const EXIT_USAGE: u32 = 2;
/// 未名（下游 F11674 的码，本卡只声明）。
pub const EXIT_NOTFOUND: u32 = 127;
/// 未判定占位。
pub const EXIT_UNJUDGED: u32 = 99;
/// 退出码档数。
pub const EXIT_RANKS: usize = 5;

// ---------------------------------------------------------------------------
// 13 个错误码（前缀 AT = applet table）
// ---------------------------------------------------------------------------

/// 阶段违例：申报的闸位比实际闸位靠前或靠后（倒退即谎报观测点）。
pub const AT_ESTAGE: u32 = SEG_BASE + 0x01;
/// 错误码违例：偏差归因码报成了另一个 errno。
pub const AT_ECODE: u32 = SEG_BASE + 0x02;
/// 偏差数违例：`value`（偏差格数）与实扫不符。
pub const AT_EVALUE: u32 = SEG_BASE + 0x03;
/// 注解违例：偏差档（missing/ghost/dup/inv…）报错了家族。
pub const AT_ENOTE: u32 = SEG_BASE + 0x04;
/// 锚位违例：防重锚该落不落、不该落却落。
pub const AT_EDEFAULT: u32 = SEG_BASE + 0x05;
/// 表构造违例：观测序列本身非法（越界下标、长度与基数不配对）。
pub const AT_ETABLE: u32 = SEG_BASE + 0x06;
/// 状态违例：闸门已关闭仍被推进、账本已冻结仍被登记。
pub const AT_ESTATE: u32 = SEG_BASE + 0x07;
/// 计数违例：槽数、段数、档位合计对不上。
pub const AT_ECOUNT: u32 = SEG_BASE + 0x08;
/// 槽位违例：越界槽号或跳号登记。
pub const AT_ESLOT: u32 = SEG_BASE + 0x09;
/// 指纹违例：重放指纹与冻结根指纹不符。
pub const AT_ESEAL: u32 = SEG_BASE + 0x0a;
/// 防重违例：表内重名或别名混登（本卡最专有一维）。
pub const AT_EDUP: u32 = SEG_BASE + 0x0b;
/// 序违例：`--list` 观测序不是严格字典序。
pub const AT_EORDER: u32 = SEG_BASE + 0x0c;
/// 账平违例：账本合格数、成败分布与 `VALS` 对不上。
pub const AT_EBAL: u32 = SEG_BASE + 0x0d;

/// 十三码全集（`name_ok` 逐格核验）。
pub const ALL_CODES: [u32; 13] = [
    AT_ESTAGE,
    AT_ECODE,
    AT_EVALUE,
    AT_ENOTE,
    AT_EDEFAULT,
    AT_ETABLE,
    AT_ESTATE,
    AT_ECOUNT,
    AT_ESLOT,
    AT_ESEAL,
    AT_EDUP,
    AT_EORDER,
    AT_EBAL,
];

// ---------------------------------------------------------------------------
// 账本操作码
// ---------------------------------------------------------------------------

/// 清单来源闸。
pub const OP_CFG: u8 = 0x01;
/// 生成表闸。
pub const OP_GEN: u8 = 0x02;
/// 链接配对闸。
pub const OP_LINK: u8 = 0x03;
/// 防重闸。
pub const OP_DUP: u8 = 0x04;
/// usage 配对闸。
pub const OP_USAGE: u8 = 0x05;
/// 零偏差主闸。
pub const OP_LIST: u8 = 0x06;
/// 序闸。
pub const OP_SORT: u8 = 0x07;
/// 语料统计。
pub const OP_CORPUS: u8 = 0x08;
/// 红档率标定。
pub const OP_RATE: u8 = 0x09;
/// 缺陷码格。
pub const OP_DEFECT: u8 = 0x0a;
/// 重放格。
pub const OP_REPLAY: u8 = 0x0b;
/// 操作码数量。
pub const OP_COUNT: usize = 11;

/// 操作码全集。
pub const OP_BITS: [u8; OP_COUNT] = [
    OP_CFG, OP_GEN, OP_LINK, OP_DUP, OP_USAGE, OP_LIST, OP_SORT, OP_CORPUS, OP_RATE,
    OP_DEFECT, OP_REPLAY,
];

// ---------------------------------------------------------------------------
// 全集名册与别名登记（逐字照 busybox 惯用名）
// ---------------------------------------------------------------------------

/// 规范名册：**必须严格字典序**（本卡的序闸以此为基准）。
pub const APPLETS: [&str; UNIVERSE] = [
    "addgroup", "arping", "awk", "basename", "cat", "cp", "date", "dmesg", "echo", "env", "grep",
    "gzip", "halt", "hostid", "id", "kill", "ls", "more", "sh", "test",
];

/// 别名 → 宿主。别名**不占表项**，只在 `--install` 时作为额外链接名出现。
pub const ALIAS_PAIRS: [(&str, &str); ALIAS_COUNT] = [
    ("[", "test"),
    ("[[", "test"),
    ("dir", "ls"),
    ("pg", "more"),
];

/// 位集全集掩码（低 `UNIVERSE` 位为 1）。
pub const MASK_ALL: u32 = 0x000f_ffff;

/// 取一位。
pub const fn bit(i: u32) -> u32 {
    1u32 << i
}

/// 去掉一位。
pub const fn without(m: u32, i: u32) -> u32 {
    m & !bit(i)
}

/// 加上一位。
pub const fn plus(m: u32, i: u32) -> u32 {
    m | bit(i)
}

/// 对称差（谁多谁少都算）。
pub const fn xor_mask(a: u32, b: u32) -> u32 {
    a ^ b
}

/// 掩码基数（逐位计数，不依赖 popcnt  intrinsic）。
pub const fn popcount(m: u32) -> u32 {
    let mut v = m;
    let mut n = 0u32;
    while v != 0 {
        if v & 1 == 1 {
            n = n + 1;
        }
        v = v >> 1;
    }
    n
}

/// 按掩码造一个**严格字典序**观测序列（掩码位序即名册序，天然递增）。
pub const fn seq_of(m: u32) -> [u8; SEQ_CAP] {
    let mut out = [SEQ_END; SEQ_CAP];
    let mut i = 0usize;
    let mut w = 0usize;
    while i < UNIVERSE {
        if m & bit(i as u32) != 0 {
            if i < SEQ_CAP {
                out[i] = i as u8;
            }
            w = w + 1;
        } else {
            if i < SEQ_CAP {
                out[i] = SEQ_END;
            }
        }
        i = i + 1;
    }
    // 压实：把空洞挪到尾部，保持名册顺序不变。
    let mut a = 0usize;
    let mut b = 0usize;
    while b < SEQ_CAP {
        if out[b] != SEQ_END {
            out[a] = out[b];
            if a != b {
                out[b] = SEQ_END;
            }
            a = a + 1;
        }
        b = b + 1;
    }
    while a < SEQ_CAP {
        out[a] = SEQ_END;
        a = a + 1;
    }
    let _ = w;
    out
}

/// 造一个**相邻交换**后的乱序序列（真实倒序位，序闸要能扫出来）。
pub const fn scramble(src: [u8; SEQ_CAP], i: usize, j: usize) -> [u8; SEQ_CAP] {
    let mut out = src;
    if i < SEQ_CAP && j < SEQ_CAP {
        let a = src[i];
        let b = src[j];
        out[i] = b;
        out[j] = a;
    }
    out
}

/// 造一个**把某位置成另一个下标**的序列（模拟重名：两个表项指向同一名字）。
pub const fn duplicate(src: [u8; SEQ_CAP], i: usize, name: u8) -> [u8; SEQ_CAP] {
    let mut out = src;
    if i < SEQ_CAP {
        out[i] = name;
    }
    out
}

/// 造一个**塞进额外一项**的序列（模拟别名混登：表比清单多一行）。
pub const fn append_seq(src: [u8; SEQ_CAP], extra: u8) -> [u8; SEQ_CAP] {
    let mut out = src;
    let mut i = 0usize;
    while i < SEQ_CAP {
        if out[i] == SEQ_END {
            out[i] = extra;
            break;
        }
        i = i + 1;
    }
    out
}

/// 空序列（全哨兵）。
pub const fn empty_seq() -> [u8; SEQ_CAP] {
    [SEQ_END; SEQ_CAP]
}

// ---------------------------------------------------------------------------
// FNV-1a 指纹
// ---------------------------------------------------------------------------

/// FNV-1a 64 位初始偏移量。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折进一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    let x = (h ^ (b as u64)).wrapping_mul(FNV_PRIME);
    x
}

/// 折进一个机器字（逐字节展开，保证与 `fold_byte` 同一条折叠链）。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut v = w;
    let mut acc = h;
    let mut i = 0usize;
    while i < 8 {
        acc = fold_byte(acc, (v & 0xff) as u8);
        v = v >> 8;
        i = i + 1;
    }
    acc
}

/// 折进一个字符串（长度先入链，防 `"ab"` 与 `"ab\0"` 同指纹）。
fn fold_str(h: u64, s: &str) -> u64 {
    let mut acc = fold_word(h, s.len() as u64);
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let Some(b) = bytes.get(i) else {
            break;
        };
        acc = fold_byte(acc, *b);
        i = i + 1;
    }
    acc
}

/// 折进一个有符号 errno（负数按补码展开，符号位必然进链）。
fn fold_i32(h: u64, v: i32) -> u64 {
    fold_word(h, v as u32 as u64)
}

/// 折进一个布尔。
fn fold_bool(h: u64, v: bool) -> u64 {
    fold_word(h, if v { 1u64 } else { 0u64 })
}

// ---------------------------------------------------------------------------
// 数字/布尔文本
// ---------------------------------------------------------------------------

/// u32 → 十进制文本（不递归，前导零不吞）。
fn dec_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut rev = String::new();
    let mut x = v;
    while x > 0 {
        let d = (x % 10) as u8;
        x = x / 10;
        let c = (b'0' + d) as char;
        rev.push(c);
    }
    let mut out = String::new();
    let bytes = rev.as_bytes();
    let mut i = bytes.len();
    while i > 0 {
        i = i - 1;
        if let Some(b) = bytes.get(i) {
            out.push(*b as char);
        }
    }
    out
}

/// 小写十六进制，定宽（高位补零）。
fn hex_text(v: u32, width: usize) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::new();
    if width == 0 {
        return out;
    }
    let mut i = 0usize;
    while i < width {
        let shift = ((width - 1 - i) * 4) as u32;
        let nib = (v >> shift) & 0xf;
        let c = match DIGITS.get(nib as usize) {
            Some(b) => *b as char,
            None => '0',
        };
        out.push(c);
        i = i + 1;
    }
    out
}

/// u32 文本别名（判据侧统一入口）。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// i32 文本（errno 用，负号显式）。
fn i32_text(v: i32) -> String {
    if v < 0 {
        let mut s = String::from("-");
        s.push_str(dec_text(v.unsigned_abs()).as_str());
        s
    } else {
        dec_text(v as u32)
    }
}

/// 布尔文本。
fn bool_text(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

/// 段码文本：`0x6d26a` 形式。
pub fn seg_text(v: u32) -> String {
    let mut s = String::from("0x");
    s.push_str(hex_text(v, 5).as_str());
    s
}

/// `(码, 名字)` 成对文本：`errno=17/EEXIST`。
pub fn errno_pair_text(code: i32) -> String {
    let mut s = String::from("errno=");
    s.push_str(i32_text(code).as_str());
    s.push('/');
    s.push_str(errno_name(code));
    s
}

/// 偏差数文本：`dev=3`。
pub fn deviation_text(n: u32) -> String {
    let mut s = String::from("dev=");
    s.push_str(dec_text(n).as_str());
    s
}

/// 掩码文本：`m=0x000fffff`。
pub fn mask_text(m: u32) -> String {
    let mut s = String::from("m=0x");
    s.push_str(hex_text(m, 6).as_str());
    s
}

// ---------------------------------------------------------------------------
// errno 文案（glibc 主表逐字；musl 分歧档另立一表）
// ---------------------------------------------------------------------------

/// glibc / musl 共用的 strerror 主表（Linux errno 值，逐字照抄 glibc）。
pub fn strerror(code: i32) -> &'static str {
    match code {
        0 => "",
        1 => "Operation not permitted",
        2 => "No such file or directory",
        5 => "Input/output error",
        8 => "Exec format error",
        13 => "Permission denied",
        17 => "File exists",
        20 => "Not a directory",
        22 => "Invalid argument",
        34 => "Numerical result out of range",
        38 => "Function not implemented",
        42 => "No message of desired type",
        75 => "Value too large for defined data type",
        84 => "Illegal byte sequence",
        122 => "Quota exceeded",
        _ => "Unknown error",
    }
}

/// musl 与 glibc 文案**不同**的那几档；其余一律回落到 `strerror`。
///
/// 本卡的 `ERANGE`(34) 正落在分歧档上：清单基数溢出时，两台基准机会打出
/// 不同的一行，所以逐字判据必须把 libc 家族当成一维。
pub fn strerror_musl(code: i32) -> &'static str {
    match code {
        34 => "Result not representable",
        75 => "Value too large for data type",
        _ => strerror(code),
    }
}

/// libc 家族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Libc {
    /// glibc。
    Glibc,
    /// musl。
    Musl,
}

impl Libc {
    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Libc::Glibc => "glibc",
            Libc::Musl => "musl",
        }
    }

    /// 全集。
    pub fn all() -> [Libc; 2] {
        [Libc::Glibc, Libc::Musl]
    }

    /// 按本表取文案。
    pub fn text(&self, code: i32) -> &'static str {
        match self {
            Libc::Glibc => strerror(code),
            Libc::Musl => strerror_musl(code),
        }
    }
}

/// 两 libc 在该 errno 上是否逐字分歧。
pub fn libc_diverges(code: i32) -> bool {
    strerror(code) != strerror_musl(code)
}

/// errno 符号名（本卡用到的那一撮）。
pub fn errno_name(code: i32) -> &'static str {
    match code {
        0 => "OK",
        2 => "ENOENT",
        5 => "EIO",
        8 => "ENOEXEC",
        13 => "EACCES",
        17 => "EEXIST",
        20 => "ENOTDIR",
        22 => "EINVAL",
        34 => "ERANGE",
        38 => "ENOSYS",
        42 => "ENOMSG",
        75 => "EOVERFLOW",
        84 => "EILSEQ",
        122 => "EDQUOT",
        _ => "E?",
    }
}

/// 该 errno 是否在本卡名册里。
pub fn errno_known(code: i32) -> bool {
    let mut i = 0usize;
    while i < KNOWN_ERRNOS.len() {
        if let Some(c) = KNOWN_ERRNOS.get(i) {
            if *c == code {
                return true;
            }
        }
        i = i + 1;
    }
    false
}

/// 该 errno 是否本卡的**偏差归因码**（missing/dup/口径/链接 四族）。
pub fn errno_deviation(code: i32) -> bool {
    matches!(
        code,
        ERR_ENOENT | ERR_EEXIST | ERR_EINVAL | ERR_ENOSYS | ERR_ENOTDIR | ERR_ENOMSG
            | ERR_ERANGE
    )
}

/// 该 errno 是否邻居码（存在但本卡不判死）。
pub fn errno_neighbour(code: i32) -> bool {
    let mut i = 0usize;
    while i < NEIGHBOUR_ERRNOS.len() {
        if let Some(c) = NEIGHBOUR_ERRNOS.get(i) {
            if *c == code {
                return true;
            }
        }
        i = i + 1;
    }
    false
}

/// 名册下标 → 名字。
pub fn applet_at(i: usize) -> Option<&'static str> {
    APPLETS.get(i).copied()
}

/// 名字 → 名册下标（线性扫，本卡不用哈希：名册短且必须严格序）。
pub fn applet_index(name: &str) -> Option<usize> {
    let mut i = 0usize;
    while i < APPLETS.len() {
        if let Some(a) = APPLETS.get(i) {
            if *a == name {
                return Some(i);
            }
        }
        i = i + 1;
    }
    None
}

/// 别名 → 宿主（查不到给 `None`，即"这不是别名"）。
pub fn alias_owner(name: &str) -> Option<&'static str> {
    let mut i = 0usize;
    while i < ALIAS_PAIRS.len() {
        if let Some((alt, host)) = ALIAS_PAIRS.get(i) {
            if *alt == name {
                return Some(*host);
            }
        }
        i = i + 1;
    }
    None
}

/// 该名字是不是别名。
pub fn is_alias(name: &str) -> bool {
    alias_owner(name).is_some()
}

/// 名字长度是否越界（applet 字段宽）。
pub fn name_fits(name: &str) -> bool {
    name.len() <= APPLET_NAME_MAX && name.len() > 0
}

/// 名册本身是否严格字典序（本卡自己的地基，先于一切比对）。
pub fn applet_roster_sorted() -> bool {
    let mut i = 1usize;
    while i < APPLETS.len() {
        let Some(prev) = APPLETS.get(i - 1) else {
            return false;
        };
        let Some(cur) = APPLETS.get(i) else {
            return false;
        };
        if prev >= cur {
            return false;
        }
        i = i + 1;
    }
    true
}

/// 名册掩码（全集）。
pub fn roster_mask() -> u32 {
    MASK_ALL
}

// ---------------------------------------------------------------------------
// 逐字文案：上游原文
// ---------------------------------------------------------------------------

/// `--list` 的一行：名字 + 换行，无缩进、无引号、无空格。
pub fn msg_list_line(name: &str) -> String {
    let mut s = String::from(name);
    s.push_str(ANCHOR_LIST_LINE_TAIL);
    s
}

/// `--list` 的整段输出（按观测序逐行）。
pub fn msg_list_body(seq: &[u8; SEQ_CAP]) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < SEQ_CAP {
        let Some(x) = seq.get(i) else {
            break;
        };
        if *x == SEQ_END {
            break;
        }
        if let Some(name) = applet_at(*x as usize) {
            s.push_str(msg_list_line(name).as_str());
        }
        i = i + 1;
    }
    s
}

/// `--list-full` 一行的名字列（本卡只到名字为止，描述串归文案卡 F11668）。
pub fn msg_list_full_line(name: &str) -> String {
    let mut s = String::from("\t");
    s.push_str(name);
    s
}

/// usage 三行块（逐字）。
pub fn msg_usage_block() -> String {
    let mut s = String::from(ANCHOR_USAGE_HEAD);
    s.push('\n');
    s.push_str(ANCHOR_USAGE_OR_LIST);
    s.push('\n');
    s.push_str(ANCHOR_USAGE_OR_INSTALL);
    s
}

/// banner 版权行（逐字）。
pub fn msg_copyright_head() -> String {
    String::from("BusyBox is copyrighted by many authors between 1998-2015.")
}

/// banner 许可行（逐字）。
pub fn msg_copyright_license() -> String {
    String::from("Licensed under GPLv2. See source files for details.")
}

/// banner 版本行（逐字）。
pub fn msg_version_line() -> String {
    String::from("BusyBox v1.36.1 (2023-11-08 23:30:50 UTC) multi-call binary.")
}

/// 名录段标题（逐字）。
pub fn msg_list_heading() -> String {
    String::from(ANCHOR_LIST_HEADING)
}

/// 名录段的一个分组行：`\t<字母>: <逗号分隔名单>`。
pub fn msg_list_group(letter: &str, names: &str) -> String {
    let mut s = String::from("\t");
    s.push_str(letter);
    s.push_str(": ");
    s.push_str(names);
    s
}

/// 本卡名册的 `c` 组行（`--help` 名录与 `--list` 必须同源）。
pub fn msg_list_group_c() -> String {
    msg_list_group("c", "cat, cp")
}

/// 本卡名册的 `d` 组行。
pub fn msg_list_group_d() -> String {
    msg_list_group("d", "date, dmesg")
}

/// 本卡名册的 `a` 组行。
pub fn msg_list_group_a() -> String {
    msg_list_group("a", "addgroup, arping, awk")
}

/// 本卡名册的 `h` 组行。
pub fn msg_list_group_h() -> String {
    msg_list_group("h", "halt, hostid")
}

/// 本卡名册的 `g` 组行。
pub fn msg_list_group_g() -> String {
    msg_list_group("g", "grep, gzip")
}

/// 本卡名册的 `b` 组行。
pub fn msg_list_group_b() -> String {
    msg_list_group("b", "basename")
}

/// 本卡名册的 `e` 组行。
pub fn msg_list_group_e() -> String {
    msg_list_group("e", "echo, env")
}

/// 本卡名册的 `i` 组行。
pub fn msg_list_group_i() -> String {
    msg_list_group("i", "id")
}

/// 本卡名册的 `k` 组行。
pub fn msg_list_group_k() -> String {
    msg_list_group("k", "kill")
}

/// 本卡名册的 `l` 组行。
pub fn msg_list_group_l() -> String {
    msg_list_group("l", "ls")
}

/// 本卡名册的 `m` 组行。
pub fn msg_list_group_m() -> String {
    msg_list_group("m", "more")
}

/// 本卡名册的 `s` 组行。
pub fn msg_list_group_s() -> String {
    msg_list_group("s", "sh")
}

/// 本卡名册的 `t` 组行。
pub fn msg_list_group_t() -> String {
    msg_list_group("t", "test")
}

/// 名录分组全表（按首字母，与 `APPLETS` 同源；本卡的 `usage`/`help` 交叉面）。
pub fn msg_help_groups() -> String {
    let mut s = msg_list_heading();
    s.push('\n');
    s.push_str(msg_list_group_a().as_str());
    s.push('\n');
    s.push_str(msg_list_group_b().as_str());
    s.push('\n');
    s.push_str(msg_list_group_c().as_str());
    s.push('\n');
    s.push_str(msg_list_group_d().as_str());
    s.push('\n');
    s.push_str(msg_list_group_e().as_str());
    s.push('\n');
    s.push_str(msg_list_group_g().as_str());
    s.push('\n');
    s.push_str(msg_list_group_h().as_str());
    s.push('\n');
    s.push_str(msg_list_group_i().as_str());
    s.push('\n');
    s.push_str(msg_list_group_k().as_str());
    s.push('\n');
    s.push_str(msg_list_group_l().as_str());
    s.push('\n');
    s.push_str(msg_list_group_m().as_str());
    s.push('\n');
    s.push_str(msg_list_group_s().as_str());
    s.push('\n');
    s.push_str(msg_list_group_t().as_str());
    s
}

/// 下游主锚（逐字）：`<prog>: applet not found`。**本卡不判这一行的好坏**。
pub fn msg_applet_not_found(prog: &str) -> String {
    let mut s = String::from(prog);
    s.push_str(": ");
    s.push_str(ANCHOR_NOTFOUND_TAIL);
    s
}

/// GNU coreutils `ln` 对已存在链接名的逐字报错（重名后果现场）。
pub fn msg_gnu_ln_exists(link: &str) -> String {
    let mut s = String::from("ln: failed to create symbolic link '");
    s.push_str(link);
    s.push_str("': File exists");
    s
}

/// GNU coreutils `ls` 逐字报错（漏名后果现场）。
pub fn msg_gnu_ls_cannot(path: &str) -> String {
    let mut s = String::from("ls: cannot access '");
    s.push_str(path);
    s.push_str("': No such file or directory");
    s
}

/// GNU coreutils `install` 逐字报错。
pub fn msg_gnu_install_cannot_stat(path: &str) -> String {
    let mut s = String::from("install: cannot stat '");
    s.push_str(path);
    s.push_str("': No such file or directory");
    s
}

/// busybox 对 `--install` 目标目录不可写的逐字报错。
pub fn msg_bb_install_cannot(dir: &str, code: i32) -> String {
    let mut s = String::from("busybox: can't create ");
    s.push_str(dir);
    s.push_str(": ");
    s.push_str(strerror(code));
    s
}

// ---------------------------------------------------------------------------
// 逐字文案：本卡取证行
// ---------------------------------------------------------------------------

/// 零偏差取证行。
pub fn msg_zero_diff(n: u32) -> String {
    let mut s = String::from("busybox --list: manifest and table agree on ");
    s.push_str(dec_text(n).as_str());
    s.push_str(" names — zero deviation");
    s
}

/// 基数相等但成员不等（本卡最阴的假绿现场）。
pub fn msg_cardinal_equal(n: u32) -> String {
    let mut s = String::from("busybox --list: both sides hold ");
    s.push_str(dec_text(n).as_str());
    s.push_str(" names — cardinality agrees, membership does not");
    s
}

/// 偏差合成取证行。
pub fn msg_deviation(missing: u32, ghost: u32, dup: u32, inv: u32) -> String {
    let mut s = String::from("applet table: missing=");
    s.push_str(dec_text(missing).as_str());
    s.push_str(" ghost=");
    s.push_str(dec_text(ghost).as_str());
    s.push_str(" dup=");
    s.push_str(dec_text(dup).as_str());
    s.push_str(" order=");
    s.push_str(dec_text(inv).as_str());
    s
}

/// 重名取证行。
pub fn msg_dup_name(name: &str) -> String {
    let mut s = String::from("applet table: '");
    s.push_str(name);
    s.push_str("' occupies more than one slot — dedup anchor refused");
    s
}

/// 别名混登取证行。
pub fn msg_alias_as_applet(alt: &str, host: &str) -> String {
    let mut s = String::from("applet table: alias '");
    s.push_str(alt);
    s.push_str("' registered as its own applet while host '");
    s.push_str(host);
    s.push_str("' is already listed");
    s
}

/// 两条数组不等长取证行。
pub fn msg_array_len(name_len: u32, main_len: u32) -> String {
    let mut s = String::from("applet_arr-v.h: applet_name[] has ");
    s.push_str(dec_text(name_len).as_str());
    s.push_str(" entries, applet_main[] has ");
    s.push_str(dec_text(main_len).as_str());
    s
}

/// 空清单取证行。
pub fn msg_empty_manifest() -> String {
    String::from("busybox --list: build manifest is empty — nothing to compare against")
}

/// usage 串孤儿取证行。
pub fn msg_orphan_usage(name: &str) -> String {
    let mut s = String::from("usage text for '");
    s.push_str(name);
    s.push_str("' is present but the table has no entry");
    s
}

/// 序错取证行。
pub fn msg_unsorted(prev: &str, next: &str) -> String {
    let mut s = String::from("applet table not sorted: '");
    s.push_str(prev);
    s.push_str("' is listed before '");
    s.push_str(next);
    s
}

/// 越界下标取证行。
pub fn msg_index_out(name_idx: u32) -> String {
    let mut s = String::from("applet table: slot ");
    s.push_str(dec_text(name_idx).as_str());
    s.push_str(" points outside the roster of ");
    s.push_str(dec_text(UNIVERSE as u32).as_str());
    s
}

/// 落锚取证行。
pub fn msg_anchor_written(cells: u32) -> String {
    let mut s = String::from("dedup anchor written on ");
    s.push_str(dec_text(cells).as_str());
    s.push_str(" cells");
    s
}

/// 未观测取证行（自检没跑）。
pub fn msg_unprobed() -> String {
    String::from("applet table self-check did not run — no observation to report")
}

/// 单格渲染：`<probe>|<stage>|<errno 对>|<dev>|<note>`。
pub fn render_line(
    probe: &str,
    stage: &str,
    code: i32,
    dev: u32,
    note: &str,
) -> String {
    let mut s = String::from(probe);
    s.push('|');
    s.push_str(stage);
    s.push('|');
    s.push_str(errno_pair_text(code).as_str());
    s.push('|');
    s.push_str(deviation_text(dev).as_str());
    s.push('|');
    s.push_str(note);
    s
}
