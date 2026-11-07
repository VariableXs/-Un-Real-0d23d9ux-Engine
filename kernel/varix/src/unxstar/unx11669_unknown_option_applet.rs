//! UNX-F11669 unknown option applet 层报错判据
//!
//! 这张卡管的是**选项 token 本身不认得**这件事。applet 已经定名、已经进 `main`、
//! `getopt_long()` 已经开始逐个吃 argv——然后撞上一个表里没有的开关。基准机
//! （GNU coreutils 8.32 + BusyBox 1.36.1，`LC_ALL=C`）上这类现场长这样：
//!
//! ```text
//! $ ls --zzz
//! ls: unrecognized option '--zzz'
//! Try 'ls --help' or 'ls --usage' for more information.
//! $ ls -alZ
//! ls: invalid option -- 'Z'
//! Try 'ls --help' or 'ls --usage' for more information.
//! $ busybox ls --zzz
//! ls: unrecognized option: --zzz
//! usage: ls [OPTIONS]... [FILE]...
//! ```
//!
//! 三行报错看着同一个意思，实际是**三个互不相容的判据面**：长项走
//! `unrecognized option '<带双横线的整串>'`，短项走 `invalid option -- '<单个字母>'`
//! （注意动词不同、引号里的内容不同、横线在不在里面也不同），busybox 只有一套
//! `unrecognized option: <串>` 且不附 Try 行。把它们混认成一句"unknown option"，
//! 双机回归脚本就会把 GNU 的现场读成 busybox 的现场，码档跟着错位。
//!
//! 更关键的是**这一句报错之前发生了什么**。`getopt` 是**从左到右**扫的，所以
//! `ls -alZ` 的红行里藏着一条几乎没人钉的事实：`-a` 与 `-l` **已经生效**了——
//! 程序状态已经被改过两项，才在第三个字母上出局。本卡把这个数立成独立一维
//! （`value` = 已消费的副作用深度：出错 token 之前已消费的 argv 项数 + cluster 内
//! 出错字母之前的字母数），并且让它**必须**能复算。只报"退 2"而把副作用深度写成 0，
//! 就是把一次有痕迹的失败读成一次干净的失败。
//!
//! 还有两条最容易读反的纵线：
//!
//! * **`--` 终结符**。`ls -- --zzz` 里 `--zzz` 是个**文件名**，一次都不许报错。
//!   把终结符之后的 token 当选项扫，是给一个合法调用凭空安上错误码；反过来，
//!   `ls --help -- --help` 里第二个 `--help` 同样落进操作数堆，**不打印帮助**。
//!   本卡因此把终结符闸排在帮助短路闸**之前**——顺序一倒，`--` 之后的 `--help`
//!   就会被当成成功侧，stdout 上多出一整段 usage。
//! * **唯一前缀缩写不是错**。GNU 的长选项允许缩写：`ls --wid=80` 认 `--width`，
//!   退 0。但 `ls --for` 同时能配 `--format` 与 `--foreground`，glibc 给的是
//!   `option '--for' is ambiguous; possibilities: '--format' '--foreground'`。
//!   这一档**不是** unrecognized——它认得了，只是认得不唯一。本卡把它判成
//!   "红而不落本卡锚"（移交面），因为把它算进 unknown-option 锚，锚就会把
//!   一个"表里有两个候选"的现场读成"表里没有这个开关"的现场。
//!
//! 本卡还管两具邻居尸位，因为它们共用同一条 argv 扫描线、却**不该**落本卡锚：
//! `ls -w` → `ls: option requires an argument -- 'w'`（选项认得了，值没给），
//! 以及 `ls --width`（同理，长项形式文案不同）。这两条的码也是 2，但它们是
//! **F11669 的兄弟档而非本卡锚**：值缺失与值不合法之间只隔一个 token——
//! `ls --width=`（值存在但是空串）已经出本卡范围，归 **F11670**。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 八道闸，秩从 **1** 起算；`Gate` 只接受"前进到下一秩"。前一闸不产出事实，
//! 后一闸就没有输入，倒过来必先产生假绿。
//!
//! 1. `dispatch`（秩 1）——applet 是否已定名且在本卡管辖内。`busybox --zzz ls`
//!    的选项在 **applet 名之前**，那是顶层分发面的事（F11585），退 1 且不进
//!    applet；`foo` 未注册是 F11664/F11585 的 127。本卡只在 applet 已定名之后生效。
//! 2. `dashdash`（秩 2）——`--` 终结符。它必须早于帮助短路：终结符之后的每一个
//!    token（包括长得像 `--help` 的那个）都是操作数，本卡零报错、零输出、退 0。
//! 3. `precedence`（秩 3）——`--help`/`--usage` 短路。只有出现在坏 token **之前**
//!    才短路（成功侧 stdout、退 0，全文逐字归 F11668）；出现在坏 token 之后
//!    就永远读不到，因为扫描已经在前一 token 上死了。
//! 4. `lex`（秩 4）——token 形状逐字节分类：`-`（裸连字符=操作数）、`--`（终结符）、
//!    `-abc`（cluster）、`--name`、`--name=value`、非 `-` 起手（操作数）。申报的
//!    形状与分类器不一致 → `consistent` 压假，这是本卡的 Estate 面。
//! 5. `long`（秩 5）——长选项查表：精确命中 / 唯一前缀（放行）/ 多前缀（歧义档，
//!    码 2、不落锚）/ 零命中（unrecognized，落锚）。
//! 6. `short`（秩 6）——短项位图逐字母扫：全部命中（放行）/ 某字母未命中
//!    （`invalid option -- 'c'`，落锚，副作用深度含前缀字母数）/ 命中但缺值
//!    （`option requires an argument -- 'c'`，不落锚）。
//! 7. `argform`（秩 7）——值的形式：长项 `--opt=`（空值）移交 F11670；
//!    `--help=x` 这类"不许带值"给 `doesn't allow an argument`；`optstring`
//!    以 `:` 起手时 applet 自出文案，本卡只钉码与锚，不钉文案。
//! 8. `emit`（秩 8）——码与通道结算：GNU 用法面 2、busybox 1、成功侧 0；
//!    127 出现即越界（那是 notfound 面的码，本卡判死）。
//!
//! ## 关键差异（本卡深度所在）
//!
//! * **与 F11668（`<applet> --help` 与 applet usage 全文，段基址 `0x6d1f0`，位距
//!   `0x20`）的边界**：F11668 判**成功侧**的 usage 全文逐字（stdout、退 0）；
//!   本卡判**失败侧**的那一行 Try 提示（stderr、随错误码同批）。同一段 usage
//!   文本在两张卡里是两个形状：F11668 要逐字节比全文，本卡只钉"必须存在一行
//!   `Try '<applet> --help' or '<applet> --usage' for more information.`"与
//!   busybox 那一行以 `usage: ` 小写起手。**谁判死谁放行**：本卡对 usage 正文
//!   内容不做判据（放行给 F11668），但对"错误行是否落 stderr、码是否为家族正解"
//!   判死。
//! * **与 F11670（无效参数值，段基址 `0x6d220`，位距 `0x10`，本卡直接后置）的
//!   边界**：分界只有一条——**坏的到底是名字还是值**。`--zzz=anything` 坏在名字
//!   （本卡，且引号里连 `=anything` 一起进文案，整串照抄）；`--width=abc` 名字
//!   认得、值不合法（F11670）。值**缺失**（`--width` 后面没东西）归本卡的邻居档，
//!   值**为空串**（`--width=`）归 F11670。本卡不许用"值面"给自己松绑。
//! * **与 F11671（applet 名大小写与别名，段基址 `0x6d230`）的边界**：那张卡判
//!   `argv[1]` 的**名字**匹配（全小写表、无大小写折叠）；本卡判名字**之后**的
//!   选项 token。`busybox LS --zzz` 的第一句红是 F11671 的 127，本卡在那条现场
//!   上必须先出局（`dispatch` 闸零裁）。
//! * **与 F11678（notfound 的 stderr/stdout 分离，段基址 `0x6d2d0`，位距 `0xc0`）
//!   的边界**：F11678 判**字节数落在哪个 fd**（本卡错误行必须全在 stderr）；
//!   本卡判**行内文案与码**。两卡在同一现场各判一维，互不代替。
//! * **与 F11495（sort/grep 高频三档文案）的联签**：`invalid option -- 'z'` 那一串
//!   由 F11495 逐字钉文案，本卡钉的是**它在哪个 applet 名下、配哪一行 Try、落哪个
//!   码**——动词相同、账不同，任一处漂移都要在本卡的 `message_ok()` 露出来。
//! * **本卡的排他职责**：① 长/短两套动词不得互串（`unrecognized option` 只配带
//!   双横线的整串，`invalid option --` 只配单字母）；② `--` 之后零报错；
//!   ③ 唯一前缀缩写是**放行**、多前缀是**红而不落锚**；④ 副作用深度（cluster 内
//!   已生效字母数 + 之前已消费的 argv 项）独立成维；⑤ GNU=2 / busybox=1 / 127
//!   越界即死；⑥ 带值的未知长项，引号里连 `=` 与值一起照抄。
//! * **引号式样的 locale 依赖**（本卡与 F11694 的分工）：coreutils 的正文行用
//!   `quote()`，引号随 locale（`LC_ALL=C` 是 ASCII `'`，UTF-8 locale 是 `‘’`）；
//!   Try 行是硬编码 ASCII 引号。本卡按 `LC_ALL=C` 快照钉 ASCII 式样，UTF-8 弯引号
//!   的申报在 `Locale::C` 画像下判死；跨工具的 strerror 逐字归 F11694，本卡不碰。
//!
//! 判据六维：结束阶段（gate）、退出码（code）、副作用深度（value）、文案家族档
//! （note）、unknown-option 锚（default）、成败申报（estate）。
//! 判序固定：状态 → 阶段 → 错误码 → 值 → 注解 → 锚位。
//!
//! 语料 14 槽：8 槽申报正确（成功 4、诚实红 3、未触扫描 1），6 槽一槽只坏一维，
//! 依次挂 Estate/Stage/Code/Value/Note/Default。
//! 账本 14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码，冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11669）。
pub const SEG_BASE: u32 = 0x6d210;

/// 交叉署名：F11668「`<applet> --help` 与 applet usage」的段基址（本卡的直接前置）。
pub const ANCHOR_F11668_SEG: u32 = 0x6d1f0;
/// F11668 与本卡的位距（0x6d210 - 0x6d1f0）。
pub const ANCHOR_F11668_GAP: u32 = 0x20;

/// 交叉署名：F11670「无效参数值」的段基址（本卡的直接后置）。
pub const ANCHOR_F11670_SEG: u32 = 0x6d220;
/// F11670 与本卡的位距（0x6d220 - 0x6d210）。
pub const ANCHOR_F11670_GAP: u32 = 0x10;

/// 交叉署名：F11671「applet 名大小写与别名」的段基址（dispatch 闸的移交对象）。
pub const ANCHOR_F11671_SEG: u32 = 0x6d230;
/// F11671 与本卡的位距（0x6d230 - 0x6d210）。
pub const ANCHOR_F11671_GAP: u32 = 0x20;

/// 交叉署名：F11678「stderr/stdout 分离」的段基址（通道面对手卡）。
pub const ANCHOR_F11678_SEG: u32 = 0x6d2d0;
/// F11678 与本卡的位距（0x6d2d0 - 0x6d210）。
pub const ANCHOR_F11678_GAP: u32 = 0xc0;

/// 交叉署名：F11667「busybox --help 退出码 0 与 usage 全文」的段基址。
pub const ANCHOR_F11667_SEG: u32 = 0x6d1e0;
/// F11667 与本卡的位距（0x6d210 - 0x6d1e0）。
pub const ANCHOR_F11667_GAP: u32 = 0x30;

/// 本卡锚定的文案主干：GNU 长项动词串（逐字，不含前缀与引号）。
pub const ANCHOR_LONG_VERB: &str = "unrecognized option";
/// GNU 短项动词串（逐字）。
pub const ANCHOR_SHORT_VERB: &str = "invalid option";
/// busybox 动词串（逐字；注意它后面跟的是冒号，不是空格加引号）。
pub const ANCHOR_BB_VERB: &str = "unrecognized option:";
/// 值缺失档的动词串（GNU 短项形式）。
pub const ANCHOR_NEEDS_ARG: &str = "option requires an argument";
/// 歧义档的动词串（GNU）。
pub const ANCHOR_AMBIGUOUS: &str = "is ambiguous; possibilities:";

/// 语料槽数。
pub const CORPUS: usize = 14;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 申报正确槽数（活格）。
pub const ACCEPT: usize = 8;
/// 一槽只坏一维的槽数（死格）。
pub const REJECT: usize = 6;
/// 档位维数。
pub const TIER_COUNT: usize = 4;
/// 闸门段数（本卡八道闸）。
pub const STAGE_COUNT: u32 = 8;
/// 最后一道闸的秩。
pub const STAGE_LAST_RANK: u32 = 8;
/// 账本统计格数。
pub const STAT_CELLS: usize = 7;
/// 红档率标定值（千分率，注入一槽缺陷后必须正好落在 929）。
pub const RED_RATE: u32 = 929;
/// 全绿千分率。
pub const RATE_FULL: u32 = 1000;

/// 长选项表每 applet 容量。
pub const LONG_CAP: usize = 8;
/// 短选项串每 applet 容量。
pub const SHORT_CAP: usize = 20;
/// applet 表容量（含未管辖位）。
pub const APPLET_COUNT: usize = 7;
/// 参与本卡查表的 applet 数（其余走 dispatch 闸出局）。
pub const APPLET_JUDGED: usize = 5;
/// cluster 内字母上限（超过视为畸形 token，本卡不裁）。
pub const CLUSTER_MAX: usize = 8;
/// 歧义候选串容量（字节）。
pub const AMBIG_CAP: usize = 64;

/// 成功退出码。
pub const EXIT_OK: u32 = 0;
/// busybox 的用法错码（`bb_show_usage` → `xfunc_die`，EXIT_FAILURE）。
pub const EXIT_FILE: u32 = 1;
/// GNU 的命令行调用错码（coreutils 约定：2 = 调用本身有问题）。
pub const EXIT_USAGE: u32 = 2;
/// 未注册 applet 的码——**本卡不许产出**，出现即判死（归 F11664/F11585）。
pub const EXIT_NOT_FOUND: u32 = 127;
/// 退出码档数（0 / 1 / 2 三档加 127 越界档）。
pub const EXIT_RANKS: usize = 4;

/// 值缺失标记（本卡用它区分"名字坏"与"值坏"）。
pub const VALUE_ABSENT: i8 = -1;
/// 空值标记（`--opt=`，移交 F11670）。
pub const VALUE_EMPTY: i8 = 0;

// ---------------------------------------------------------------------------
// 错误码：13 个，前缀 UO，值 SEG_BASE + 0x01 .. + 0x0d
// ---------------------------------------------------------------------------

/// UO+0x01 状态违例：账本已冻结还动闸、关闸后再走。
pub const UO_ESTATE: u32 = SEG_BASE + 0x01;
/// UO+0x02 错误码维缺陷：申报的退出码与家族码档表对不上。
pub const UO_ECODE: u32 = SEG_BASE + 0x02;
/// UO+0x03 值维缺陷：副作用深度（已消费项数）报错。
pub const UO_EVALUE: u32 = SEG_BASE + 0x03;
/// UO+0x04 注解维缺陷：文案家族档张冠李戴（长短动词互串等）。
pub const UO_ENOTE: u32 = SEG_BASE + 0x04;
/// UO+0x05 锚位维缺陷：unknown-option 锚该亮不亮、不该亮偏亮。
pub const UO_EDEFAULT: u32 = SEG_BASE + 0x05;
/// UO+0x06 选项表画像不合法：applet 位图与长项表自相矛盾。
pub const UO_EOPTAB: u32 = SEG_BASE + 0x06;
/// UO+0x07 闸序倒退：阶段闸门回退。
pub const UO_ESTAGE: u32 = SEG_BASE + 0x07;
/// UO+0x08 计数错：段数/槽数/候选数与容量不符。
pub const UO_ECOUNT: u32 = SEG_BASE + 0x08;
/// UO+0x09 槽位越界：账本槽位跳号或超出容量。
pub const UO_ESLOT: u32 = SEG_BASE + 0x09;
/// UO+0x0a 指纹不符：重放与冻结时不同。
pub const UO_ESEAL: u32 = SEG_BASE + 0x0a;
/// UO+0x0b 观测缺失：语料/期望表取不到格。
pub const UO_ENOBS: u32 = SEG_BASE + 0x0b;
/// UO+0x0c 矩阵不闭合：四档合计不等于语料格数。
pub const UO_EMATRIX: u32 = SEG_BASE + 0x0c;
/// UO+0x0d 账不平：账本格数、成败配比或审计不通过。
pub const UO_EBAL: u32 = SEG_BASE + 0x0d;

/// 13 个错误码按序（自检逐条取用）。
pub const ALL_CODES: [u32; 13] = [
    UO_ESTATE,
    UO_ECODE,
    UO_EVALUE,
    UO_ENOTE,
    UO_EDEFAULT,
    UO_EOPTAB,
    UO_ESTAGE,
    UO_ECOUNT,
    UO_ESLOT,
    UO_ESEAL,
    UO_ENOBS,
    UO_EMATRIX,
    UO_EBAL,
];

// ---------------------------------------------------------------------------
// 账本操作码（本卡自己的扫描阶段名，逐段入账）
// ---------------------------------------------------------------------------

/// 操作码：dispatch 闸计数。
pub const OP_DISPATCH: u8 = 0x01;
/// 操作码：`--` 终结符计数。
pub const OP_DASHDASH: u8 = 0x02;
/// 操作码：帮助短路计数。
pub const OP_PRECEDENCE: u8 = 0x03;
/// 操作码：token 形状分类计数。
pub const OP_LEX: u8 = 0x04;
/// 操作码：长项查表计数。
pub const OP_LONG: u8 = 0x05;
/// 操作码：短项位图计数。
pub const OP_SHORT: u8 = 0x06;
/// 操作码：值形式计数。
pub const OP_ARGFORM: u8 = 0x07;
/// 操作码：语料总数。
pub const OP_CORPUS: u8 = 0x08;
/// 操作码：红档率。
pub const OP_RATE: u8 = 0x09;
/// 操作码：缺陷槽。
pub const OP_DEFECT: u8 = 0x0a;
/// 操作码：重放。
pub const OP_REPLAY: u8 = 0x0b;
/// 操作码总数。
pub const OP_COUNT: usize = 11;

// ---------------------------------------------------------------------------
// FNV-1a 指纹
// ---------------------------------------------------------------------------

/// FNV-1a 初始偏移量。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折一个字节进哈希。
fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// 折一个字（u64，按小端逐字节）进哈希。
fn fold_word(h: u64, w: u64) -> u64 {
    let bytes = w.to_le_bytes();
    let mut v = h;
    let mut i = 0usize;
    while i < 8 {
        if let Some(b) = bytes.get(i) {
            v = fold_byte(v, *b);
        }
        i += 1;
    }
    v
}

/// 折一段文本进哈希。
fn fold_str(h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut v = fold_word(h, bytes.len() as u64);
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            v = fold_byte(v, *b);
        }
        i += 1;
    }
    v
}

/// 折一个布尔进哈希。
fn fold_bool(h: u64, b: bool) -> u64 {
    fold_word(h, if b { 1u64 } else { 0u64 })
}

/// 折一个有符号整数进哈希（符号与绝对值分两拍，避免补码混叠）。
fn fold_i32(h: u64, v: i32) -> u64 {
    let neg = if v < 0 { 1u64 } else { 0u64 };
    let mag = if v < 0 {
        (v.unsigned_abs()) as u64
    } else {
        v as u64
    };
    let w = fold_word(h, neg);
    fold_word(w, mag)
}

// ---------------------------------------------------------------------------
// 数字与布尔文本（不递归、不分配额外容器）
// ---------------------------------------------------------------------------

/// u32 → 十进制文本（不递归）。
fn dec_text(mut v: u32) -> String {
    let mut buf = [0u8; 10];
    let mut n = 10usize;
    loop {
        let d = (v % 10) as u8;
        n = n.saturating_sub(1);
        if let Some(slot) = buf.get_mut(n) {
            *slot = b'0' + d;
        }
        v = v / 10;
        if v == 0 {
            break;
        }
    }
    let mut s = String::new();
    let mut i = n;
    while i < 10 {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

/// u32 → 十六进制文本（固定宽度，小写，高位起）。
fn hex_text(v: u32, width: usize) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    let mut i = width;
    while i > 0 {
        i = i.saturating_sub(1);
        let shift = (i * 4) as u32;
        if shift >= 32 {
            s.push('0');
            continue;
        }
        let nib = (v >> shift) & 0x0f;
        if let Some(c) = DIGITS.get(nib as usize) {
            s.push(*c as char);
        }
    }
    s
}

/// 段址文本：`0x` + 六位十六进制。
pub fn seg_text(v: u32) -> String {
    let mut s = String::from("0x");
    s.push_str(hex_text(v, 6).as_str());
    s
}

/// u32 文本对外口。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// i32 文本（带负号）。
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

/// ASCII 单引号包裹（`LC_ALL=C` 的 `quote()` 式样）。
fn ascii_quote(s: &str) -> String {
    let mut out = String::from("'");
    out.push_str(s);
    out.push('\'');
    out
}

/// UTF-8 弯引号包裹（locale 为 UTF-8 时 coreutils 的 `quote()` 式样）。
fn utf8_quote(s: &str) -> String {
    let mut out = String::from("\u{2018}");
    out.push_str(s);
    out.push_str("\u{2019}");
    out
}

// ---------------------------------------------------------------------------
// 域模型枚举：applet / 家族 / locale / token 形状 / 阶段 / 注解 / 档位 / 缺陷
// ---------------------------------------------------------------------------

/// 本卡认识的 applet（选项表按 applet 分家，名字逐字用 argv 里那一个）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Applet {
    /// `ls`——长项表里有 `--format`/`--foreground` 两个 `f` 头，歧义档的原型现场。
    Ls,
    /// `cp`——长项 `--preserve`/`--recursive`，值面归 F11670。
    Cp,
    /// `head`——`-c`/`-n` 都带值，缺值档的正例来自这里。
    Head,
    /// `tail`——`-f` 可带可不带，`--follow=name` 的等号值归 F11670。
    Tail,
    /// `wc`——短项全不带值，用来钉"cluster 全命中"的放行侧。
    Wc,
    /// `echo`——shell 内建与 coreutils 两份文案不同，逐字归 F11495，本卡出局。
    Echo,
    /// 未注册/别的工具——dispatch 闸零裁（归 F11585/F11664）。
    Foreign,
}

impl Applet {
    /// argv 里的那一个名字（错误行的前缀）。
    pub fn name(self) -> &'static str {
        match self {
            Applet::Ls => "ls",
            Applet::Cp => "cp",
            Applet::Head => "head",
            Applet::Tail => "tail",
            Applet::Wc => "wc",
            Applet::Echo => "echo",
            Applet::Foreign => "notmyapplet",
        }
    }

    /// 序号（查表用）。
    pub const fn index(self) -> usize {
        match self {
            Applet::Ls => 0usize,
            Applet::Cp => 1usize,
            Applet::Head => 2usize,
            Applet::Tail => 3usize,
            Applet::Wc => 4usize,
            Applet::Echo => 5usize,
            Applet::Foreign => 6usize,
        }
    }

    /// 按序号取 applet。
    pub fn at(i: usize) -> Option<Applet> {
        match i {
            0usize => Some(Applet::Ls),
            1usize => Some(Applet::Cp),
            2usize => Some(Applet::Head),
            3usize => Some(Applet::Tail),
            4usize => Some(Applet::Wc),
            5usize => Some(Applet::Echo),
            6usize => Some(Applet::Foreign),
            _ => None,
        }
    }

    /// 全集（顺序=序号）。
    pub fn all() -> [Applet; APPLET_COUNT] {
        [
            Applet::Ls,
            Applet::Cp,
            Applet::Head,
            Applet::Tail,
            Applet::Wc,
            Applet::Echo,
            Applet::Foreign,
        ]
    }

    /// 是否进本卡查表面（`echo` 与未注册名都不进）。
    pub fn judged(self) -> bool {
        matches!(
            self,
            Applet::Ls | Applet::Cp | Applet::Head | Applet::Tail | Applet::Wc
        )
    }
}

/// 文案家族。`Mixed` 是"申报者自己也说不清这台机器是 GNU 还是 busybox"的画像，
/// 本卡对它不产出任何文案——拿到空串就是判死的信号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flavor {
    /// GNU coreutils + glibc getopt_long。
    Gnu,
    /// BusyBox 1.36 applet。
    Busybox,
    /// 混认：不配任何动词。
    Mixed,
}

impl Flavor {
    /// 家族名。
    pub fn name(self) -> &'static str {
        match self {
            Flavor::Gnu => "gnu",
            Flavor::Busybox => "busybox",
            Flavor::Mixed => "mixed",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Flavor::Gnu => 0usize,
            Flavor::Busybox => 1usize,
            Flavor::Mixed => 2usize,
        }
    }

    /// 用法错的退出码：GNU 2、busybox 1、混认按 GNU 落码但整格判死。
    pub const fn usage_exit(self) -> i32 {
        match self {
            Flavor::Gnu => 2i32,
            Flavor::Busybox => 1i32,
            Flavor::Mixed => 2i32,
        }
    }

    /// 是否出 Try 行（GNU 出，busybox 出 `usage:` 行，混认什么都不出）。
    pub fn emits_try(self) -> bool {
        matches!(self, Flavor::Gnu)
    }

    /// 是否出 busybox 的小写 usage 行。
    pub fn emits_bb_usage(self) -> bool {
        matches!(self, Flavor::Busybox)
    }

    /// 能否产出文案（混认不能）。
    pub fn speakable(self) -> bool {
        !matches!(self, Flavor::Mixed)
    }
}

/// 引号式样的 locale 画像。coreutils 正文行走 `quote()`，随 locale 变；
/// Try 行是硬编码 ASCII。本卡基准快照是 `LC_ALL=C`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Locale {
    /// `LC_ALL=C` / POSIX：ASCII 单引号。
    C,
    /// UTF-8 locale：弯引号。
    Utf8,
    /// 申报者没读 locale——本卡按 `C` 复算，申报弯引号即错位。
    Unread,
}

impl Locale {
    /// 名字。
    pub fn name(self) -> &'static str {
        match self {
            Locale::C => "C",
            Locale::Utf8 => "UTF-8",
            Locale::Unread => "unread",
        }
    }

    /// 按画像包一层引号。
    pub fn quote(self, s: &str) -> String {
        match self {
            Locale::Utf8 => utf8_quote(s),
            Locale::C | Locale::Unread => ascii_quote(s),
        }
    }
}

/// token 形状（申报维之一：与逐字节分类器对账，不一致就压 `consistent`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokKind {
    /// 不以 `-` 起手（含空串）：操作数。
    PlainOperand,
    /// 恰为一个 `-`：stdin 惯用法，仍是操作数。
    BareHyphen,
    /// 恰为 `--`：选项区终结符。
    Terminator,
    /// `-abc`：短项 cluster（`-` 后至少一个字符，且第二个字符不是 `-`）。
    ShortCluster,
    /// `--name`：长项，不带值。
    LongBare,
    /// `--name=value`：长项，带内联值。
    LongAssigned,
    /// 逐字节读不下去（超长、含 NUL 形状等）。
    Malformed,
}

impl TokKind {
    /// 名字。
    pub fn name(self) -> &'static str {
        match self {
            TokKind::PlainOperand => "operand",
            TokKind::BareHyphen => "bare-hyphen",
            TokKind::Terminator => "terminator",
            TokKind::ShortCluster => "short-cluster",
            TokKind::LongBare => "long-bare",
            TokKind::LongAssigned => "long-assigned",
            TokKind::Malformed => "malformed",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            TokKind::PlainOperand => 0usize,
            TokKind::BareHyphen => 1usize,
            TokKind::Terminator => 2usize,
            TokKind::ShortCluster => 3usize,
            TokKind::LongBare => 4usize,
            TokKind::LongAssigned => 5usize,
            TokKind::Malformed => 6usize,
        }
    }

    /// 全集。
    pub fn all() -> [TokKind; 7] {
        [
            TokKind::PlainOperand,
            TokKind::BareHyphen,
            TokKind::Terminator,
            TokKind::ShortCluster,
            TokKind::LongBare,
            TokKind::LongAssigned,
            TokKind::Malformed,
        ]
    }

    /// 会不会进选项表（操作数与终结符不会）。
    pub fn reaches_table(self) -> bool {
        matches!(
            self,
            TokKind::ShortCluster | TokKind::LongBare | TokKind::LongAssigned | TokKind::Malformed
        )
    }
}

/// 阶段：本卡的八道闸，秩从 1 起。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 闸 1：applet 定名与管辖。
    Dispatch,
    /// 闸 2：`--` 终结符。
    DashDash,
    /// 闸 3：`--help`/`--usage` 短路。
    Precedence,
    /// 闸 4：token 形状分类。
    Lex,
    /// 闸 5：长项查表。
    Long,
    /// 闸 6：短项位图。
    Short,
    /// 闸 7：值的形式。
    ArgForm,
    /// 闸 8：码与通道结算。
    Emit,
}

impl Stage {
    /// 秩（1..8）。
    pub const fn rank(self) -> u32 {
        match self {
            Stage::Dispatch => 1u32,
            Stage::DashDash => 2u32,
            Stage::Precedence => 3u32,
            Stage::Lex => 4u32,
            Stage::Long => 5u32,
            Stage::Short => 6u32,
            Stage::ArgForm => 7u32,
            Stage::Emit => 8u32,
        }
    }

    /// 小写阶段名（明细行与矩阵用）。
    pub fn name(self) -> &'static str {
        match self {
            Stage::Dispatch => "dispatch",
            Stage::DashDash => "dashdash",
            Stage::Precedence => "precedence",
            Stage::Lex => "lex",
            Stage::Long => "long",
            Stage::Short => "short",
            Stage::ArgForm => "argform",
            Stage::Emit => "emit",
        }
    }

    /// 按秩取阶段；越界给 `None`。
    pub fn from_rank(r: u32) -> Option<Stage> {
        match r {
            1u32 => Some(Stage::Dispatch),
            2u32 => Some(Stage::DashDash),
            3u32 => Some(Stage::Precedence),
            4u32 => Some(Stage::Lex),
            5u32 => Some(Stage::Long),
            6u32 => Some(Stage::Short),
            7u32 => Some(Stage::ArgForm),
            8u32 => Some(Stage::Emit),
            _ => None,
        }
    }

    /// 全集（按闸序）。
    pub fn all() -> [Stage; 8] {
        [
            Stage::Dispatch,
            Stage::DashDash,
            Stage::Precedence,
            Stage::Lex,
            Stage::Long,
            Stage::Short,
            Stage::ArgForm,
            Stage::Emit,
        ]
    }

    /// 这一闸的红是不是"选项名本身坏"（决定本卡锚）。
    pub fn names_bad_option(self) -> bool {
        matches!(self, Stage::Long | Stage::Short)
    }

    /// 这一闸是否根本还没碰选项表。
    pub fn before_table(self) -> bool {
        matches!(self, Stage::Dispatch | Stage::DashDash | Stage::Precedence)
    }
}

/// 注解：文案家族档。一维只装"这一行该长成什么样"。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// `ls: unrecognized option '--zzz'`（GNU 长项）。
    GnuUnrecLong,
    /// `ls: invalid option -- 'Z'`（GNU 短项）。
    GnuBadShort,
    /// `ls: option '--for' is ambiguous; possibilities: ...`（GNU 歧义，红而不落锚）。
    GnuAmbiguous,
    /// `ls: option '--width' requires an argument`（GNU 长项缺值，邻居档）。
    GnuLongNeedArg,
    /// `ls: option requires an argument -- 'w'`（GNU 短项缺值，邻居档）。
    GnuShortNeedArg,
    /// `ls: option '--all' doesn't allow an argument`（GNU 不许带值，邻居档）。
    GnuNoArgAllowed,
    /// `ls: unrecognized option: --zzz`（busybox 长项）。
    BbUnrecLong,
    /// `ls: unrecognized option: Z`（busybox 短项）。
    BbUnrecShort,
    /// busybox 缺值/歧义统一走 usage 面（本卡只钉 usage 行 + 码 1）。
    BbUsageDie,
    /// `optstring` 以 `:` 起手：applet 自出文案，本卡不钉正文。
    OwnMessage,
    /// 长项精确命中（放行，无文案）。
    LongExactOk,
    /// 长项唯一前缀命中（放行，无文案）。
    LongAbbrevOk,
    /// 短项 cluster 全命中（放行，无文案）。
    ShortClusterOk,
    /// `--help`/`--usage` 短路（成功侧全文归 F11668，本卡只钉码 0）。
    HelpShortCircuit,
    /// `--` 之后的 token：操作数，零报错。
    OperandAfterDD,
    /// 不以 `-` 起手：操作数。
    PlainOperand,
    /// 裸 `-`：stdin 惯用法，按操作数读。
    BareHyphenStdin,
    /// token 本身就是 `--`：被吃掉的终结符。
    TerminatorToken,
    /// `--width=`：值存在但为空，移交 F11670。
    HandoffEmptyValue,
    /// applet 不在本卡管辖：移交 F11585/F11664/F11495。
    NotOurApplet,
    /// 家族混认：不配任何动词，整格判死。
    VerbMismatch,
}

impl Note {
    /// 档名（明细行里逐字出现）。
    pub fn name(self) -> &'static str {
        match self {
            Note::GnuUnrecLong => "gnu-unrec-long",
            Note::GnuBadShort => "gnu-invalid-short",
            Note::GnuAmbiguous => "gnu-ambiguous",
            Note::GnuLongNeedArg => "gnu-long-needs-arg",
            Note::GnuShortNeedArg => "gnu-short-needs-arg",
            Note::GnuNoArgAllowed => "gnu-no-arg-allowed",
            Note::BbUnrecLong => "bb-unrec-long",
            Note::BbUnrecShort => "bb-unrec-short",
            Note::BbUsageDie => "bb-usage-die",
            Note::OwnMessage => "own-message",
            Note::LongExactOk => "long-exact-ok",
            Note::LongAbbrevOk => "long-abbrev-ok",
            Note::ShortClusterOk => "cluster-ok",
            Note::HelpShortCircuit => "help-shortcircuit",
            Note::OperandAfterDD => "operand-after-dd",
            Note::PlainOperand => "plain-operand",
            Note::BareHyphenStdin => "bare-hyphen-stdin",
            Note::TerminatorToken => "terminator-token",
            Note::HandoffEmptyValue => "handoff-empty-value",
            Note::NotOurApplet => "not-our-applet",
            Note::VerbMismatch => "verb-mismatch",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Note::GnuUnrecLong => 0usize,
            Note::GnuBadShort => 1usize,
            Note::GnuAmbiguous => 2usize,
            Note::GnuLongNeedArg => 3usize,
            Note::GnuShortNeedArg => 4usize,
            Note::GnuNoArgAllowed => 5usize,
            Note::BbUnrecLong => 6usize,
            Note::BbUnrecShort => 7usize,
            Note::BbUsageDie => 8usize,
            Note::OwnMessage => 9usize,
            Note::LongExactOk => 10usize,
            Note::LongAbbrevOk => 11usize,
            Note::ShortClusterOk => 12usize,
            Note::HelpShortCircuit => 13usize,
            Note::OperandAfterDD => 14usize,
            Note::PlainOperand => 15usize,
            Note::BareHyphenStdin => 16usize,
            Note::TerminatorToken => 17usize,
            Note::HandoffEmptyValue => 18usize,
            Note::NotOurApplet => 19usize,
            Note::VerbMismatch => 20usize,
        }
    }

    /// 全集。
    pub fn all() -> [Note; 21] {
        [
            Note::GnuUnrecLong,
            Note::GnuBadShort,
            Note::GnuAmbiguous,
            Note::GnuLongNeedArg,
            Note::GnuShortNeedArg,
            Note::GnuNoArgAllowed,
            Note::BbUnrecLong,
            Note::BbUnrecShort,
            Note::BbUsageDie,
            Note::OwnMessage,
            Note::LongExactOk,
            Note::LongAbbrevOk,
            Note::ShortClusterOk,
            Note::HelpShortCircuit,
            Note::OperandAfterDD,
            Note::PlainOperand,
            Note::BareHyphenStdin,
            Note::TerminatorToken,
            Note::HandoffEmptyValue,
            Note::NotOurApplet,
            Note::VerbMismatch,
        ]
    }

    /// 注解总数。
    pub const COUNT: usize = 21;

    /// 是不是"选项名本身不认得"（本卡锚的唯一来源）。
    pub fn is_unknown_option(self) -> bool {
        matches!(
            self,
            Note::GnuUnrecLong | Note::GnuBadShort | Note::BbUnrecLong | Note::BbUnrecShort
        )
    }

    /// 会不会打出错误行（红档面）。
    pub fn speaks_error(self) -> bool {
        matches!(
            self,
            Note::GnuUnrecLong
                | Note::GnuBadShort
                | Note::GnuAmbiguous
                | Note::GnuLongNeedArg
                | Note::GnuShortNeedArg
                | Note::GnuNoArgAllowed
                | Note::BbUnrecLong
                | Note::BbUnrecShort
                | Note::BbUsageDie
                | Note::OwnMessage
                | Note::VerbMismatch
        )
    }

    /// 是否落进操作数/移交面（未触选项判定）。
    pub fn hands_off(self) -> bool {
        matches!(
            self,
            Note::OperandAfterDD
                | Note::PlainOperand
                | Note::BareHyphenStdin
                | Note::TerminatorToken
                | Note::HandoffEmptyValue
                | Note::NotOurApplet
        )
    }

    /// 是否走进过选项表并放行（成功档）。
    pub fn scanned_ok(self) -> bool {
        matches!(
            self,
            Note::LongExactOk | Note::LongAbbrevOk | Note::ShortClusterOk | Note::HelpShortCircuit
        )
    }

    /// 是否算"进过扫描"（分 Clean 与 NotTouched 用）。
    pub fn touched(self) -> bool {
        self.scanned_ok() || self.speaks_error()
    }

    /// 是不是邻居档（红但不落本卡锚）。
    pub fn neighbour_risk(self) -> bool {
        matches!(
            self,
            Note::GnuAmbiguous | Note::GnuLongNeedArg | Note::GnuShortNeedArg | Note::GnuNoArgAllowed
        )
    }

    /// 该不该附 GNU 的 Try 行。
    pub fn wants_try_line(self) -> bool {
        matches!(
            self,
            Note::GnuUnrecLong
                | Note::GnuBadShort
                | Note::GnuAmbiguous
                | Note::GnuLongNeedArg
                | Note::GnuShortNeedArg
                | Note::GnuNoArgAllowed
        )
    }

    /// 该不该附 busybox 的 `usage:` 行。
    pub fn wants_bb_usage(self) -> bool {
        matches!(self, Note::BbUnrecLong | Note::BbUnrecShort | Note::BbUsageDie)
    }

    /// 文案是否由 applet 自出（本卡不钉正文）。
    pub fn own_text(self) -> bool {
        matches!(self, Note::OwnMessage | Note::VerbMismatch)
    }
}

/// 四档：按**实际结论**归档（不是按申报）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 一档：走进扫描并放行（成功侧，码 0）。
    Clean,
    /// 二档：诚实红，且落了本卡 unknown-option 锚。
    HonestFail,
    /// 三档：压根没碰选项表（操作数、`--` 之后、移交）。
    NotTouched,
    /// 四档：红了但不落本卡锚（邻居档、混认、谎报）。
    Refused,
}

impl Tier {
    /// 档名。
    pub fn name(self) -> &'static str {
        match self {
            Tier::Clean => "clean",
            Tier::HonestFail => "honest",
            Tier::NotTouched => "untouched",
            Tier::Refused => "refused",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Tier::Clean => 0usize,
            Tier::HonestFail => 1usize,
            Tier::NotTouched => 2usize,
            Tier::Refused => 3usize,
        }
    }

    /// 按实际结论归档；四档互斥且全覆盖。
    pub fn of(o: &Outcome) -> Tier {
        if !o.ok() {
            if o.default {
                return Tier::HonestFail;
            }
            return Tier::Refused;
        }
        if o.note.touched() {
            Tier::Clean
        } else {
            Tier::NotTouched
        }
    }
}

/// 缺陷维：判序 Estate → Stage → Code → Value → Note → Default → Ok。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 全对。
    Ok,
    /// 成败互报。
    Estate,
    /// 落点闸错。
    Stage,
    /// 退出码错。
    Code,
    /// 副作用深度错。
    Value,
    /// 文案家族档错。
    Note,
    /// 锚位错。
    Default,
}

impl Fault {
    /// 本维错误码（`Ok` 给 0）。
    pub const fn code(self) -> u32 {
        match self {
            Fault::Ok => 0u32,
            Fault::Estate => UO_ESTATE,
            Fault::Stage => UO_ESTAGE,
            Fault::Code => UO_ECODE,
            Fault::Value => UO_EVALUE,
            Fault::Note => UO_ENOTE,
            Fault::Default => UO_EDEFAULT,
        }
    }

    /// 维名。
    pub fn name(self) -> &'static str {
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

    /// 序号（判序位次，`Ok` 排最后）。
    pub const fn order(self) -> usize {
        match self {
            Fault::Estate => 0usize,
            Fault::Stage => 1usize,
            Fault::Code => 2usize,
            Fault::Value => 3usize,
            Fault::Note => 4usize,
            Fault::Default => 5usize,
            Fault::Ok => 6usize,
        }
    }
}

/// 短名（明细行里的缺陷标注）。
pub fn fault_short(f: Fault) -> &'static str {
    match f {
        Fault::Ok => "OK",
        Fault::Estate => "ESTATE",
        Fault::Stage => "ESTAGE",
        Fault::Code => "ECODE",
        Fault::Value => "EVALUE",
        Fault::Note => "ENOTE",
        Fault::Default => "EDEFAUL",
    }
}

/// 判序全集（六维顺序固定，不许换）。
pub const JUDGE_ORDER: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

// ---------------------------------------------------------------------------
// 选项表：每 applet 一份长项表 + 位图（本卡查表的唯一依据）
// ---------------------------------------------------------------------------

/// 一个 applet 的选项画像。长项表按字典序落位（歧义候选串就按这个序呈现），
/// `long_val` 是位图：第 i 位为 1 表示第 i 个长项**需要**值。
#[derive(Clone, Copy)]
pub struct OptSpec {
    /// 所属 applet。
    pub applet: Applet,
    /// 长项名（不含 `--`；空串表示该位未用）。
    pub longs: [&'static str; LONG_CAP],
    /// 需要值的长项位图。
    pub long_val: u8,
    /// 认识的短项字母（无序串，逐字节判命中）。
    pub shorts: &'static str,
    /// 需要值的短项字母子集。
    pub short_val: &'static str,
    /// 是否支持 `--usage`（coreutils 有，busybox 无）。
    pub has_usage: bool,
}

/// 基准表（B24 快照裁剪：只留本卡判据用得上的项，顺序即歧义候选呈现顺序）。
pub const OPT_TABLE: [OptSpec; APPLET_COUNT] = [
    OptSpec {
        applet: Applet::Ls,
        longs: ["all", "color", "format", "foreground", "help", "sort", "usage", "width"],
        long_val: 0xA6u8,
        shorts: "abcCfgHhilmMnNqRsSTuvwx1",
        short_val: "Tw",
        has_usage: true,
    },
    OptSpec {
        applet: Applet::Cp,
        longs: [
            "archive",
            "backup",
            "force",
            "help",
            "preserve",
            "recursive",
            "usage",
            "verbose",
        ],
        long_val: 0x12u8,
        shorts: "abfFijklLmnNprRsStuvVw",
        short_val: "bSt",
        has_usage: true,
    },
    OptSpec {
        applet: Applet::Head,
        longs: [
            "bytes",
            "help",
            "lines",
            "quiet",
            "repetitions",
            "silent",
            "usage",
            "verbose",
        ],
        long_val: 0x15u8,
        shorts: "bcCfLnqsvz",
        short_val: "cn",
        has_usage: true,
    },
    OptSpec {
        applet: Applet::Tail,
        longs: [
            "bytes",
            "follow",
            "help",
            "lines",
            "pid",
            "retry",
            "sleep",
            "usage",
        ],
        long_val: 0x57u8,
        shorts: "bcFqRsuvz",
        short_val: "bcRs",
        has_usage: true,
    },
    OptSpec {
        applet: Applet::Wc,
        longs: [
            "bytes",
            "chars",
            "help",
            "lines",
            "quiet",
            "total",
            "usage",
            "words",
        ],
        long_val: 0x00u8,
        shorts: "bcLlmqwz",
        short_val: "",
        has_usage: true,
    },
    OptSpec {
        applet: Applet::Echo,
        longs: ["", "", "", "", "", "", "", ""],
        long_val: 0x00u8,
        shorts: "neE",
        short_val: "",
        has_usage: false,
    },
    OptSpec {
        applet: Applet::Foreign,
        longs: ["", "", "", "", "", "", "", ""],
        long_val: 0x00u8,
        shorts: "",
        short_val: "",
        has_usage: false,
    },
];

// ---------------------------------------------------------------------------
// 查表基元：形状分类、长项匹配、短项 cluster 扫描
// ---------------------------------------------------------------------------

/// 空长项表（越界时的兜底画像）。
const NO_LONGS: [&'static str; LONG_CAP] = ["", "", "", "", "", "", "", ""];

/// 兜底 applet 画像：什么都不认识。
pub const SPEC_NONE: OptSpec = OptSpec {
    applet: Applet::Foreign,
    longs: NO_LONGS,
    long_val: 0u8,
    shorts: "",
    short_val: "",
    has_usage: false,
};

/// 取 applet 的选项画像。
pub fn spec(a: Applet) -> OptSpec {
    match OPT_TABLE.get(a.index()) {
        Some(s) => *s,
        None => SPEC_NONE,
    }
}

/// `hay` 是否以 `needle` 开头（逐字节，空 needle 不算命中）。
pub fn starts_with(hay: &str, needle: &str) -> bool {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || n.len() > h.len() {
        return false;
    }
    let mut i = 0usize;
    while i < n.len() {
        let (Some(a), Some(b)) = (h.get(i), n.get(i)) else {
            return false;
        };
        if a != b {
            return false;
        }
        i += 1;
    }
    true
}

/// 单个字符是否在字母串里。
pub fn letter_has(set: &str, c: char) -> bool {
    let mut found = false;
    for x in set.chars() {
        if x == c {
            found = true;
        }
    }
    found
}

/// 第 i 个长项是否需要值。
pub fn long_needs_value(a: Applet, i: usize) -> bool {
    if i >= LONG_CAP {
        return false;
    }
    let mask = spec(a).long_val;
    let bit = 1u8 << i;
    mask & bit != 0
}

/// 长项匹配结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LongHit {
    /// 表里没有任何候选。
    None,
    /// 精确命中第 i 项。
    Exact(usize),
    /// 唯一前缀命中第 i 项（GNU 允许缩写，这是**放行**）。
    Unique(usize),
    /// 前缀命中 n 项（歧义，红而不落本卡锚）。
    Ambiguous(usize),
    /// 名字读不出来（`--=v` 这类），仍按整串走 unrecognized。
    Malformed,
}

/// 前缀命中的下标集合与个数。
pub fn prefix_hits(a: Applet, name: &str) -> ([usize; LONG_CAP], usize) {
    let sp = spec(a);
    let mut hits = [usize::MAX; LONG_CAP];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < LONG_CAP {
        let Some(l) = sp.longs.get(i) else {
            i += 1;
            continue;
        };
        if !l.is_empty() && starts_with(l, name) {
            if let Some(slot) = hits.get_mut(n) {
                *slot = i;
            }
            n = n.saturating_add(1);
        }
        i += 1;
    }
    (hits, n)
}

/// 长项查表（两遍扫：先精确、再前缀，与 glibc 同序）。
pub fn long_lookup(a: Applet, name: &str) -> LongHit {
    if name.is_empty() || name.len() > 40 {
        return LongHit::Malformed;
    }
    let sp = spec(a);
    let mut i = 0usize;
    while i < LONG_CAP {
        let Some(l) = sp.longs.get(i) else {
            i += 1;
            continue;
        };
        if !l.is_empty() && *l == name {
            return LongHit::Exact(i);
        }
        i += 1;
    }
    let (hits, n) = prefix_hits(a, name);
    if n == 0 {
        return LongHit::None;
    }
    if n == 1 {
        let first = match hits.get(0) {
            Some(v) => *v,
            None => return LongHit::None,
        };
        return LongHit::Unique(first);
    }
    LongHit::Ambiguous(n)
}

/// cluster 扫描结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cluster {
    /// 全部字母都认得（值也拿得到）。
    AllKnown,
    /// 第 `pos` 个字母不认得（前 `pos` 个已经生效）。
    BadAt {
        /// 出错字母在 cluster 内的位置。
        pos: usize,
        /// 出错字母。
        letter: char,
    },
    /// 第 `pos` 个字母认得但要值，cluster 到头且后面没 token 了。
    NeedsArg {
        /// 缺值字母的位置。
        pos: usize,
        /// 缺值字母。
        letter: char,
    },
}

/// 逐字母扫 cluster。`value_follows` 表示 argv 里后面还有 token 可当值。
pub fn scan_cluster(a: Applet, letters: &str, value_follows: bool) -> Cluster {
    let sp = spec(a);
    let bytes: [char; CLUSTER_MAX] = {
        let mut buf = ['\0'; CLUSTER_MAX];
        let mut k = 0usize;
        for c in letters.chars() {
            if k >= CLUSTER_MAX {
                break;
            }
            if let Some(slot) = buf.get_mut(k) {
                *slot = c;
            }
            k += 1;
        }
        buf
    };
    let total = letters.chars().count();
    let mut i = 0usize;
    while i < total {
        let Some(c) = bytes.get(i) else {
            return Cluster::AllKnown;
        };
        if !letter_has(sp.shorts, *c) {
            return Cluster::BadAt {
                pos: i,
                letter: *c,
            };
        }
        if letter_has(sp.short_val, *c) {
            if i + 1 < total {
                // 同一个 token 内剩余部分就是它的值：本卡按"已受理"读。
                return Cluster::AllKnown;
            }
            if !value_follows {
                return Cluster::NeedsArg {
                    pos: i,
                    letter: *c,
                };
            }
            return Cluster::AllKnown;
        }
        i += 1;
    }
    Cluster::AllKnown
}

/// 逐字节把 token 分类成形状。
pub fn classify(tok: &str) -> TokKind {
    let b = tok.as_bytes();
    if b.is_empty() {
        return TokKind::PlainOperand;
    }
    if b.get(0) != Some(&b'-') {
        return TokKind::PlainOperand;
    }
    if b.len() == 1 {
        return TokKind::BareHyphen;
    }
    if b.get(1) != Some(&b'-') {
        return TokKind::ShortCluster;
    }
    if b.len() == 2 {
        return TokKind::Terminator;
    }
    let rest = match tok.get(2..) {
        Some(r) => r,
        None => return TokKind::Malformed,
    };
    if rest.is_empty() || rest.len() > 40 {
        return TokKind::Malformed;
    }
    if starts_with(rest, "=") {
        return TokKind::Malformed;
    }
    let mut has_eq = false;
    for c in rest.chars() {
        if c == '=' {
            has_eq = true;
        }
    }
    if has_eq {
        TokKind::LongAssigned
    } else {
        TokKind::LongBare
    }
}

/// `--` 之后的整串（含 `=value`，文案要照抄）。
pub fn long_body(tok: &str) -> &str {
    match tok.get(2..) {
        Some(r) => r,
        None => "",
    }
}

/// `-` 之后的字母串。
pub fn cluster_letters(tok: &str) -> &str {
    match tok.get(1..) {
        Some(r) => r,
        None => "",
    }
}

/// 把 `name=value` 拆开；没有等号时值是空 `Option`。
pub fn split_assign(body: &str) -> (&str, Option<&str>) {
    let b = body.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if b.get(i) == Some(&b'=') {
            let name = match body.get(..i) {
                Some(n) => n,
                None => "",
            };
            let val = match body.get(i + 1..) {
                Some(v) => Some(v),
                None => Some(""),
            };
            return (name, val);
        }
        i += 1;
    }
    (body, None)
}

// ---------------------------------------------------------------------------
// 文案：逐字复刻基准机报错（`LC_ALL=C` 快照）
// ---------------------------------------------------------------------------

/// applet 前缀：`ls: `。
pub fn applet_prefix(a: Applet) -> String {
    let mut s = String::from(a.name());
    s.push_str(": ");
    s
}

/// GNU 长项未命中：`ls: unrecognized option '--zzz'`。
///
/// 引号里是**去掉前导两横线的整串**，包括 `=value`——glibc 把 `nextchar` 原样
/// 打出来，所以 `--zzz=1` 的文案里带 `=1`。
pub fn msg_gnu_unrec_long(a: Applet, loc: Locale, body: &str) -> String {
    let mut s = applet_prefix(a);
    s.push_str(ANCHOR_LONG_VERB);
    s.push(' ');
    s.push_str(loc.quote(body).as_str());
    s
}

/// GNU 短项未命中：`ls: invalid option -- 'Z'`。
pub fn msg_gnu_bad_short(a: Applet, loc: Locale, c: char) -> String {
    let mut s = applet_prefix(a);
    s.push_str(ANCHOR_SHORT_VERB);
    s.push_str(" -- ");
    s.push_str(loc.quote_char(c).as_str());
    s
}

/// GNU 歧义：`ls: option '--fo' is ambiguous; possibilities: '--format' '--foreground'`。
///
/// `name` 是**不含前导横线**的名字（候选集按它筛），但引号里必须是带 `--` 的原串：
/// glibc 打的是 `nextchar`，也就是 argv 里那一段，前缀两横线不能丢。
pub fn msg_gnu_ambiguous(a: Applet, name: &str) -> String {
    let mut s = applet_prefix(a);
    s.push_str("option ");
    let mut full = String::from("--");
    full.push_str(name);
    s.push_str(ascii_quote(full.as_str()).as_str());
    s.push(' ');
    s.push_str(ANCHOR_AMBIGUOUS);
    let sp = spec(a);
    let (hits, n) = prefix_hits(a, name);
    let mut k = 0usize;
    while k < n {
        let Some(idx) = hits.get(k) else {
            k += 1;
            continue;
        };
        let Some(l) = sp.longs.get(*idx) else {
            k += 1;
            continue;
        };
        s.push(' ');
        s.push_str(ascii_quote(l).as_str());
        k += 1;
    }
    s
}

/// GNU 长项缺值：`ls: option '--width' requires an argument`。
pub fn msg_gnu_long_need_arg(a: Applet, full: &str) -> String {
    let mut s = applet_prefix(a);
    s.push_str("option ");
    s.push_str(ascii_quote(full).as_str());
    s.push_str(" requires an argument");
    s
}

/// GNU 短项缺值：`ls: option requires an argument -- 'w'`。
pub fn msg_gnu_short_need_arg(a: Applet, c: char) -> String {
    let mut s = applet_prefix(a);
    s.push_str(ANCHOR_NEEDS_ARG);
    s.push_str(" -- ");
    s.push_str(ascii_quote_char(c).as_str());
    s
}

/// GNU 不许带值：`ls: option '--help' doesn't allow an argument`。
pub fn msg_gnu_no_arg_allowed(a: Applet, full: &str) -> String {
    let mut s = applet_prefix(a);
    s.push_str("option ");
    s.push_str(ascii_quote(full).as_str());
    s.push_str(" doesn't allow an argument");
    s
}

/// busybox 未命中：`ls: unrecognized option: --zzz`（动词后是冒号，不带引号）。
pub fn msg_bb_unrecognized(a: Applet, body: &str) -> String {
    let mut s = applet_prefix(a);
    s.push_str(ANCHOR_BB_VERB);
    s.push(' ');
    s.push_str(body);
    s
}

/// busybox 缺值：只落 usage 面，正文由 applet 自出。
pub fn msg_bb_usage_die(a: Applet) -> String {
    let mut s = applet_prefix(a);
    s.push_str(bb_usage_line(a));
    s
}

/// GNU 的 Try 提示行（硬编码 ASCII 引号，不随 locale）。
pub fn msg_try_line(a: Applet) -> String {
    let sp = spec(a);
    let mut s = String::from("Try '");
    s.push_str(a.name());
    s.push_str(" --help'");
    if sp.has_usage {
        s.push_str(" or '");
        s.push_str(a.name());
        s.push_str(" --usage'");
    }
    s.push_str(" for more information.");
    s
}

/// busybox 的小写 usage 行（正文逐字归 F11668，本卡只钉这一行的存在）。
pub fn bb_usage_line(a: Applet) -> &'static str {
    match a {
        Applet::Ls => "usage: ls [OPTIONS]... [FILE]...",
        Applet::Cp => "usage: cp [OPTIONS]... SOURCE DEST",
        Applet::Head => "usage: head [OPTIONS]... [FILE]...",
        Applet::Tail => "usage: tail [OPTIONS]... [FILE]...",
        Applet::Wc => "usage: wc [OPTIONS]... [FILE]...",
        _ => "usage: [OPTIONS]... [FILE]...",
    }
}

/// ASCII 单引号包一个字符。
pub fn ascii_quote_char(c: char) -> String {
    let mut s = String::from("'");
    s.push(c);
    s.push('\'');
    s
}

/// 完整一屏（正文行 + 提示行）。
impl Locale {
    /// 按画像包一个字符。
    pub fn quote_char(self, c: char) -> String {
        match self {
            Locale::Utf8 => {
                let mut s = String::from("\u{2018}");
                s.push(c);
                s.push_str("\u{2019}");
                s
            }
            Locale::C | Locale::Unread => ascii_quote_char(c),
        }
    }
}

// ---------------------------------------------------------------------------
// Shape：一次 argv 扫描能读到的全部维度
// ---------------------------------------------------------------------------

/// 一格输入形状。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// 哪个 applet（决定查哪张选项表）。
    pub applet: Applet,
    /// 文案家族。
    pub flavor: Flavor,
    /// 引号式样的 locale 画像。
    pub locale: Locale,
    /// 出问题的 token 原文。
    pub tok: &'static str,
    /// 申报的 token 形状（与逐字节分类器对账）。
    pub kind: TokKind,
    /// 坏 token 之前是否已越过 `--`。
    pub dd_before: bool,
    /// 坏 token 之前 `--help`/`--usage` 是否已生效。
    pub help_before: bool,
    /// 坏 token 之前已消费的 argv 项数（副作用深度基线）。
    pub consumed_before: u32,
    /// argv 里后面还有没有 token（能不能拿来当值）。
    pub value_follows: bool,
    /// `optstring` 以 `:` 起手：getopt 不出文案，applet 自己出。
    pub optstring_quiet: bool,
    /// `opterr` 是否打开（关掉以后本卡不钉正文）。
    pub opterr_on: bool,
    /// 坏 token 在 argv 里的序号。
    pub argv_index: u32,
    /// 申报的退出码（emit 闸对它）。
    pub exit_declared: u32,
}

impl Shape {
    /// 基型：一个全绿的 `ls --width=80`，供记录更新组装用。
    pub const fn blank() -> Shape {
        Shape {
            applet: Applet::Ls,
            flavor: Flavor::Gnu,
            locale: Locale::C,
            tok: "--width=80",
            kind: TokKind::LongAssigned,
            dd_before: false,
            help_before: false,
            consumed_before: 0u32,
            value_follows: true,
            optstring_quiet: false,
            opterr_on: true,
            argv_index: 1u32,
            exit_declared: EXIT_OK,
        }
    }

    /// 文案是否由 applet 自出（本卡只钉码与锚）。
    pub fn silent_text(&self) -> bool {
        self.optstring_quiet || !self.opterr_on
    }

    /// 形状整体是否自洽。不自洽不改变落点，只把 `consistent` 压成 `false`。
    pub fn valid(&self) -> bool {
        if !self.flavor.speakable() {
            return false;
        }
        if self.tok.len() > 48 {
            return false;
        }
        if classify(self.tok) != self.kind {
            return false;
        }
        if self.consumed_before > self.argv_index {
            return false;
        }
        if self.dd_before && self.help_before {
            return false;
        }
        if self.exit_declared > EXIT_NOT_FOUND {
            return false;
        }
        if self.kind == TokKind::Malformed && self.applet.judged() {
            // 畸形长串是合法形状（glibc 照样报错），但必须真是 `--` 起手。
            if !starts_with(self.tok, "--") {
                return false;
            }
        }
        true
    }

    /// 只换 token 与形状。
    pub fn with_tok(&self, t: &'static str, k: TokKind) -> Shape {
        let mut s = *self;
        s.tok = t;
        s.kind = k;
        s
    }

    /// 只换家族。
    pub fn with_flavor(&self, f: Flavor) -> Shape {
        let mut s = *self;
        s.flavor = f;
        s
    }

    /// 只换 applet。
    pub fn with_applet(&self, a: Applet) -> Shape {
        let mut s = *self;
        s.applet = a;
        s
    }

    /// 形状指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.applet.index() as u64);
        h = fold_word(h, self.flavor.index() as u64);
        h = fold_str(h, self.locale.name());
        h = fold_str(h, self.tok);
        h = fold_str(h, self.kind.name());
        h = fold_bool(h, self.dd_before);
        h = fold_bool(h, self.help_before);
        h = fold_word(h, self.consumed_before as u64);
        h = fold_bool(h, self.value_follows);
        h = fold_bool(h, self.optstring_quiet);
        h = fold_bool(h, self.opterr_on);
        h = fold_word(h, self.argv_index as u64);
        fold_word(h, self.exit_declared as u64)
    }
}

/// 一次扫描的**实际**结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 落点闸。
    pub stage: Stage,
    /// 退出码（0 表示这一格绿）。
    pub code: i32,
    /// 副作用深度：出错点之前已经生效的选项数。
    pub value: u32,
    /// 文案家族档。
    pub note: Note,
    /// 本卡 unknown-option 锚。
    pub default: bool,
    /// 结构自洽：谎报的形状在这里被压假，但落点照算。
    pub consistent: bool,
}

impl Outcome {
    /// 全零初值。
    pub const fn blank() -> Outcome {
        Outcome {
            stage: Stage::Emit,
            code: 0i32,
            value: 0u32,
            note: Note::LongExactOk,
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

    /// 是否绿（成败互报的那一维就按这个读，比 `code == 0` 更严）。
    pub fn ok(&self) -> bool {
        self.code == 0 && self.consistent
    }

    /// 是否红。
    pub fn failed(&self) -> bool {
        self.code != 0
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
    pub fn exit(&self) -> u32 {
        if self.ok() {
            return EXIT_OK;
        }
        if self.code < 0 {
            return EXIT_USAGE;
        }
        self.code as u32
    }

    /// 该不该落锚（复算一遍供 `default` 自检）：选项名本身坏、结构自洽、
    /// 家族能出文案、码非零。
    pub fn should_anchor(&self) -> bool {
        if !self.consistent || self.code == 0 {
            return false;
        }
        if !self.stage.names_bad_option() {
            return false;
        }
        self.note.is_unknown_option() || matches!(self.note, Note::OwnMessage)
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

/// 申报（读到的那一段账；与 `Outcome` 同构但没有自洽权）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    /// 申报落在哪道闸。
    pub stage: Stage,
    /// 申报的退出码。
    pub code: i32,
    /// 申报的副作用深度。
    pub value: u32,
    /// 申报的文案家族档。
    pub note: Note,
    /// 申报的锚位。
    pub default: bool,
}

impl Claim {
    /// 全零初值。
    pub const fn blank() -> Claim {
        Claim {
            stage: Stage::Emit,
            code: 0i32,
            value: 0u32,
            note: Note::LongExactOk,
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
        self.code == 0
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

/// 一格语料。
#[derive(Clone, Copy, Debug)]
pub struct Case {
    /// 输入形状。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
    /// 短名（必须是本卡标签）。
    pub tag: &'static str,
}

impl Case {
    /// 空格。
    pub const fn blank() -> Case {
        Case {
            shape: Shape::blank(),
            claim: Claim::blank(),
            tag: "uo-blank",
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

// ---------------------------------------------------------------------------
// define：把形状过八道闸
// ---------------------------------------------------------------------------

/// 未知长项现场的落点（家族决定动词与码，`silent_text` 决定钉不钉正文）。
fn unknown_long_at(s: &Shape, consistent: bool, consumed: u32) -> Outcome {
    let base = match s.flavor {
        Flavor::Gnu => Note::GnuUnrecLong,
        Flavor::Busybox => Note::BbUnrecLong,
        Flavor::Mixed => Note::VerbMismatch,
    };
    let note = if s.silent_text() && matches!(s.flavor, Flavor::Gnu | Flavor::Busybox) {
        Note::OwnMessage
    } else {
        base
    };
    let anchored = s.flavor.speakable() && !s.anchor_forbidden();
    Outcome::make(
        Stage::Long,
        s.flavor.usage_exit(),
        consumed,
        note,
        anchored,
        consistent,
    )
}

/// 未知短项现场的落点。
fn unknown_short_at(s: &Shape, pos: usize, c: char, consistent: bool) -> Outcome {
    let consumed = s.consumed_before.saturating_add(pos as u32);
    let base = match s.flavor {
        Flavor::Gnu => Note::GnuBadShort,
        Flavor::Busybox => Note::BbUnrecShort,
        Flavor::Mixed => Note::VerbMismatch,
    };
    let note = if s.silent_text() && matches!(s.flavor, Flavor::Gnu | Flavor::Busybox) {
        Note::OwnMessage
    } else {
        base
    };
    let anchored = s.flavor.speakable() && !s.anchor_forbidden();
    Outcome::make(Stage::Short, s.flavor.usage_exit(), consumed, note, anchored, consistent)
}

/// `Mixed` 家族整格判死，不许落锚。
impl Shape {
    /// 这一格是否禁止落锚（家族混认时成立）。
    pub fn anchor_forbidden(&self) -> bool {
        matches!(self.flavor, Flavor::Mixed)
    }
}

/// **本卡的纵线**：八道闸按秩走，前一闸不产出事实，后一闸就没有输入。
pub fn define(s: &Shape) -> Outcome {
    let mut consistent = s.valid();

    // --- 闸 1：dispatch ----------------------------------------------------
    if !s.applet.judged() {
        return Outcome::make(
            Stage::Dispatch,
            0i32,
            0u32,
            Note::NotOurApplet,
            false,
            consistent,
        );
    }

    // --- 闸 2：dashdash ----------------------------------------------------
    if s.dd_before {
        if s.help_before {
            // `--` 之后不可能还有一个"生效的 --help"：那是谎报现场。
            consistent = false;
        }
        return Outcome::make(
            Stage::DashDash,
            0i32,
            s.consumed_before,
            Note::OperandAfterDD,
            false,
            consistent,
        );
    }

    // --- 闸 3：precedence --------------------------------------------------
    if s.help_before {
        return Outcome::make(
            Stage::Precedence,
            0i32,
            s.consumed_before,
            Note::HelpShortCircuit,
            false,
            consistent,
        );
    }

    // --- 闸 4：lex ---------------------------------------------------------
    let k = classify(s.tok);
    if k != s.kind {
        consistent = false;
    }
    match k {
        TokKind::PlainOperand => {
            return Outcome::make(
                Stage::Lex,
                0i32,
                s.consumed_before,
                Note::PlainOperand,
                false,
                consistent,
            );
        }
        TokKind::BareHyphen => {
            return Outcome::make(
                Stage::Lex,
                0i32,
                s.consumed_before,
                Note::BareHyphenStdin,
                false,
                consistent,
            );
        }
        TokKind::Terminator => {
            return Outcome::make(
                Stage::Lex,
                0i32,
                s.consumed_before,
                Note::TerminatorToken,
                false,
                consistent,
            );
        }
        _ => {}
    }

    // --- 闸 6：short -------------------------------------------------------
    if k == TokKind::ShortCluster {
        let letters = cluster_letters(s.tok);
        if letters.is_empty() {
            return Outcome::make(
                Stage::Lex,
                0i32,
                s.consumed_before,
                Note::BareHyphenStdin,
                false,
                consistent,
            );
        }
        match scan_cluster(s.applet, letters, s.value_follows) {
            Cluster::AllKnown => {
                return Outcome::make(
                    Stage::Short,
                    0i32,
                    s.consumed_before,
                    Note::ShortClusterOk,
                    false,
                    consistent,
                );
            }
            Cluster::BadAt { pos, letter } => {
                return unknown_short_at(s, pos, letter, consistent);
            }
            Cluster::NeedsArg { pos, .. } => {
                let consumed = s.consumed_before.saturating_add(pos as u32);
                let note = match s.flavor {
                    Flavor::Gnu => Note::GnuShortNeedArg,
                    _ => Note::BbUsageDie,
                };
                let code = if matches!(s.flavor, Flavor::Busybox) {
                    1i32
                } else {
                    s.flavor.usage_exit()
                };
                return Outcome::make(
                    Stage::ArgForm, code, consumed, note, false, consistent,
                );
            }
        }
    }

    // --- 闸 5 + 闸 7：long -------------------------------------------------
    let body = long_body(s.tok);
    let (name, val) = split_assign(body);
    if k == TokKind::Malformed {
        return unknown_long_at(s, consistent, s.consumed_before);
    }
    match long_lookup(s.applet, name) {
        LongHit::None | LongHit::Malformed => {
            unknown_long_at(s, consistent, s.consumed_before)
        }
        LongHit::Ambiguous(_) => {
            let note = match s.flavor {
                Flavor::Gnu => Note::GnuAmbiguous,
                _ => Note::BbUsageDie,
            };
            let code = if matches!(s.flavor, Flavor::Busybox) {
                1i32
            } else {
                s.flavor.usage_exit()
            };
            Outcome::make(Stage::Long, code, s.consumed_before, note, false, consistent)
        }
        LongHit::Exact(i) | LongHit::Unique(i) => {
            let ok_note = match k {
                TokKind::LongAssigned | TokKind::LongBare => {
                    if matches!(long_lookup(s.applet, name), LongHit::Unique(_)) {
                        Note::LongAbbrevOk
                    } else {
                        Note::LongExactOk
                    }
                }
                _ => Note::LongExactOk,
            };
            if long_needs_value(s.applet, i) {
                match val {
                    None => {
                        if s.value_follows {
                            Outcome::make(
                                Stage::Emit,
                                0i32,
                                s.consumed_before,
                                ok_note,
                                false,
                                consistent,
                            )
                        } else {
                            let note = match s.flavor {
                                Flavor::Gnu => Note::GnuLongNeedArg,
                                _ => Note::BbUsageDie,
                            };
                            let code = if matches!(s.flavor, Flavor::Busybox) {
                                1i32
                            } else {
                                s.flavor.usage_exit()
                            };
                            Outcome::make(
                                Stage::ArgForm,
                                code,
                                s.consumed_before,
                                note,
                                false,
                                consistent,
                            )
                        }
                    }
                    Some(v) => {
                        if v.is_empty() {
                            // 值存在但是空串：出本卡范围，移交 F11670。
                            return Outcome::make(
                                Stage::ArgForm,
                                0i32,
                                s.consumed_before,
                                Note::HandoffEmptyValue,
                                false,
                                consistent,
                            );
                        }
                        Outcome::make(
                            Stage::Emit,
                            0i32,
                            s.consumed_before,
                            ok_note,
                            false,
                            consistent,
                        )
                    }
                }
            } else if val.is_some() {
                let note = match s.flavor {
                    Flavor::Gnu => Note::GnuNoArgAllowed,
                    _ => Note::BbUsageDie,
                };
                let code = if matches!(s.flavor, Flavor::Busybox) {
                    1i32
                } else {
                    s.flavor.usage_exit()
                };
                Outcome::make(
                    Stage::ArgForm,
                    code,
                    s.consumed_before,
                    note,
                    false,
                    consistent,
                )
            } else {
                Outcome::make(
                    Stage::Emit,
                    0i32,
                    s.consumed_before,
                    ok_note,
                    false,
                    consistent,
                )
            }
        }
    }
}

// @@PART5@@

/// 槽级对判：申报对实测逐维比，第一个错维定案（判序钉死在 `Fault::order()`：
/// 成败 → 落点闸 → 退出码 → 副作用深度 → 文案家族 → 锚位）。
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
