//! UNX-F11668 <applet> --help 与 applet usage 逐字判据
//!
//! 这张卡管的是**帮助成功侧**：applet 已经定名、已经进 `main`、argv 里躺着一个
//! `--help`，进程把一段 usage 全文吐到 **stdout**，然后退 **0**。基准机
//! （GNU coreutils 8.32 + BusyBox 1.36.1，`LC_ALL=C`）上现场长这样：
//!
//! ```text
//! $ ls --help
//! Usage: ls [OPTION]... [FILE]...
//! List information about the FILEs (the current directory by default).
//! ...
//! GNU coreutils online help: <https://www.gnu.org/software/coreutils/>
//! Full documentation <https://www.gnu.org/software/coreutils/ls>
//! or available locally via: info '(coreutils) ls invocation'
//!
//! $ busybox ls --help
//! BusyBox v1.36.1 (2023-01-03 11:00:00 UTC) multi-call binary.
//!
//! Usage: ls [-Z] [-1AaCxdLHRFplinshrSXvctu] [-w WIDTH] [FILE]...
//! List directory contents
//! ...
//! ```
//!
//! 两段都叫"usage"，但**逐字**是三件不同的事，混认一次全批崩：
//!
//! * **首行骨架**。coreutils 的首行恒是 `Usage: <名> [OPTION]... ...`（`OPTION`
//!   是八股占位，具体 token 在 `--usage` 那一行才铺开）；busybox 的首行把**全部**
//!   短项位图一次列尽（`Usage: ls [-Z] [-1AaC...] [-w WIDTH] [FILE]...`），
//!   没有 `[OPTION]...` 这个占位。拿 `[OPTION]` 去 grep busybox 的首行，永远空手。
//! * **banner**。`busybox ls --help`（多调用入口）固定先打三行
//!   `BusyBox v1.36.1 (...) multi-call binary.` + 空行；但同一个 applet 走**符号
//!   链接**入口（`/bin/ls` 是指向 busybox 的链）时**不打** banner。同一段 usage，
//!   两种入口字节数不同——只钉正文不钉入口，逐字判据就会在链调用现场误判。
//! * **尾注**。coreutils 正文末尾恒附三行（在线帮助 / 文档 / `info '(coreutils)
//!   <名> invocation'`），busybox 一个字节都没有。把尾注算进"正文必须存在"，
//!   busybox 现场会被读成缺段。
//!
//! 还有两条最容易读反的纵线：
//!
//! * **`-h` 不是 `--help`**。`ls -h` 在 coreutils 与 busybox 里都是
//!   `--human-readable`，进程照常列目录、退 0、**stdout 里没有一个 usage 字节**。
//!   把 `-h` 当帮助短路，就会给一个"跑了活"的现场安上一整段 usage。
//! * **`--usage` 只有一半家族有**。coreutils 认 `--usage`（单行、退 0，与
//!   `--help` 同一段文本的压缩形式）；busybox 的长项表里没有它，落进
//!   `unrecognized option` 面，退 **1**（那一行的逐字归 F11669，本卡只钉
//!   "没有 usage 正文"与码档）。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 七道闸，秩从 **1** 起算；`Gate` 只接受"前进到下一秩"。
//!
//! 1. `dispatch`（秩 1）——applet 是否已定名且在本卡管辖内。未注册名的第一句红是
//!    `<名>: applet not found` + 127（F11674 的锚），本卡在这一档零裁：**先出局**，
//!    不许拿帮助表替它作答。
//! 2. `form`（秩 2）——请求的是哪一种形式：`--help`（长篇）、`--usage`（单行）、
//!    `-h`（**不是帮助**，是 applet 自己的开关）、无开关操作数。形式判错，后面
//!    每一维都跟着错，所以它必须早于文本组装。
//! 3. `line`（秩 3）——选哪条家族的文本线：coreutils / busybox / 混认。混认没有
//!    任何快照，必须当场判死（红且不给正文），不许退化成"随便挑一条"。
//! 4. `banner`（秩 4）——入口决定 banner：多调用入口（`busybox <名> --help`）有，
//!    符号链接入口没有。这一维必须独立成闸，因为它改变**字节数**而不改变首行。
//! 5. `body`（秩 5）——首行 + 正文 + 尾注按序拼装，并复核截断：截断的正文不许
//!    当逐字现场（短路成"只看到首行"的账是假绿）。
//! 6. `channel`（秩 6）——帮助恒走 **stdout**。落到 stderr 的"帮助"不是帮助，
//!    是本卡的通道误读档（`F11678` 判字节落在哪个 fd，本卡判"该不该在 stdout"）。
//! 7. `exit`（秩 7）——码档结算：帮助成功恒 0；busybox 用法面 1；未名 applet 127。
//!    127 出现在帮助成功侧即越界判死。
//!
//! ## 关键差异（本卡深度所在）
//!
//! * **与 F11667（`busybox --help` 顶层帮助，段基址 `0x6d1e0`，位距 `0x10`）的
//!   边界**：那张卡判**多调用二进制自己**的帮助全文（banner + usage 三行 +
//!   脚注 + 名录），主语是 `busybox`；本卡判**已定名 applet** 的帮助，主语是
//!   applet 名。同一台机器上 `busybox --help` 与 `busybox ls --help` 是两段完全
//!   不同的文本，前者不出 `Usage: ls ...`，后者不出 `Currently defined functions:`。
//!   谁判死谁放行：本卡对顶层名录判死（不认），F11667 对 applet 正文放行。
//! * **与 F11669（unknown option applet 层，段基址 `0x6d210`，位距 `0x20`）的
//!   边界**：分界是**成败**。`--help` 成功侧归本卡（stdout、退 0、全文逐字）；
//!   `--zzz` 失败侧归 F11669（stderr、码 1/2、只钉一行 Try/`usage:`）。唯一交叠
//!   点是 busybox 的 `--usage`：它在本卡是"**没有正文的成功侧假象**"（实际是
//!   失败侧），本卡只钉"无 usage 正文 + 码 1"，那一行的逐字放行给 F11669。
//! * **与 F11674（未知 applet 文案，段基址 `0x6d270`）的边界**：`<名>: applet
//!   not found` 的**模板**归那张卡；本卡只在 `dispatch` 闸引用它作为"零裁"的
//!   证据，一个字都不重复判。名字大小写/别名匹配归 F11671（`0x6d230`），本卡
//!   按 `argv[1]` 的**原样字节**取名字，不做任何折叠。
//! * **与 F11678（stderr/stdout 分离，段基址 `0x6d2d0`）的边界**：那张卡数字节
//!   落在哪个 fd；本卡判"帮助该不该在 stdout、在不在"。
//! * **本卡的排他职责**：① `--help` 与 `-h` 不得互串；② 多调用入口的 banner 与
//!   符号链接入口的无 banner 是**两档**，字节数不同；③ coreutils 尾注三行只配
//!   coreutils 正文，不许出现在 busybox 现场；④ 帮助的通道恒 stdout；
//!   ⑤ 帮助码档恒 0，1/127 出现在成功侧即判死；⑥ 截断的正文一律判死。
//! * **双线差异的落位**：本卡与 F11694（strerror 逐字）的分工是"文本主人"——
//!   coreutils 文案的弯引号随 locale 变（`LC_ALL=C` 是 ASCII），而 usage 正文
//!   **不随 locale 变**，所以本卡可以按 `LC_ALL=C` 快照钉死；一条非 `C` 的记录
//!   在 `valid()` 里当场压假，绝不用 locale 差异给漂移松绑。
//!
//! 判据六维：结束阶段（gate）、退出码（code）、正文行数（value）、文本家族档
//! （note）、兜底锚（default）、成败申报（estate）。判序固定：
//! 状态 → 阶段 → 错误码 → 值 → 注解 → 锚位。
//!
//! 语料 14 槽：8 槽申报正确（coreutils 长篇四档、busybox 多调用两档、单行
//! `--usage` 一档、`-h` 非帮助一档），6 槽一槽只坏一维，依次挂
//! Estate/Stage/Code/Value/Note/Default。
//!
//! 账本 14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码，冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格对账。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11668，VX-1029-5 批分配值，不得自改）。
pub const SEG_BASE: u32 = 0x6d1f0;

/// 交叉署名：F11667「busybox --help 退出码 0 与 usage 全文」的段基址（本卡直接前置）。
pub const ANCHOR_F11667_SEG: u32 = 0x6d1e0;
/// F11667 与本卡的位距（0x6d1f0 - 0x6d1e0）。
pub const ANCHOR_F11667_GAP: u32 = 0x10;

/// 交叉署名：F11669「unknown option applet 层」的段基址（本卡直接后置）。
pub const ANCHOR_F11669_SEG: u32 = 0x6d210;
/// F11669 与本卡的位距（0x6d210 - 0x6d1f0）。
pub const ANCHOR_F11669_GAP: u32 = 0x20;

/// 交叉署名：F11674「未知 applet 文案」的段基址（dispatch 闸的移交对象）。
pub const ANCHOR_F11674_SEG: u32 = 0x6d270;

/// 交叉署名：F11678「stderr/stdout 分离」的段基址（通道面对手卡）。
pub const ANCHOR_F11678_SEG: u32 = 0x6d2d0;

/// 语料槽数。
pub const CORPUS: usize = 14;
/// 账本格数（与语料同尺）。
pub const SAMPLES: usize = 14;
/// 申报正确槽数。
pub const ACCEPT: usize = 8;
/// 一槽只坏一维的槽数。
pub const REJECT: usize = 6;
/// 档位维数。
pub const TIER_COUNT: usize = 4;

/// 闸门道数（秩 1..7）。
pub const STAGE_COUNT: u32 = 7;
/// 最后一道闸的秩。
pub const STAGE_LAST_RANK: u32 = 7;
/// 账本统计格数。
pub const STAT_CELLS: usize = 7;
/// 红档率标定值（千分率，14 格中 13 格时的读数）。
pub const RED_RATE: u32 = 929;
/// 全绿千分率。
pub const RATE_FULL: u32 = 1000;

/// 本卡认识的 applet 个数（含未管辖位）。
pub const APPLET_COUNT: usize = 6;
/// 参与本卡帮助表的 applet 数（其余在 dispatch 闸出局）。
pub const APPLET_JUDGED: usize = 5;
/// 文本线档数（coreutils / busybox / 混认）。
pub const LINE_COUNT: usize = 3;
/// 帮助形式档数（`--help` / `--usage` / `-h` / 操作数）。
pub const FORM_COUNT: usize = 4;
/// usage 表格数（每个受管辖 applet × 两条文本线）。
pub const USAGE_CELLS: usize = 10;
/// 正文行数的合法上界（远超基准机最大帮助即为坏形状）。
pub const BODY_LINE_CAP: u32 = 400;
/// 首行字节数的合法上界。
pub const FIRST_BYTE_CAP: u32 = 160;

/// 帮助成功码：本卡的正判据锚档。
pub const EXIT_OK: u32 = 0;
/// busybox 家族用法错码（帮助面**不许**出现）。
pub const EXIT_FAIL: u32 = 1;
/// coreutils 家族用法错码（帮助面**不许**出现）。
pub const EXIT_USAGE: u32 = 2;
/// 未注册 applet 的码（F11664/F11674 持有；本卡只在 dispatch 引用）。
pub const EXIT_NOTFOUND: u32 = 127;

/// 多调用二进制的版本串（`busybox ls --help` banner 里的那一段）。
pub const BB_VERSION: &str = "BusyBox v1.36.1 (2023-01-03 11:00:00 UTC) multi-call binary.";
/// banner 的完整形式（含其后空行）。
pub const BANNER_TEXT: &str = "BusyBox v1.36.1 (2023-01-03 11:00:00 UTC) multi-call binary.\n\n";
/// coreutils 尾注第一行的逐字前缀（其余两行含 applet 名）。
pub const TRAILER_HEAD: &str = "GNU coreutils online help: <https://www.gnu.org/software/coreutils/>\n";
/// 首行八股占位：只有 coreutils 用。
pub const GNU_PLACEHOLDER: &str = "[OPTION]...";
/// busybox 首行的固定前缀（大写 Usage + 冒号空格）。
pub const USAGE_PREFIX: &str = "Usage: ";

// ---------------------------------------------------------------------------
// 错误码：13 个，前缀 AH，值 SEG_BASE + 0x01 .. + 0x0d
// ---------------------------------------------------------------------------

/// AH+0x01 状态违例：闸门封账后再走、账本冻结后再登记。
pub const AH_ESTATE: u32 = SEG_BASE + 0x01;
/// AH+0x02 阶段维缺陷：申报的落点闸与现场不符。
pub const AH_ESTAGE: u32 = SEG_BASE + 0x03;
/// AH+0x03 码维缺陷：帮助成功侧报了非 0 码，或用法面报了 0。
pub const AH_ECODE: u32 = SEG_BASE + 0x02;
/// AH+0x04 值维缺陷：正文行数与现场不符。
pub const AH_EVALUE: u32 = SEG_BASE + 0x04;
/// AH+0x05 注解维缺陷：文本家族档张冠李戴。
pub const AH_ENOTE: u32 = SEG_BASE + 0x05;
/// AH+0x06 兜底维缺陷：无正文现场申报成"有正文"或反之。
pub const AH_EDEFAULT: u32 = SEG_BASE + 0x06;

/// AH+0x07 正文维缺陷：usage 表缺格、首行骨架不符或正文被截断。
pub const AH_EBODY: u32 = SEG_BASE + 0x07;
/// AH+0x08 通道维缺陷：帮助没有落在 stdout。
pub const AH_ECHAN: u32 = SEG_BASE + 0x08;
/// AH+0x09 槽位越界：账本槽号跳号或注入槽越界。
pub const AH_ESLOT: u32 = SEG_BASE + 0x09;
/// AH+0x0a 指纹不符：冻结后重放对不上。
pub const AH_ESEAL: u32 = SEG_BASE + 0x0a;
/// AH+0x0b 观测缺失：语料/期望表取不到格。
pub const AH_ENOBS: u32 = SEG_BASE + 0x0b;
/// AH+0x0c 矩阵不闭合：四档合计不等于语料格数。
pub const AH_EMATRIX: u32 = SEG_BASE + 0x0c;
/// AH+0x0d 账不平：格数、成败配比或逐格复核不通过。
pub const AH_EBAL: u32 = SEG_BASE + 0x0d;

/// 13 个错误码按序（自检与账本共用）。
pub const ALL_CODES: [u32; 13] = [
    AH_ESTATE,
    AH_ESTAGE,
    AH_ECODE,
    AH_EVALUE,
    AH_ENOTE,
    AH_EDEFAULT,
    AH_EBODY,
    AH_ECHAN,
    AH_ESLOT,
    AH_ESEAL,
    AH_ENOBS,
    AH_EMATRIX,
    AH_EBAL,
];

// ---------------------------------------------------------------------------
// 账本操作码：本卡七个扫描阶段 + 红档 + 缺陷
// ---------------------------------------------------------------------------

/// 操作码：dispatch 闸。
pub const OP_DISPATCH: u8 = 0x01;
/// 操作码：form 闸。
pub const OP_FORM: u8 = 0x02;
/// 操作码：line 闸。
pub const OP_LINE: u8 = 0x03;
/// 操作码：banner 闸。
pub const OP_BANNER: u8 = 0x04;
/// 操作码：body 闸。
pub const OP_BODY: u8 = 0x05;
/// 操作码：channel 闸。
pub const OP_CHANNEL: u8 = 0x06;
/// 操作码：exit 闸。
pub const OP_EXIT: u8 = 0x07;

/// 统计格的操作码（与七道闸一一对应）。
pub const STAT_OPS: [u8; 7] = [
    OP_DISPATCH,
    OP_FORM,
    OP_LINE,
    OP_BANNER,
    OP_BODY,
    OP_CHANNEL,
    OP_EXIT,
];

/// 红档格的操作码。
pub const OPID_STAT_RED: u8 = 0x08;
/// 六个缺陷码格的算子号起点。
pub const DEFECT_OP_BASE: u8 = 0x21;

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

/// 折一个字（小端逐字节）进哈希。
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

/// 折一段文本进哈希（先折长度，避免拼接歧义）。
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

/// 折一个有符号整数进哈希（符号与绝对值分两拍）。
fn fold_i32(h: u64, v: i32) -> u64 {
    let neg = if v < 0 { 1u64 } else { 0u64 };
    let mag = if v < 0 { v.unsigned_abs() as u64 } else { v as u64 };
    fold_word(fold_word(h, neg), mag)
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
        v /= 10;
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

/// u32 → 固定宽度十六进制文本（小写，高位起）。
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

/// 千分率文本（`929permille`）。
pub fn permille_text(v: u32) -> String {
    let mut s = dec_text(v);
    s.push_str("permille");
    s
}

/// 文本行数（按 `\n` 计）。
fn lines_of(s: &str) -> u32 {
    let b = s.as_bytes();
    let mut n = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b'\n' {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 文本字节数。
fn bytes_of(s: &str) -> u32 {
    s.as_bytes().len() as u32
}

// ---------------------------------------------------------------------------
// 域模型枚举：applet / 文本线 / 入口 / 形式 / 通道 / 阶段 / 注解 / 档位 / 缺陷
// ---------------------------------------------------------------------------

/// 本卡管辖的 applet（帮助表按 applet 分家，名字逐字用 argv 里那一个）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Applet {
    /// `ls`——帮助正文最长，`--usage` 那一行也最宽。
    Ls,
    /// `cp`——coreutils 首行有三段 `or:` 续行。
    Cp,
    /// `head`——短项 `-h` 不存在，是 `-h` 非帮助档的对照。
    Head,
    /// `tail`——同上，但短项表里 `-h` 也不存在。
    Tail,
    /// `wc`——busybox 首行最短（`[-clmwL]`），逐字对照锚。
    Wc,
    /// 未注册/别的工具——dispatch 闸零裁（归 F11674）。
    Unknown,
}

impl Applet {
    /// argv 里的那一个名字（同时也是未名文案的主语）。
    pub fn name(self) -> &'static str {
        match self {
            Applet::Ls => "ls",
            Applet::Cp => "cp",
            Applet::Head => "head",
            Applet::Tail => "tail",
            Applet::Wc => "wc",
            Applet::Unknown => "notmyapplet",
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
            Applet::Unknown => 5usize,
        }
    }

    /// 按序号取 applet；越界给 `None`。
    pub fn at(i: usize) -> Option<Applet> {
        match i {
            0usize => Some(Applet::Ls),
            1usize => Some(Applet::Cp),
            2usize => Some(Applet::Head),
            3usize => Some(Applet::Tail),
            4usize => Some(Applet::Wc),
            5usize => Some(Applet::Unknown),
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
            Applet::Unknown,
        ]
    }

    /// 是否进本卡帮助表（未注册名不进）。
    pub fn judged(self) -> bool {
        matches!(
            self,
            Applet::Ls | Applet::Cp | Applet::Head | Applet::Tail | Applet::Wc
        )
    }
}

/// 文本线：同一段 usage 在哪条家族上取值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UsageLine {
    /// GNU coreutils 8.32：首行用 `[OPTION]...` 占位，末尾附三行尾注。
    Gnu,
    /// BusyBox 1.36.1：首行铺满短项位图，无尾注，多调用入口带 banner。
    Busybox,
    /// 混认：没有快照，不产出任何正文（拿到空串就是判死信号）。
    Mixed,
}

impl UsageLine {
    /// 文本线名。
    pub fn name(self) -> &'static str {
        match self {
            UsageLine::Gnu => "gnu",
            UsageLine::Busybox => "busybox",
            UsageLine::Mixed => "mixed",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            UsageLine::Gnu => 0usize,
            UsageLine::Busybox => 1usize,
            UsageLine::Mixed => 2usize,
        }
    }

    /// 全集。
    pub fn all() -> [UsageLine; LINE_COUNT] {
        [UsageLine::Gnu, UsageLine::Busybox, UsageLine::Mixed]
    }

    /// 这条线有没有可对账的正文。
    pub fn produces_text(self) -> bool {
        matches!(self, UsageLine::Gnu | UsageLine::Busybox)
    }

    /// 这条线的帮助成功码（两族都是 0——这正是本卡的正判据锚）。
    pub const fn help_exit(self) -> u32 {
        match self {
            UsageLine::Gnu => EXIT_OK,
            UsageLine::Busybox => EXIT_OK,
            UsageLine::Mixed => EXIT_FAIL,
        }
    }

    /// 这条线是否认 `--usage`（coreutils 认，busybox 不认）。
    pub fn has_usage_flag(self) -> bool {
        matches!(self, UsageLine::Gnu)
    }

    /// 这条线是否附 coreutils 尾注三行。
    pub fn has_trailer(self) -> bool {
        matches!(self, UsageLine::Gnu)
    }

    /// 这条线的首行是否用 `[OPTION]...` 占位。
    pub fn uses_placeholder(self) -> bool {
        matches!(self, UsageLine::Gnu)
    }
}

/// 入口：同一段按 applet 的 usage 从哪个门进来。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    /// `busybox <名> --help`：多调用入口，banner 在前。
    MultiCall,
    /// `<名> --help`（argv[0] 是符号链接）：无 banner。
    Symlink,
}

impl Entry {
    /// 入口名。
    pub fn name(self) -> &'static str {
        match self {
            Entry::MultiCall => "multicall",
            Entry::Symlink => "symlink",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Entry::MultiCall => 0usize,
            Entry::Symlink => 1usize,
        }
    }

    /// 全集。
    pub fn all() -> [Entry; 2] {
        [Entry::MultiCall, Entry::Symlink]
    }

    /// 这个入口会不会打 banner（只有多调用入口会）。
    pub fn has_banner(self) -> bool {
        matches!(self, Entry::MultiCall)
    }
}

/// 帮助形式：argv 里那个起作用的 token 长什么样。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HelpForm {
    /// `--help`：长篇，stdout，退 0。
    LongHelp,
    /// `--usage`：单行（只有 coreutils 认）。
    LongUsage,
    /// `-h`：**不是帮助**（coreutils/busybox 都是 `--human-readable`）。
    ShortH,
    /// 没有任何帮助开关：applet 照常跑操作数。
    Operand,
}

impl HelpForm {
    /// 形式名。
    pub fn name(self) -> &'static str {
        match self {
            HelpForm::LongHelp => "--help",
            HelpForm::LongUsage => "--usage",
            HelpForm::ShortH => "-h",
            HelpForm::Operand => "operand",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            HelpForm::LongHelp => 0usize,
            HelpForm::LongUsage => 1usize,
            HelpForm::ShortH => 2usize,
            HelpForm::Operand => 3usize,
        }
    }

    /// 全集。
    pub fn all() -> [HelpForm; FORM_COUNT] {
        [
            HelpForm::LongHelp,
            HelpForm::LongUsage,
            HelpForm::ShortH,
            HelpForm::Operand,
        ]
    }

    /// 这个形式会不会打出 usage 正文。
    pub fn wants_text(self) -> bool {
        matches!(self, HelpForm::LongHelp | HelpForm::LongUsage)
    }

    /// 这个形式是不是"真帮助"（`-h` 与操作数都不是）。
    pub fn is_help(self) -> bool {
        matches!(self, HelpForm::LongHelp)
    }
}

/// 输出通道。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Channel {
    /// 标准输出（帮助的唯一合法通道）。
    Stdout,
    /// 标准错误（帮助落这里就是通道误读）。
    Stderr,
    /// 一个字都没出。
    Nowhere,
}

impl Channel {
    /// 通道名。
    pub fn name(self) -> &'static str {
        match self {
            Channel::Stdout => "stdout",
            Channel::Stderr => "stderr",
            Channel::Nowhere => "nowhere",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Channel::Stdout => 0usize,
            Channel::Stderr => 1usize,
            Channel::Nowhere => 2usize,
        }
    }

    /// 全集。
    pub fn all() -> [Channel; 3] {
        [Channel::Stdout, Channel::Stderr, Channel::Nowhere]
    }

    /// 该通道是否有字节。
    pub fn carries_bytes(self) -> bool {
        !matches!(self, Channel::Nowhere)
    }
}

/// 阶段：本卡七道闸，秩从 1 起。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 闸 1：applet 定名与管辖。
    Dispatch,
    /// 闸 2：帮助形式辨析。
    Form,
    /// 闸 3：文本线选择。
    Line,
    /// 闸 4：banner 判定。
    Banner,
    /// 闸 5：正文拼装与截断复核。
    Body,
    /// 闸 6：通道判定。
    Channel,
    /// 闸 7：码档结算。
    Exit,
}

impl Stage {
    /// 秩（1..7）。
    pub const fn rank(self) -> u32 {
        match self {
            Stage::Dispatch => 1u32,
            Stage::Form => 2u32,
            Stage::Line => 3u32,
            Stage::Banner => 4u32,
            Stage::Body => 5u32,
            Stage::Channel => 6u32,
            Stage::Exit => 7u32,
        }
    }

    /// 小写阶段名（明细行与矩阵用）。
    pub fn name(self) -> &'static str {
        match self {
            Stage::Dispatch => "dispatch",
            Stage::Form => "form",
            Stage::Line => "line",
            Stage::Banner => "banner",
            Stage::Body => "body",
            Stage::Channel => "channel",
            Stage::Exit => "exit",
        }
    }

    /// 按秩取阶段；越界给 `None`。
    pub fn from_rank(r: u32) -> Option<Stage> {
        match r {
            1u32 => Some(Stage::Dispatch),
            2u32 => Some(Stage::Form),
            3u32 => Some(Stage::Line),
            4u32 => Some(Stage::Banner),
            5u32 => Some(Stage::Body),
            6u32 => Some(Stage::Channel),
            7u32 => Some(Stage::Exit),
            _ => None,
        }
    }

    /// 全集（按闸序）。
    pub fn all() -> [Stage; 7] {
        [
            Stage::Dispatch,
            Stage::Form,
            Stage::Line,
            Stage::Banner,
            Stage::Body,
            Stage::Channel,
            Stage::Exit,
        ]
    }

    /// 这一闸是否还没产出任何正文。
    pub fn before_text(self) -> bool {
        matches!(self, Stage::Dispatch | Stage::Form | Stage::Line)
    }

    /// 这一闸是否已经结算完码档。
    pub fn settles_code(self) -> bool {
        matches!(self, Stage::Exit)
    }
}

/// 注解：文本家族档 + 兜底形态。一维只装"这一格该长成什么样"。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// coreutils 长篇帮助（首行 `[OPTION]...` + 三行尾注）。
    HelpVerbatim,
    /// busybox 多调用入口：banner + 短项位图首行。
    HelpBbBanner,
    /// busybox 符号链接入口：同正文、无 banner。
    HelpBbBare,
    /// coreutils 的 `--usage`：单行压缩形式。
    UsageOneLine,
    /// `-h`：applet 照常跑，stdout 里没有 usage。
    ShortHNotHelp,
    /// 无开关操作数：applet 照常跑。
    OperandRun,
    /// busybox 不认 `--usage`：落用法面（逐字归 F11669）。
    UsageUnsupported,
    /// 未注册 applet：`<名>: applet not found` + 127（归 F11674）。
    NotOurApplet,
    /// 家族混认：不产出任何正文。
    MixedNoText,
    /// 正文被截断：只看到首行。
    TruncatedBody,
    /// 帮助落在 stderr（通道误读）。
    HelpOnStderr,
    /// 真的一字不出（表缺格或坏形状）。
    SilentHelp,
}

impl Note {
    /// 档名（明细行里逐字出现）。
    pub fn name(self) -> &'static str {
        match self {
            Note::HelpVerbatim => "help-verbatim",
            Note::HelpBbBanner => "help-bb-banner",
            Note::HelpBbBare => "help-bb-bare",
            Note::UsageOneLine => "usage-oneline",
            Note::ShortHNotHelp => "short-h-not-help",
            Note::OperandRun => "operand-run",
            Note::UsageUnsupported => "usage-unsupported",
            Note::NotOurApplet => "not-our-applet",
            Note::MixedNoText => "mixed-no-text",
            Note::TruncatedBody => "truncated-body",
            Note::HelpOnStderr => "help-on-stderr",
            Note::SilentHelp => "silent-help",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Note::HelpVerbatim => 0usize,
            Note::HelpBbBanner => 1usize,
            Note::HelpBbBare => 2usize,
            Note::UsageOneLine => 3usize,
            Note::ShortHNotHelp => 4usize,
            Note::OperandRun => 5usize,
            Note::UsageUnsupported => 6usize,
            Note::NotOurApplet => 7usize,
            Note::MixedNoText => 8usize,
            Note::TruncatedBody => 9usize,
            Note::HelpOnStderr => 10usize,
            Note::SilentHelp => 11usize,
        }
    }

    /// 全集。
    pub fn all() -> [Note; 12] {
        [
            Note::HelpVerbatim,
            Note::HelpBbBanner,
            Note::HelpBbBare,
            Note::UsageOneLine,
            Note::ShortHNotHelp,
            Note::OperandRun,
            Note::UsageUnsupported,
            Note::NotOurApplet,
            Note::MixedNoText,
            Note::TruncatedBody,
            Note::HelpOnStderr,
            Note::SilentHelp,
        ]
    }

    /// 注解总数。
    pub const COUNT: usize = 12;

    /// 兜底位：这一格**没有结论**（未名跑到别的卡、或一个字都没出）。
    ///
    /// `-h` / 操作数 / busybox 不支持 `--usage` 都不算兜底——它们有确定的
    /// applet 侧结论（跑了活或退了用法码），只是没有 usage 正文。
    pub fn defaulted(self) -> bool {
        matches!(self, Note::NotOurApplet | Note::SilentHelp)
    }

    /// 这一格有没有可对账的 usage 正文。
    pub fn has_body(self) -> bool {
        matches!(
            self,
            Note::HelpVerbatim | Note::HelpBbBanner | Note::HelpBbBare | Note::UsageOneLine
        )
    }

    /// 这一格是不是"帮助成功"（stdout + 退 0 + 有正文）。
    pub fn help_ok(self) -> bool {
        self.has_body()
    }

    /// 这一格会不会打错误行（红档面）。
    pub fn speaks_error(self) -> bool {
        matches!(
            self,
            Note::UsageUnsupported
                | Note::NotOurApplet
                | Note::MixedNoText
                | Note::TruncatedBody
                | Note::HelpOnStderr
        )
    }

    /// 这一格是不是"applet 照常跑"（不是帮助，也不是错）。
    pub fn runs_applet(self) -> bool {
        matches!(self, Note::ShortHNotHelp | Note::OperandRun)
    }

    /// 这一格是否落 coreutils 线。
    pub fn is_gnu(self) -> bool {
        matches!(self, Note::HelpVerbatim | Note::UsageOneLine)
    }

    /// 这一格是否落 busybox 线。
    pub fn is_busybox(self) -> bool {
        matches!(self, Note::HelpBbBanner | Note::HelpBbBare | Note::UsageUnsupported)
    }

    /// 这一格是否需要 coreutils 尾注。
    pub fn needs_trailer(self) -> bool {
        matches!(self, Note::HelpVerbatim)
    }
}

/// 四档：按**实际结论**归档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 一档：coreutils 线逐字正文（含单行 `--usage` 与无 banner 的 busybox 正文）。
    Verbatim,
    /// 二档：多调用入口，banner 在前，正文同尺。
    Bannered,
    /// 三档：没有 usage 正文（`-h`、操作数、未名、不支持 `--usage`）。
    NoBody,
    /// 四档：拒收（混认、截断、通道误读、谎报形状）。
    Refused,
}

impl Tier {
    /// 档名。
    pub fn name(self) -> &'static str {
        match self {
            Tier::Verbatim => "verbatim",
            Tier::Bannered => "bannered",
            Tier::NoBody => "nobody",
            Tier::Refused => "refused",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Tier::Verbatim => 0usize,
            Tier::Bannered => 1usize,
            Tier::NoBody => 2usize,
            Tier::Refused => 3usize,
        }
    }

    /// 四档全表。
    pub fn all() -> [Tier; TIER_COUNT] {
        [Tier::Verbatim, Tier::Bannered, Tier::NoBody, Tier::Refused]
    }

    /// 该档是否算"有正文可对账"的两档。
    pub fn has_text(self) -> bool {
        matches!(self, Tier::Verbatim | Tier::Bannered)
    }

    /// 由一次结果归档；四档互斥且全覆盖。
    pub fn of(o: &Outcome) -> Tier {
        if !o.consistent {
            return Tier::Refused;
        }
        if !o.note.has_body() {
            return Tier::NoBody;
        }
        if matches!(o.note, Note::HelpBbBanner) {
            return Tier::Bannered;
        }
        Tier::Verbatim
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
    /// 正文行数错。
    Value,
    /// 文本家族档错。
    Note,
    /// 兜底锚错。
    Default,
}

impl Fault {
    /// 本维错误码（`Ok` 给 0）。
    pub const fn code(self) -> u32 {
        match self {
            Fault::Ok => 0u32,
            Fault::Estate => AH_ESTATE,
            Fault::Stage => AH_ESTAGE,
            Fault::Code => AH_ECODE,
            Fault::Value => AH_EVALUE,
            Fault::Note => AH_ENOTE,
            Fault::Default => AH_EDEFAULT,
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

    /// 判序位次（`Ok` 排最后）。
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
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

/// 六个缺陷码（与判序一一对应，两两互异）。
pub const REJECT_CODES: [u32; REJECT] = [
    AH_ESTATE,
    AH_ESTAGE,
    AH_ECODE,
    AH_EVALUE,
    AH_ENOTE,
    AH_EDEFAULT,
];

// ---------------------------------------------------------------------------
// usage 表：五个 applet × 两条文本线，逐字快照
// ---------------------------------------------------------------------------

/// 一格 usage 快照：首行 + 正文（首行之后全部行）。
#[derive(Clone, Copy)]
pub struct UsageCell {
    /// 所属 applet。
    pub applet: Applet,
    /// 所属文本线。
    pub line: UsageLine,
    /// 首行（逐字，含 `Usage: ` 前缀）。
    pub first: &'static str,
    /// 正文（首行之后的行，含空行）。
    pub body: &'static str,
    /// 是否附 coreutils 尾注三行。
    pub trailer: bool,
}

/// 基准快照表（`LC_ALL=C`，coreutils 8.32 / BusyBox 1.36.1）。
pub const USAGE_TABLE: [UsageCell; USAGE_CELLS] = [
    UsageCell {
        applet: Applet::Ls,
        line: UsageLine::Gnu,
        first: "Usage: ls [OPTION]... [FILE]...",
        body: "List information about the FILEs (the current directory by default).\n\
Sort entries alphabetically if none of -cftuvSUX nor --sort is specified.\n\
\n\
Mandatory arguments to long options are mandatory for short options too.\n\
  -a, --all                  do not ignore entries starting with .\n\
  -A, --almost-all           do not list implied . and ..\n\
  -h, --human-readable       with -l and -s, print sizes like 1K 234M 2G etc.\n\
      --help     display this help and exit\n\
      --version  output version information and exit",
        trailer: true,
    },
    UsageCell {
        applet: Applet::Ls,
        line: UsageLine::Busybox,
        first: "Usage: ls [-Z] [-1AaCxdLHRFplinshrSXvctu] [-w WIDTH] [FILE]...",
        body: "List directory contents\n\
\n\
\t-1\tOne column output\n\
\t-a\tInclude entries which start with .\n\
\t-A\tLike -a, but exclude . and ..\n\
\t-h\tList sizes in human readable format (1K 243M 2G)\n\
\t--help\tDisplay this help and exit",
        trailer: false,
    },
    UsageCell {
        applet: Applet::Cp,
        line: UsageLine::Gnu,
        first: "Usage: cp [OPTION]... [-T] SOURCE DEST",
        body: "  or:  cp [OPTION]... SOURCE... DIRECTORY\n\
  or:  cp [OPTION]... -t DIRECTORY SOURCE...\n\
Copy SOURCE to DEST, or multiple SOURCE(s) to DIRECTORY.\n\
\n\
Mandatory arguments to long options are mandatory for short options too.\n\
  -a, --archive                same as -dR --preserve=all\n\
  -f, --force                  if an existing destination file cannot be\n\
                                 opened, remove it and try again\n\
      --help     display this help and exit\n\
      --version  output version information and exit",
        trailer: true,
    },
    UsageCell {
        applet: Applet::Cp,
        line: UsageLine::Busybox,
        first: "Usage: cp [OPTIONS] SOURCE DEST",
        body: "Copy SOURCE(s) to DEST\n\
\n\
\t-a\tSame as -dpR\n\
\t-R,-r\tRecurse\n\
\t-d,-P\tPreserve symlinks (default if -R)\n\
\t--help\tDisplay this help and exit",
        trailer: false,
    },
    UsageCell {
        applet: Applet::Head,
        line: UsageLine::Gnu,
        first: "Usage: head [OPTION]... [FILE]...",
        body: "Print the first 10 lines of each FILE to standard output.\n\
With more than one FILE, precede each with a header giving the file name.\n\
\n\
Mandatory arguments to long options are mandatory for short options too.\n\
  -c, --bytes=[-]NUM       print the first NUM bytes of each file\n\
  -n, --lines=[-]NUM       print the first NUM lines instead of the first 10\n\
  -q, --quiet, --silent    never print headers giving file names\n\
      --help     display this help and exit\n\
      --version  output version information and exit",
        trailer: true,
    },
    UsageCell {
        applet: Applet::Head,
        line: UsageLine::Busybox,
        first: "Usage: head [OPTIONS]... [FILE]...",
        body: "Print first 10 lines of each FILE to standard output.\n\
With more than one FILE, precede each with a header giving the file name.\n\
\n\
\t-n N\tPrint first N lines instead of first 10\n\
\t-c N\tOutput the first N bytes\n\
\t-q\tNever print headers\n\
\t-v\tAlways print headers\n\
\t--help\tDisplay this help and exit",
        trailer: false,
    },
    UsageCell {
        applet: Applet::Tail,
        line: UsageLine::Gnu,
        first: "Usage: tail [OPTION]... [FILE]...",
        body: "Print the last 10 lines of each FILE to standard output.\n\
With more than one FILE, precede each with a header giving the file name.\n\
\n\
Mandatory arguments to long options are mandatory for short options too.\n\
  -c, --bytes=[+]NUM       output the last NUM bytes\n\
  -f, --follow[={name|descriptor}]  output appended data as the file grows\n\
  -n, --lines=[+]NUM       output the last NUM lines, instead of the last 10\n\
      --help     display this help and exit\n\
      --version  output version information and exit",
        trailer: true,
    },
    UsageCell {
        applet: Applet::Tail,
        line: UsageLine::Busybox,
        first: "Usage: tail [OPTIONS]... [FILE]...",
        body: "Print last 10 lines of each FILE to standard output.\n\
With more than one FILE, precede each with a header giving the file name.\n\
\n\
\t-c N[kbm]\tOutput the last N bytes\n\
\t-f\tOutput data as the file grows\n\
\t-n N[kbm]\tPrint last N lines instead of last 10\n\
\t--help\tDisplay this help and exit",
        trailer: false,
    },
    UsageCell {
        applet: Applet::Wc,
        line: UsageLine::Gnu,
        first: "Usage: wc [OPTION]... [FILE]...",
        body: "  or:  wc [OPTION]... --files0-from=F\n\
Print newline, word, and byte counts for each FILE, and a total line if\n\
more than one FILE is specified.\n\
\n\
  -c, --bytes            print the byte counts\n\
  -l, --lines            print the newline counts\n\
  -m, --chars            print the character counts\n\
  -w, --words            print the word counts\n\
      --help     display this help and exit\n\
      --version  output version information and exit",
        trailer: true,
    },
    UsageCell {
        applet: Applet::Wc,
        line: UsageLine::Busybox,
        first: "Usage: wc [-clmwL] [FILE]...",
        body: "Count lines, words, and bytes for each FILE (or stdin)\n\
\n\
\t-c\tCount bytes\n\
\t-l\tCount newlines\n\
\t-m\tCount characters\n\
\t-w\tCount words\n\
\t-L\tPrint the length of the longest line\n\
\t--help\tDisplay this help and exit",
        trailer: false,
    },
];

/// 兜底格（取不到时用来判错的空快照，绝不冒充真快照）。
pub const CELL_NONE: UsageCell = UsageCell {
    applet: Applet::Unknown,
    line: UsageLine::Mixed,
    first: "",
    body: "",
    trailer: false,
};

/// 取一格 usage 快照（逐格扫表，不用下标）。
pub fn usage_cell(a: Applet, l: UsageLine) -> Option<&'static UsageCell> {
    let mut i = 0usize;
    while i < USAGE_CELLS {
        if let Some(c) = USAGE_TABLE.get(i) {
            if c.applet == a && c.line == l {
                return Some(c);
            }
        }
        i += 1;
    }
    None
}

/// 取首行（取不到给空串）。
pub fn usage_first(a: Applet, l: UsageLine) -> &'static str {
    match usage_cell(a, l) {
        Some(c) => c.first,
        None => "",
    }
}

/// 取正文（取不到给空串）。
pub fn usage_body(a: Applet, l: UsageLine) -> &'static str {
    match usage_cell(a, l) {
        Some(c) => c.body,
        None => "",
    }
}

/// coreutils 的 `--usage` 单行（每 applet 一行，逐字）。
pub fn usage_oneline(a: Applet) -> &'static str {
    match a {
        Applet::Ls => "Usage: ls [-abcCdfghilmnopqrstuvwxyz] [--block-size=SIZE] [--color=WHEN] [--format=WORD] [--hide=PATTERN] [--indicator-style=WORD] [--quoting-style=WORD] [--sort=WORD] [--time=WORD] [--time-style=STYLE] [--width=COLS] [FILE]...",
        Applet::Cp => "Usage: cp [-abdfilpPrRsuvx] [-S SUFFIX] [-V METHOD] [--backup[=CONTROL]] [--preserve[=ATTR_LIST]] [--reflink[=WHEN]] [--sparse=WHEN] [--help] [--version] SOURCE DEST",
        Applet::Head => "Usage: head [-n NUM] [-c NUM] [-qvz] [FILE]...",
        Applet::Tail => "Usage: tail [-n NUM] [-c NUM] [-q] [-f] [FILE]...",
        Applet::Wc => "Usage: wc [-clmw] [--files0-from=F] [FILE]...",
        Applet::Unknown => "Usage: ",
    }
}

/// coreutils 尾注三行（第二、三行含 applet 名）。
pub fn trailer_text(a: Applet) -> String {
    let mut s = String::from(TRAILER_HEAD);
    s.push_str("Full documentation <https://www.gnu.org/software/coreutils/");
    s.push_str(a.name());
    s.push_str(">\nor available locally via: info '(coreutils) ");
    s.push_str(a.name());
    s.push_str(" invocation'\n");
    s
}

/// 多调用入口的 banner（含其后空行）。
pub fn banner_text() -> &'static str {
    BANNER_TEXT
}

/// 未名 applet 的逐字文案：`<名>: applet not found`（模板归 F11674）。
pub fn msg_not_our_applet(a: Applet) -> String {
    let mut s = String::from(a.name());
    s.push_str(": applet not found");
    s
}

/// busybox 不认 `--usage` 的那一行（正文逐字归 F11669，本卡只钉码与"无正文"）。
pub fn msg_usage_unsupported(a: Applet) -> String {
    let mut s = String::from(a.name());
    s.push_str(": unrecognized option: usage");
    s
}

// ---------------------------------------------------------------------------
// 正文拼装
// ---------------------------------------------------------------------------

/// 这一格会不会打 banner：只有 busybox 线的多调用入口会，且必须真在打正文。
pub fn banner_shown(s: &Shape) -> bool {
    s.line == UsageLine::Busybox && s.entry == Entry::MultiCall && s.form == HelpForm::LongHelp
}

/// 这一格会不会附 coreutils 尾注：只有 coreutils 线长篇会。
pub fn trailer_shown(s: &Shape) -> bool {
    s.line == UsageLine::Gnu
        && s.form == HelpForm::LongHelp
        && usage_cell(s.applet, s.line).map(|c| c.trailer).unwrap_or(false)
}

/// 只留首行（截断档的现场形态）。
fn first_line_only(s: &str) -> String {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut cut = b.len();
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b'\n' {
                cut = i;
                break;
            }
        }
        i += 1;
    }
    let mut out = String::new();
    let mut k = 0usize;
    while k < cut {
        if let Some(c) = s.get(k..k + 1) {
            out.push_str(c);
        }
        k += 1;
    }
    out.push('\n');
    out
}

/// 拼装帮助全文（形状决定入口/家族/形式；取不到格给空串）。
pub fn render_help(s: &Shape) -> String {
    let mut out = String::new();
    match s.form {
        HelpForm::LongHelp => {
            let cell = match usage_cell(s.applet, s.line) {
                Some(c) => c,
                None => return out,
            };
            if banner_shown(s) {
                out.push_str(banner_text());
            }
            out.push_str(cell.first);
            out.push('\n');
            out.push_str(cell.body);
            out.push('\n');
            if cell.trailer {
                out.push_str(trailer_text(s.applet).as_str());
            }
            if s.truncated {
                return first_line_only(out.as_str());
            }
        }
        HelpForm::LongUsage => {
            if s.line == UsageLine::Gnu {
                out.push_str(usage_oneline(s.applet));
                out.push('\n');
            }
        }
        HelpForm::ShortH | HelpForm::Operand => {}
    }
    out
}

/// 帮助文本的指纹（逐字对账用）。
pub fn help_seal(a: Applet, l: UsageLine, e: Entry, f: HelpForm) -> u64 {
    let s = Shape {
        applet: a,
        line: l,
        entry: e,
        form: f,
        ..Shape::blank()
    };
    let text = render_help(&s);
    let mut h = fold_str(FNV_OFF, text.as_str());
    h = fold_word(h, lines_of(text.as_str()) as u64);
    h = fold_word(h, bytes_of(text.as_str()) as u64);
    h
}

// ---------------------------------------------------------------------------
// Shape：一次帮助现场能读到的全部维度
// ---------------------------------------------------------------------------

/// 一格输入形状。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// 哪个 applet。
    pub applet: Applet,
    /// 哪条文本线。
    pub line: UsageLine,
    /// 从哪个入口进来。
    pub entry: Entry,
    /// 请求的帮且形式。
    pub form: HelpForm,
    /// 记录者观测到的输出通道。
    pub channel: Channel,
    /// 申报的退出码（valid 只查档界）。
    pub exit_declared: u32,
    /// 记录者声称看到的 banner。
    pub banner_seen: bool,
    /// 记录者声称看到的 coreutils 尾注。
    pub trailer_seen: bool,
    /// 正文是否被截断（只留首行）。
    pub truncated: bool,
    /// `--help` 之后还跟了操作数（帮助仍然先出，进程不退到操作数上）。
    pub operand_after: bool,
    /// 记录是否在 `LC_ALL=C` 下采集（逐字快照的前提）。
    pub lang_c: bool,
    /// 输出是否接到终端（帮助文本不随 tty 变，留作画像校验）。
    pub tty: bool,
}

impl Shape {
    /// 基型：一个全绿的 coreutils `ls --help`。
    pub const fn blank() -> Shape {
        Shape {
            applet: Applet::Ls,
            line: UsageLine::Gnu,
            entry: Entry::Symlink,
            form: HelpForm::LongHelp,
            channel: Channel::Stdout,
            exit_declared: EXIT_OK,
            banner_seen: false,
            trailer_seen: true,
            truncated: false,
            operand_after: false,
            lang_c: true,
            tty: false,
        }
    }

    /// 形状整体是否自洽。不自洽不改变落点，只把 `consistent` 压假。
    pub fn valid(&self) -> bool {
        if self.exit_declared > EXIT_NOTFOUND {
            return false;
        }
        if !self.lang_c {
            return false;
        }
        // 多调用入口只存在于 busybox 线：coreutils 没有多调用二进制。
        if self.entry == Entry::MultiCall && self.line != UsageLine::Busybox {
            return false;
        }
        // coreutils 线永不打 banner；busybox 线永不带 coreutils 尾注。
        if self.line == UsageLine::Gnu && self.banner_seen {
            return false;
        }
        if self.line == UsageLine::Busybox && self.trailer_seen {
            return false;
        }
        // `--usage` 不带操作数（coreutils 的 usage 面不吃操作数）。
        if self.form == HelpForm::LongUsage && self.operand_after {
            return false;
        }
        // 截断只可能是长篇采集被切断。
        if self.truncated && self.form != HelpForm::LongHelp {
            return false;
        }
        // 混认不给正文，因此也不许声称看到 banner/尾注。
        if self.line == UsageLine::Mixed && (self.banner_seen || self.trailer_seen) {
            return false;
        }
        true
    }

    /// 只换 applet。
    pub fn with_applet(&self, a: Applet) -> Shape {
        let mut s = *self;
        s.applet = a;
        s
    }

    /// 只换文本线。
    pub fn with_line(&self, l: UsageLine) -> Shape {
        let mut s = *self;
        s.line = l;
        s
    }

    /// 只换入口。
    pub fn with_entry(&self, e: Entry) -> Shape {
        let mut s = *self;
        s.entry = e;
        s
    }

    /// 只换形式。
    pub fn with_form(&self, f: HelpForm) -> Shape {
        let mut s = *self;
        s.form = f;
        s
    }

    /// 形状指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.applet.index() as u64);
        h = fold_word(h, self.line.index() as u64);
        h = fold_word(h, self.entry.index() as u64);
        h = fold_word(h, self.form.index() as u64);
        h = fold_word(h, self.channel.index() as u64);
        h = fold_word(h, self.exit_declared as u64);
        h = fold_bool(h, self.banner_seen);
        h = fold_bool(h, self.trailer_seen);
        h = fold_bool(h, self.truncated);
        h = fold_bool(h, self.operand_after);
        h = fold_bool(h, self.lang_c);
        fold_bool(h, self.tty)
    }
}

/// 一次帮助现场的**实际**结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 落点闸。
    pub stage: Stage,
    /// 退出码。
    pub code: i32,
    /// 正文行数（无正文为 0）。
    pub value: u32,
    /// 文本家族档。
    pub note: Note,
    /// 兜底位：这一格没有可对账的正文。
    pub default: bool,
    /// 结构自洽：谎报的形状在这里被压假。
    pub consistent: bool,
}

impl Outcome {
    /// 全零初值（取不到任何现场时的形态）。
    pub const fn blank() -> Outcome {
        Outcome {
            stage: Stage::Exit,
            code: 0i32,
            value: 0u32,
            note: Note::SilentHelp,
            default: true,
            consistent: true,
        }
    }

    /// 组装一格结论。
    pub const fn make(
        stage: Stage,
        code: u32,
        value: u32,
        note: Note,
        default: bool,
        consistent: bool,
    ) -> Outcome {
        Outcome {
            stage,
            code: code as i32,
            value,
            note,
            default,
            consistent,
        }
    }

    /// 是否绿（成败互报按这个读，比 `code == 0` 更严）。
    pub fn ok(&self) -> bool {
        self.code == 0 && self.consistent
    }

    /// 是否红。
    pub fn failed(&self) -> bool {
        self.code != 0
    }

    /// 是否落进兜底档。
    pub fn anchored(&self) -> bool {
        self.default
    }

    /// 是否拒收（结构上不自洽）。
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

    /// 帮助成功是否成立（正文非空 + 码 0 + 自洽）。
    pub fn help_ok(&self) -> bool {
        self.ok() && self.note.has_body() && self.value > 0
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
    /// 申报读成绿还是红。
    pub success: bool,
    /// 申报落在哪道闸。
    pub stage: Stage,
    /// 申报的退出码。
    pub code: i32,
    /// 申报的正文行数。
    pub value: u32,
    /// 申报的文本家族档。
    pub note: Note,
    /// 申报的兜底位。
    pub default: bool,
}

impl Claim {
    /// 全零初值。
    pub const fn blank() -> Claim {
        Claim {
            success: true,
            stage: Stage::Exit,
            code: 0i32,
            value: 0u32,
            note: Note::HelpVerbatim,
            default: false,
        }
    }

    /// 组装一份申报。
    pub const fn make(
        success: bool,
        stage: Stage,
        code: i32,
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

    /// 申报读成绿还是红。
    pub fn success(&self) -> bool {
        self.success
    }

    /// 申报指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_bool(h, self.success);
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
            tag: "f11668-blank",
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
// define：把形状过七道闸
// ---------------------------------------------------------------------------

/// **本卡的纵线**：七道闸按秩走，前一闸不产出事实，后一闸就没有输入。
pub fn define(s: &Shape) -> Outcome {
    let mut consistent = s.valid();

    // --- 闸 1：dispatch ----------------------------------------------------
    if !s.applet.judged() {
        // 未注册名：`<名>: applet not found` + 127，逐字与码都归 F11674。
        return Outcome::make(
            Stage::Dispatch,
            EXIT_NOTFOUND,
            0,
            Note::NotOurApplet,
            true,
            consistent,
        );
    }

    // --- 闸 2：form --------------------------------------------------------
    match s.form {
        HelpForm::ShortH => {
            // `-h` 是 applet 自己的开关，进程照常跑，stdout 里没有 usage。
            return Outcome::make(
                Stage::Form,
                EXIT_OK,
                0,
                Note::ShortHNotHelp,
                false,
                consistent,
            );
        }
        HelpForm::Operand => {
            return Outcome::make(
                Stage::Form,
                EXIT_OK,
                0,
                Note::OperandRun,
                false,
                consistent,
            );
        }
        _ => {}
    }

    // --- 闸 3：line --------------------------------------------------------
    if !s.line.produces_text() {
        // 混认：没有快照，一个字都不许出，整格判死。
        return Outcome::make(
            Stage::Line,
            EXIT_FAIL,
            0,
            Note::MixedNoText,
            false,
            false,
        );
    }
    if s.form == HelpForm::LongUsage && !s.line.has_usage_flag() {
        // busybox 长项表里没有 `--usage`：落用法面，退 1，无正文。
        return Outcome::make(
            Stage::Line,
            EXIT_FAIL,
            0,
            Note::UsageUnsupported,
            false,
            consistent,
        );
    }

    // --- 闸 4：banner ------------------------------------------------------
    let real_banner = banner_shown(s);
    let real_trailer = trailer_shown(s);
    if real_banner != s.banner_seen {
        consistent = false;
    }
    if real_trailer != s.trailer_seen {
        consistent = false;
    }

    // --- 闸 5：body --------------------------------------------------------
    if s.truncated {
        // 只看到首行的采集不许当逐字现场。
        let cut = render_help(s);
        return Outcome::make(
            Stage::Body,
            EXIT_OK,
            lines_of(cut.as_str()),
            Note::TruncatedBody,
            false,
            false,
        );
    }
    let text = render_help(s);
    let lines = lines_of(text.as_str());
    if lines == 0 {
        return Outcome::make(Stage::Body, EXIT_OK, 0, Note::SilentHelp, true, false);
    }
    if lines > BODY_LINE_CAP {
        return Outcome::make(Stage::Body, EXIT_OK, lines, Note::TruncatedBody, false, false);
    }

    // --- 闸 6：channel -----------------------------------------------------
    if s.channel != Channel::Stdout {
        // 帮助恒走 stdout；落到别处就是通道误读档。
        return Outcome::make(
            Stage::Channel,
            EXIT_FAIL,
            lines,
            Note::HelpOnStderr,
            false,
            false,
        );
    }

    // --- 闸 7：exit --------------------------------------------------------
    let note = match (s.form, s.line, real_banner) {
        (HelpForm::LongUsage, _, _) => Note::UsageOneLine,
        (_, UsageLine::Busybox, true) => Note::HelpBbBanner,
        (_, UsageLine::Busybox, false) => Note::HelpBbBare,
        _ => Note::HelpVerbatim,
    };
    Outcome::make(Stage::Exit, EXIT_OK, lines, note, false, consistent)
}

// ---------------------------------------------------------------------------
// 语料：八活六死，逐槽手写
// ---------------------------------------------------------------------------

/// 语料现场表。
pub const SHAPES: [Shape; CORPUS] = [
    // 0 母版锚：coreutils `ls --help`，首行 `[OPTION]...`，三行尾注。
    Shape {
        ..Shape::blank()
    },
    // 1 coreutils `cp --help`：首行带 `[-T] SOURCE DEST`，后接两段 `or:`。
    Shape {
        applet: Applet::Cp,
        ..Shape::blank()
    },
    // 2 coreutils `head --help`：`-n NUM` 是数字面，正文短一截。
    Shape {
        applet: Applet::Head,
        ..Shape::blank()
    },
    // 3 coreutils `tail --help`：`--follow[={name|descriptor}]` 是全表最长的选项行。
    Shape {
        applet: Applet::Tail,
        ..Shape::blank()
    },
    // 4 多调用入口 `busybox ls --help`：banner 三行在前。
    Shape {
        line: UsageLine::Busybox,
        entry: Entry::MultiCall,
        trailer_seen: false,
        banner_seen: true,
        ..Shape::blank()
    },
    // 5 多调用入口 `busybox cp --help`：banner + 最短的 busybox cp 正文。
    Shape {
        applet: Applet::Cp,
        line: UsageLine::Busybox,
        entry: Entry::MultiCall,
        trailer_seen: false,
        banner_seen: true,
        ..Shape::blank()
    },
    // 6 coreutils `ls --usage`：单行压缩形式，无尾注。
    Shape {
        form: HelpForm::LongUsage,
        trailer_seen: false,
        ..Shape::blank()
    },
    // 7 `ls -h`：不是帮助，进程照常跑，console 里一个 usage 字节都没有。
    Shape {
        line: UsageLine::Busybox,
        form: HelpForm::ShortH,
        trailer_seen: false,
        ..Shape::blank()
    },
    // 8 Estate：家族混认现场（拒收），申报却把成败读反。
    Shape {
        line: UsageLine::Mixed,
        trailer_seen: false,
        ..Shape::blank()
    },
    // 9 Stage：busybox 不认 `--usage`，落 line 闸；申报把落点说成 body 闸。
    Shape {
        line: UsageLine::Busybox,
        form: HelpForm::LongUsage,
        trailer_seen: false,
        ..Shape::blank()
    },
    // 10 Code：`head -h`（无正文）现场，申报的码不是 0。
    Shape {
        applet: Applet::Head,
        line: UsageLine::Busybox,
        form: HelpForm::ShortH,
        trailer_seen: false,
        ..Shape::blank()
    },
    // 11 Value：`wc` 操作数现场（无正文），申报的正文行数却是 3。
    Shape {
        applet: Applet::Wc,
        line: UsageLine::Busybox,
        form: HelpForm::Operand,
        trailer_seen: false,
        ..Shape::blank()
    },
    // 12 Note：`tail -h` 现场，申报的注解档说成"无开关操作数"。
    Shape {
        applet: Applet::Tail,
        line: UsageLine::Busybox,
        form: HelpForm::ShortH,
        trailer_seen: false,
        ..Shape::blank()
    },
    // 13 Default：未名 applet 现场（127，逐字归 F11674），申报却说"没落兜底"。
    Shape {
        applet: Applet::Unknown,
        ..Shape::blank()
    },
];

/// 申报基型：与活槽的实际结论逐格相同（coreutils `ls --help`）。
const GOOD_CLAIM: Claim = Claim {
    success: true,
    stage: Stage::Exit,
    code: 0,
    value: 13,
    note: Note::HelpVerbatim,
    default: false,
};

/// 十四格申报。前八格与 `define(SHAPES[i])` 逐格相同，后六格各坏一维。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim {
        ..GOOD_CLAIM
    },
    Claim {
        value: 14,
        ..GOOD_CLAIM
    },
    Claim {
        value: 13,
        ..GOOD_CLAIM
    },
    Claim {
        value: 13,
        ..GOOD_CLAIM
    },
    Claim {
        value: 10,
        note: Note::HelpBbBanner,
        ..GOOD_CLAIM
    },
    Claim {
        value: 9,
        note: Note::HelpBbBanner,
        ..GOOD_CLAIM
    },
    Claim {
        value: 1,
        note: Note::UsageOneLine,
        ..GOOD_CLAIM
    },
    Claim {
        success: true,
        stage: Stage::Form,
        code: 0,
        value: 0,
        note: Note::ShortHNotHelp,
        default: false,
    },
    // 8 成败互报：拒收现场（混认）被申报成帮助成功；其余五维照实现场。
    Claim {
        success: true,
        stage: Stage::Line,
        code: 1,
        value: 0,
        note: Note::MixedNoText,
        default: false,
    },
    // 9 阶段不符：落点是 line 闸，申报说成 body 闸；其余五维照实现场。
    Claim {
        success: false,
        stage: Stage::Body,
        code: 1,
        value: 0,
        note: Note::UsageUnsupported,
        default: false,
    },
    // 10 码档不符：无正文现场申报成码 2；其余五维照实现场。
    Claim {
        success: true,
        stage: Stage::Form,
        code: 2,
        value: 0,
        note: Note::ShortHNotHelp,
        default: false,
    },
    // 11 数值不符：无正文现场申报成正文三行；其余五维照实现场。
    Claim {
        success: true,
        stage: Stage::Form,
        code: 0,
        value: 3,
        note: Note::OperandRun,
        default: false,
    },
    // 12 注解不符：`-h` 现场申报成"无开关操作数"；其余五维照实现场。
    Claim {
        success: true,
        stage: Stage::Form,
        code: 0,
        value: 0,
        note: Note::OperandRun,
        default: false,
    },
    // 13 兜底不符：未名现场（127 兜底档）申报成"没落兜底"；其余五维照实现场。
    Claim {
        success: false,
        stage: Stage::Dispatch,
        code: 127,
        value: 0,
        note: Note::NotOurApplet,
        default: false,
    },
];

// ---------------------------------------------------------------------------
// 期望表（申报表的第二种形式，任一处笔误都要露出来）
// ---------------------------------------------------------------------------

/// 期望阶段（现场**真实**落到的闸，不是申报那一栏）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Form,
    Stage::Line,
    Stage::Line,
    Stage::Form,
    Stage::Form,
    Stage::Form,
    Stage::Dispatch,
];

/// 期望错误码（0 = 无码；第二形式，现场真实挂的码）。
pub const WANT_CODE: [u32; CORPUS] = [
    0, 0, 0, 0, 0, 0, 0, 0, EXIT_FAIL, EXIT_FAIL, 0, 0, 0, EXIT_NOTFOUND,
];

/// 期望正文行数（第二形式）。
pub const WANT_VALUE: [u32; CORPUS] = [13, 14, 13, 13, 10, 9, 1, 0, 0, 0, 0, 0, 0, 0];

/// 期望注解（第二形式）。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::HelpVerbatim,
    Note::HelpVerbatim,
    Note::HelpVerbatim,
    Note::HelpVerbatim,
    Note::HelpBbBanner,
    Note::HelpBbBanner,
    Note::UsageOneLine,
    Note::ShortHNotHelp,
    Note::MixedNoText,
    Note::UsageUnsupported,
    Note::ShortHNotHelp,
    Note::OperandRun,
    Note::ShortHNotHelp,
    Note::NotOurApplet,
];

/// 期望兜底位（第二形式）。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, false, false, false, false, false, false, false, false, false, true,
];

/// 期望缺陷维序位（0..5 六维，6 = 无缺陷）。
pub const WANT_NDX: [usize; CORPUS] = [6, 6, 6, 6, 6, 6, 6, 6, 0, 1, 2, 3, 4, 5];

/// 期望成败（第二形式）。
pub const WANT_SUCCESS: [bool; CORPUS] = [
    true, true, true, true, true, true, true, true, false, false, true, true, true, false,
];

/// 期望短名（标签必须挂本卡号）。
pub const WANT_TAGS: [&str; CORPUS] = [
    "f11668-ls-help-verbatim",
    "f11668-cp-help-verbatim",
    "f11668-head-help-verbatim",
    "f11668-tail-help-verbatim",
    "f11668-bb-ls-multicall",
    "f11668-bb-cp-multicall",
    "f11668-ls-usage-oneline",
    "f11668-ls-short-h-not-help",
    "f11668-estate-mixed",
    "f11668-stage-skip",
    "f11668-code-shift",
    "f11668-value-drift",
    "f11668-note-flip",
    "f11668-default-unjudged",
];

// ---------------------------------------------------------------------------
// 判定
// ---------------------------------------------------------------------------

/// 缺陷维 → 序位。
pub fn fault_ndx(f: Fault) -> usize {
    f.order()
}

/// 序位 → 缺陷维。
pub fn ndx_fault(n: usize) -> Option<Fault> {
    match n {
        0usize => Some(Fault::Estate),
        1usize => Some(Fault::Stage),
        2usize => Some(Fault::Code),
        3usize => Some(Fault::Value),
        4usize => Some(Fault::Note),
        5usize => Some(Fault::Default),
        6usize => Some(Fault::Ok),
        _ => None,
    }
}

/// 申报对判——固定次序 Estate → Stage → Code → Value → Note → Default → Ok。
///
/// 成败互报优先：帮助是"成功侧"判据，把它读成失败或把拒收读成成功，都是最重的
/// 那类谎，后面的维再对也不许掩盖。
pub fn judge(c: &Case) -> Fault {
    let a = c.actual();
    let k = &c.claim;
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
    // 兜底双向判死：现场真落兜底，无论怎么申报都不许当结论；现场没落兜底却
    // 申报"落了"，同样是把结论塞进兜底。
    if a.default || a.default != k.default {
        return Fault::Default;
    }
    Fault::Ok
}

/// 语料取槽。
pub fn case_at(i: usize) -> Option<Case> {
    match (SHAPES.get(i), CLAIMS.get(i), WANT_TAGS.get(i)) {
        (Some(s), Some(c), Some(t)) => Some(Case {
            shape: *s,
            claim: *c,
            tag: *t,
        }),
        _ => None,
    }
}

/// 全语料。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case::blank(); CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(c) = case_at(i) {
            if let Some(slot) = out.get_mut(i) {
                *slot = c;
            }
        }
        i += 1;
    }
    out
}

/// 覆盖校验：槽数必须落在语料范围内。
pub fn cover(n: usize) -> Result<(), u32> {
    if n > CORPUS {
        return Err(AH_ESLOT);
    }
    Ok(())
}

/// 第 `i` 槽期望的缺陷维。
pub fn expected(i: usize) -> Option<Fault> {
    match WANT_NDX.get(i) {
        Some(n) => ndx_fault(*n),
        None => None,
    }
}

/// 反例区第 `i` 槽期望的缺陷维（取不到按 `Ok`，绝不冒充某一维）。
fn expected_reject_ndx(i: usize) -> Fault {
    match expected(i) {
        Some(f) => f,
        None => Fault::Ok,
    }
}

// ---------------------------------------------------------------------------
// 档位与矩阵
// ---------------------------------------------------------------------------

/// 一档的统计行：`total` 与 `good` 两维独立。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Row {
    /// 档位。
    pub tier: Tier,
    /// 该档扫过的槽数。
    pub total: usize,
    /// 该档判对的槽数。
    pub good: usize,
}

impl Row {
    /// 该行是否全对。
    pub fn perfect(&self) -> bool {
        self.total == self.good
    }

    /// 该行红格数。
    pub fn red(&self) -> usize {
        self.total.saturating_sub(self.good)
    }

    /// 呈现一行。
    pub fn render(&self) -> String {
        let mut s = String::from(self.tier.name());
        s.push('=');
        s.push_str(dec_text(self.good as u32).as_str());
        s.push('/');
        s.push_str(dec_text(self.total as u32).as_str());
        s
    }
}

/// 四档矩阵：横看语料槽，纵看档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Matrix {
    rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 由语料构造。
    pub fn build() -> Matrix {
        let table = Tier::all();
        let mut rows = [Row {
            tier: Tier::Verbatim,
            total: 0,
            good: 0,
        }; TIER_COUNT];
        let mut t = 0usize;
        while t < TIER_COUNT {
            let tier = match table.get(t) {
                Some(x) => *x,
                None => return Matrix { rows },
            };
            let mut total = 0usize;
            let mut good = 0usize;
            let mut i = 0usize;
            while i < CORPUS {
                let c = match case_at(i) {
                    Some(x) => x,
                    None => return Matrix { rows },
                };
                total += 1;
                let landed = Tier::of(&c.actual());
                let hit = if i < ACCEPT {
                    landed == tier && c.ok()
                } else {
                    landed == tier && !tier.has_text() && c.fault() == expected_reject_ndx(i)
                };
                if hit {
                    good += 1;
                }
                i += 1;
            }
            if let Some(slot) = rows.get_mut(t) {
                *slot = Row { tier, total, good };
            }
            t += 1;
        }
        Matrix { rows }
    }

    /// 取一档。
    pub fn row(&self, t: Tier) -> Option<Row> {
        let table = Tier::all();
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(x) = table.get(i) {
                if *x == t {
                    if let Some(r) = self.rows.get(i) {
                        return Some(*r);
                    }
                }
            }
            i += 1;
        }
        None
    }

    /// 扫过的格总数。
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

    /// 判对的格总数。
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

    /// 全对的行数。
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

    /// 红格总数。
    pub fn red_total(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n += r.red();
            }
            i += 1;
        }
        n
    }

    /// 四档是否都有落点（空档说明归档逻辑漏了形态）。
    pub fn covered(&self) -> bool {
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if r.good == 0 {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 呈现矩阵。
    pub fn render(&self) -> String {
        let mut s = String::from("matrix ");
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if i > 0 {
                    s.push(' ');
                }
                s.push_str(r.render().as_str());
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 结算报告
// ---------------------------------------------------------------------------

/// 全绿渲染文本（逐字钉住，改一个字都要红）。
pub const RENDER_OK: &str =
    "applet-help corpus=14 accept=8 reject=6 verbatim=5 bannered=2 nobody=6 refused=1 match=14 rate=1000permille\n";

/// 帮助批结算报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Report {
    /// 语料分母。
    pub total: usize,
    /// 正例槽数。
    pub accept: usize,
    /// 反例槽数。
    pub reject: usize,
    /// 逐字正文档计数。
    pub verbatim: usize,
    /// 带 banner 档计数。
    pub bannered: usize,
    /// 无正文档计数。
    pub nobody: usize,
    /// 拒收档计数。
    pub refused: usize,
    /// 判对槽数。
    pub matched: usize,
}

impl Report {
    /// 一致率千分比。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.matched as u32 * RATE_FULL + (self.total as u32) / 2) / (self.total as u32)
    }

    /// 是否全绿。
    pub fn green(&self) -> bool {
        self.accept == ACCEPT
            && self.reject == REJECT
            && self.total == CORPUS
            && self.tier_cells() == CORPUS as u32
            && self.matched == self.total
            && self.rate() == RATE_FULL
    }

    /// 红项计数。
    pub fn reds(&self) -> u32 {
        (self.total.saturating_sub(self.matched)) as u32
    }

    /// 闸门：全绿才放行。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(AH_ENOBS);
        }
        if self.tier_cells() != self.total as u32 {
            return Err(AH_EMATRIX);
        }
        if self.matched > self.total {
            return Err(AH_EBAL);
        }
        if self.rate() < RATE_FULL {
            return Err(AH_EBAL);
        }
        Ok(())
    }

    /// 四档格数。
    pub fn tier_cells(&self) -> u32 {
        (self.verbatim as u32) + (self.bannered as u32) + (self.nobody as u32) + (self.refused as u32)
    }

    /// 逐字渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("applet-help corpus=");
        s.push_str(dec_text(self.accept as u32 + self.reject as u32).as_str());
        s.push_str(" accept=");
        s.push_str(dec_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(dec_text(self.reject as u32).as_str());
        s.push_str(" verbatim=");
        s.push_str(dec_text(self.verbatim as u32).as_str());
        s.push_str(" bannered=");
        s.push_str(dec_text(self.bannered as u32).as_str());
        s.push_str(" nobody=");
        s.push_str(dec_text(self.nobody as u32).as_str());
        s.push_str(" refused=");
        s.push_str(dec_text(self.refused as u32).as_str());
        s.push_str(" match=");
        s.push_str(dec_text(self.matched as u32).as_str());
        s.push_str(" rate=");
        s.push_str(dec_text(self.rate()).as_str());
        s.push_str("permille\n");
        s
    }
}

/// 空报告（未跑任何语料时的形态，绝不与全绿混淆）。
pub fn empty_report() -> Report {
    Report {
        total: 0,
        accept: 0,
        reject: 0,
        verbatim: 0,
        bannered: 0,
        nobody: 0,
        refused: 0,
        matched: 0,
    }
}

/// 由语料结算。
///
/// 分子只由"该对的槽对了几格"给出：正例槽必须判 `Ok`，反例槽必须判中它那一维。
/// 这样"该红的红"不污染一致率，"不该红的红了"一定把分子打下来。
fn settle(over: Option<(usize, Fault)>) -> Report {
    let mut rep = Report {
        total: CORPUS,
        accept: ACCEPT,
        reject: REJECT,
        verbatim: 0,
        bannered: 0,
        nobody: 0,
        refused: 0,
        matched: 0,
    };
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return empty_report(),
        };
        let actual = c.actual();
        match Tier::of(&actual) {
            Tier::Verbatim => rep.verbatim += 1,
            Tier::Bannered => rep.bannered += 1,
            Tier::NoBody => rep.nobody += 1,
            Tier::Refused => rep.refused += 1,
        }
        let landed = match over {
            Some((slot, fault)) => {
                if slot == i {
                    fault
                } else {
                    c.fault()
                }
            }
            None => c.fault(),
        };
        let want = if i < ACCEPT {
            Fault::Ok
        } else {
            expected_reject_ndx(i)
        };
        if landed == want {
            rep.matched += 1;
        }
        i += 1;
    }
    rep
}

/// 主结算：全绿才给报告，否则给闸门码。
pub fn run() -> Result<Report, u32> {
    let rep = settle(None);
    match rep.gate() {
        Ok(()) => Ok(rep),
        Err(code) => Err(code),
    }
}

/// 带注入的结算：越界槽给槽位码。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((slot, _)) = over {
        if slot >= CORPUS {
            return Err(AH_ESLOT);
        }
    }
    Ok(settle(over))
}

/// 报告与矩阵是否互相印证（两条独立路径给同一个分母）。
pub fn report_agrees_with_matrix() -> bool {
    let rep = settle(None);
    let m = Matrix::build();
    let verbatim_cells = match m.row(Tier::Verbatim) {
        Some(r) => r.good,
        None => 0,
    };
    let bannered_cells = match m.row(Tier::Bannered) {
        Some(r) => r.good,
        None => 0,
    };
    rep.total == CORPUS
        && m.slots() == CORPUS * TIER_COUNT
        && m.goods() == CORPUS
        && verbatim_cells == rep.verbatim
        && bannered_cells == rep.bannered
}

/// 红档率标定：注入一维红之后分子必须掉到 13/14，且不是被整除掩盖。
pub fn red_rate_calibrated() -> bool {
    let base = settle(None);
    let bad = match run_with(Some((2, Fault::Value))) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if bad.matched >= base.matched {
        return false;
    }
    if bad.total < 1 {
        return false;
    }
    let want = (13u32 * RATE_FULL + (bad.total as u32) / 2) / (bad.total as u32);
    want == RED_RATE && RED_RATE == 929
}

/// 注入一路红，四路自检必须各指各的。
pub fn inject_four() -> [bool; 4] {
    let table_route = {
        let mut ok = true;
        let mut i = 0usize;
        while i < APPLET_JUDGED {
            let a = match Applet::at(i) {
                Some(x) => x,
                None => return [false, false, false, false],
            };
            if usage_first(a, UsageLine::Gnu).is_empty() {
                ok = false;
            }
            if usage_first(a, UsageLine::Busybox).is_empty() {
                ok = false;
            }
            if usage_first(a, UsageLine::Gnu) == usage_first(a, UsageLine::Busybox) {
                ok = false;
            }
            i += 1;
        }
        ok && usage_cell(Applet::Unknown, UsageLine::Gnu).is_none()
    };
    let judge_route = {
        let mut all_ok = true;
        let mut i = 0usize;
        while i < ACCEPT {
            if let Some(c) = case_at(i) {
                if c.fault() != Fault::Ok {
                    all_ok = false;
                }
            }
            i += 1;
        }
        let mut k = ACCEPT;
        while k < CORPUS {
            if let Some(c) = case_at(k) {
                if c.fault() == Fault::Ok {
                    all_ok = false;
                }
                if c.fault() != expected_reject_ndx(k) {
                    all_ok = false;
                }
            }
            k += 1;
        }
        all_ok
    };
    let render_route = {
        let rep = settle(None);
        rep.green() && rep.render().as_str() == RENDER_OK
    };
    let ledger_route = {
        let mut l = build_ledger();
        match l.freeze() {
            Ok(h) => h != 0 && l.is_frozen() && l.replay() == Ok(h),
            Err(_) => false,
        }
    };
    [table_route, judge_route, render_route, ledger_route]
}

// ---------------------------------------------------------------------------
// 阶段闸门：Dispatch→Form→Line→Banner→Body→Channel→Exit
// ---------------------------------------------------------------------------

/// 一次帮助判据的走闸状态机。
///
/// 这条纵线为什么不能倒：形式（`--help` / `--usage` / `-h`）必须先辨，家族线
/// 必须先选，banner 必须先定，正文才拼得出来；通道与码档是结算项，只能在正文
/// 之后。一旦允许倒退，就能出现"先判成功再补形式"——一个 `-h` 现场被回填成
/// `--help` 成功档，本卡的判据当场失效。所以倒退判死（`AH_ESTAGE`），封账后
/// 再走一步也判死（`AH_ESTATE`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gate {
    /// 当前档；`None` 表示还没进过任何闸。
    current: Option<Stage>,
    /// 是否已封账。
    closed: bool,
    /// 成功走档次数。
    steps: u32,
    /// 被拒次数。
    refusals: u32,
    /// 走档轨迹指纹。
    root: u64,
}

impl Gate {
    /// 空闸门。
    pub fn new() -> Gate {
        Gate {
            current: None,
            closed: false,
            steps: 0,
            refusals: 0,
            root: FNV_OFF,
        }
    }

    /// 走一档。倒退即拒，且不改状态。
    pub fn step(&mut self, s: Stage) -> Result<(), u32> {
        if self.closed {
            self.refusals += 1;
            return Err(AH_ESTATE);
        }
        if let Some(cur) = self.current {
            if s.rank() < cur.rank() {
                self.refusals += 1;
                return Err(AH_ESTAGE);
            }
        }
        self.root = fold_word(self.root, s.rank() as u64);
        self.root = fold_str(self.root, s.name());
        self.current = Some(s);
        self.steps += 1;
        Ok(())
    }

    /// 封账：之后任何走档与再次封账都拒。
    pub fn close(&mut self) -> Result<u64, u32> {
        if self.closed {
            self.refusals += 1;
            return Err(AH_ESTATE);
        }
        if self.current.is_none() {
            return Err(AH_ESTAGE);
        }
        self.closed = true;
        self.root = fold_str(self.root, "closed");
        Ok(self.root)
    }

    /// 当前档。
    pub fn stage(&self) -> Option<Stage> {
        self.current
    }

    /// 成功走档次数。
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// 被拒次数。
    pub fn refusals(&self) -> u32 {
        self.refusals
    }

    /// 是否已封账。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 轨迹指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 依次走一串档。
    pub fn walk(stages: &[Stage]) -> Result<Gate, u32> {
        let mut g = Gate::new();
        let mut i = 0usize;
        while i < stages.len() {
            let s = match stages.get(i) {
                Some(x) => *x,
                None => return Err(AH_ESLOT),
            };
            match g.step(s) {
                Ok(()) => {}
                Err(code) => return Err(code),
            }
            i += 1;
        }
        Ok(g)
    }

    /// 全档正序走一遍并封账，返回指纹。
    pub fn full_pass() -> Result<u64, u32> {
        let table = Stage::all();
        let mut g = Gate::new();
        let mut i = 0usize;
        while i < table.len() {
            let s = match table.get(i) {
                Some(x) => *x,
                None => return Err(AH_ESLOT),
            };
            match g.step(s) {
                Ok(()) => {}
                Err(code) => return Err(code),
            }
            i += 1;
        }
        g.close()
    }

    /// 呈现闸门。
    pub fn render(&self) -> String {
        let mut s = String::from("gate stage=");
        match self.current {
            Some(cur) => s.push_str(cur.name()),
            None => s.push_str("none"),
        }
        s.push_str(" steps=");
        s.push_str(dec_text(self.steps).as_str());
        s.push_str(" refusals=");
        s.push_str(dec_text(self.refusals).as_str());
        s.push_str(" closed=");
        s.push_str(bool_text(self.closed));
        s
    }
}

// ---------------------------------------------------------------------------
// 账本：逐格登记本卡的统计量与缺陷码
// ---------------------------------------------------------------------------

/// 一格台账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    /// 槽号，必须严格等于登记时的当前长度。
    pub slot: u8,
    /// 这一格对应的算子号。
    pub op: u8,
    /// 这一格记的是成功还是败。
    pub ok: bool,
    /// 错误码（成功格必须为 0）。
    pub code: u32,
    /// 统计值（败格必须为 0）。
    pub value: u32,
}

impl Sample {
    /// 一格的指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_byte(h, self.slot);
        h = fold_byte(h, self.op);
        h = fold_bool(h, self.ok);
        h = fold_word(h, self.code as u64);
        fold_word(h, self.value as u64)
    }

    /// 呈现一格。
    pub fn render(&self) -> String {
        let mut s = String::from("slot ");
        s.push_str(dec_text(self.slot as u32).as_str());
        s.push_str(" op ");
        s.push_str(hex_text(self.op as u32, 2).as_str());
        s.push(' ');
        s.push_str(bool_text(self.ok));
        s.push_str(" code ");
        s.push_str(hex_text(self.code, 5).as_str());
        s.push_str(" value ");
        s.push_str(dec_text(self.value).as_str());
        s
    }
}

/// 账本：十四格 = 七统计格 + 一红档格 + 六缺陷码格。
///
/// 登记纪律：槽号必须严格等于当前长度，跳号与重号一律拒收并计数——本卡不许
/// "先出报告再补台账"，所以 `rejects` 是一个要判死的量。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    slots: [Option<Sample>; SAMPLES],
    len: usize,
    rejects: usize,
    frozen: bool,
    root: u64,
}

impl Ledger {
    /// 空账。
    pub fn new() -> Ledger {
        Ledger {
            slots: [None; SAMPLES],
            len: 0,
            rejects: 0,
            frozen: false,
            root: 0,
        }
    }

    /// 登记一格；槽号错位一律拒。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(AH_ESTATE);
        }
        if self.len >= SAMPLES {
            self.rejects += 1;
            return Err(AH_ESLOT);
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(AH_ESLOT);
        }
        if let Some(cell) = self.slots.get_mut(self.len) {
            *cell = Some(s);
        }
        self.len += 1;
        Ok(())
    }

    /// 已登记格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        match self.slots.get(i) {
            Some(x) => *x,
            None => None,
        }
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.sample(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 败格数。
    pub fn err_count(&self) -> usize {
        self.len.saturating_sub(self.ok_count())
    }

    /// 拒收次数。
    pub fn rejects(&self) -> usize {
        self.rejects
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 冻结后的指纹；未冻结时为 0。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 统计格应有的算子号。
    fn stat_op_want(&self, i: usize) -> Option<u8> {
        if i < STAT_CELLS {
            return match STAT_OPS.get(i) {
                Some(v) => Some(*v),
                None => None,
            };
        }
        if i == STAT_CELLS {
            return Some(OPID_STAT_RED);
        }
        None
    }

    /// 逐格复核：统计格对上 `VALS`、红档格对上 `RED_RATE`、缺陷格对上
    /// `REJECT_CODES` 且败格值必须为 0，缺陷码两两互异。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(AH_ESLOT);
        }
        let stat_last = match STAT_CELLS.checked_add(1) {
            Some(x) => x,
            None => return Err(AH_ESLOT),
        };
        let mut i = 0usize;
        while i < stat_last {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(AH_ESLOT),
            };
            if !s.ok {
                return Err(AH_ESTATE);
            }
            if s.code != 0 {
                return Err(AH_ECODE);
            }
            let want_value = match VALS.get(i) {
                Some(v) => *v,
                None => return Err(AH_ESLOT),
            };
            if s.value != want_value {
                return Err(AH_EVALUE);
            }
            let want_op = match self.stat_op_want(i) {
                Some(v) => v,
                None => return Err(AH_ESLOT),
            };
            if s.op != want_op {
                return Err(AH_ECODE);
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < REJECT {
            let cell = match STAT_CELLS.checked_add(1).and_then(|x| x.checked_add(j)) {
                Some(x) => x,
                None => return Err(AH_ESLOT),
            };
            let s = match self.sample(cell) {
                Some(x) => x,
                None => return Err(AH_ESLOT),
            };
            if s.ok {
                return Err(AH_ESTATE);
            }
            if s.value != 0 {
                return Err(AH_EVALUE);
            }
            let want_code = match REJECT_CODES.get(j) {
                Some(c) => *c,
                None => return Err(AH_ESLOT),
            };
            if s.code != want_code {
                return Err(AH_ECODE);
            }
            let want_op = match DEFECT_OP_BASE.checked_add(j as u8) {
                Some(x) => x,
                None => return Err(AH_ESLOT),
            };
            if s.op != want_op {
                return Err(AH_ECODE);
            }
            let mut k = j + 1usize;
            while k < REJECT {
                let other = match REJECT_CODES.get(k) {
                    Some(c) => *c,
                    None => return Err(AH_ESLOT),
                };
                if other == want_code {
                    return Err(AH_ECODE);
                }
                k += 1;
            }
            j += 1;
        }
        Ok(())
    }

    /// 第一处不合规的格子与原因（排障用，不改判定）。
    pub fn first_mismatch(&self) -> Option<(usize, &'static str)> {
        if self.len != SAMPLES {
            return Some((self.len, "len"));
        }
        let mut i = 0usize;
        while i < SAMPLES {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Some((i, "hole")),
            };
            if i < STAT_CELLS + 1 {
                let want_value = match VALS.get(i) {
                    Some(v) => *v,
                    None => return Some((i, "slot")),
                };
                if !s.ok {
                    return Some((i, "ok-flag"));
                }
                if s.code != 0 {
                    return Some((i, "stat-code"));
                }
                if s.value != want_value {
                    return Some((i, "stat-value"));
                }
            } else {
                let j = i - (STAT_CELLS + 1);
                let want_code = match REJECT_CODES.get(j) {
                    Some(c) => *c,
                    None => return Some((i, "slot")),
                };
                if s.ok {
                    return Some((i, "ok-flag"));
                }
                if s.value != 0 {
                    return Some((i, "defect-value"));
                }
                if s.code != want_code {
                    return Some((i, "defect-code"));
                }
            }
            i += 1;
        }
        None
    }

    /// 折叠全账得指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.len as u64);
        h = fold_word(h, self.rejects as u64);
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.sample(i) {
                h = fold_word(h, s.seal());
            }
            i += 1;
        }
        h
    }

    /// 冻结：先复核再折叠，复核不过一律拒（红账不许盖戳）。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(AH_ESTATE);
        }
        match self.audit() {
            Ok(()) => {}
            Err(code) => return Err(code),
        }
        let h = self.seal();
        if h == 0 {
            return Err(AH_ESEAL);
        }
        self.root = h;
        self.frozen = true;
        Ok(h)
    }

    /// 重放：以同一批格子重建账本，比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(AH_ESTATE);
        }
        let mut fresh = Ledger::new();
        let mut i = 0usize;
        while i < self.len {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(AH_ESEAL),
            };
            match fresh.register(s) {
                Ok(()) => {}
                Err(_) => return Err(AH_ESEAL),
            }
            i += 1;
        }
        let h = fresh.seal();
        if h != self.root {
            return Err(AH_ESEAL);
        }
        Ok(h)
    }

    /// 改动一格（仅未冻结时允许），用于注入红。
    pub fn poke(&mut self, i: usize, value: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(AH_ESTATE);
        }
        match self.slots.get_mut(i) {
            Some(cell) => {
                match *cell {
                    Some(ref mut s) => {
                        s.value = value;
                    }
                    None => return Err(AH_ESLOT),
                }
                Ok(())
            }
            None => Err(AH_ESLOT),
        }
    }

    /// 呈现全账。
    pub fn render(&self) -> String {
        let mut s = String::from("ledger len=");
        s.push_str(dec_text(self.len as u32).as_str());
        s.push_str(" ok=");
        s.push_str(dec_text(self.ok_count() as u32).as_str());
        s.push_str(" err=");
        s.push_str(dec_text(self.err_count() as u32).as_str());
        s.push_str(" rejects=");
        s.push_str(dec_text(self.rejects as u32).as_str());
        s.push_str(" frozen=");
        s.push_str(bool_text(self.frozen));
        s
    }
}

/// 统计表：七个统计量 + 红档率。
///
/// 前七格是"这道闸今天扫过多少格"的真实计数，第八格是红档率标定值 929
/// （14 格里 13 格判对时的千分率）——它必须是一个**能复算出来的数**，不是
/// 随手写的常数：`red_rate_calibrated()` 每次都用 13/14 现算一遍来比。
pub const VALS: [u32; ACCEPT] = [
    STAGE_COUNT,
    APPLET_JUDGED as u32,
    USAGE_CELLS as u32,
    CORPUS as u32,
    ACCEPT as u32,
    REJECT as u32,
    RATE_FULL,
    RED_RATE,
];

/// 十四格标准台账：七统计 + 一红档 + 六缺陷码。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut i = 0usize;
    while i < STAT_CELLS {
        let op = match STAT_OPS.get(i) {
            Some(x) => *x,
            None => return l,
        };
        let value = match VALS.get(i) {
            Some(x) => *x,
            None => return l,
        };
        let s = Sample {
            slot: i as u8,
            op,
            ok: true,
            code: 0,
            value,
        };
        match l.register(s) {
            Ok(()) => {}
            Err(_) => return l,
        }
        i += 1;
    }
    let red = Sample {
        slot: STAT_CELLS as u8,
        op: OPID_STAT_RED,
        ok: true,
        code: 0,
        value: RED_RATE,
    };
    match l.register(red) {
        Ok(()) => {}
        Err(_) => return l,
    }
    let mut j = 0usize;
    while j < REJECT {
        let code = match REJECT_CODES.get(j) {
            Some(x) => *x,
            None => return l,
        };
        let op = match DEFECT_OP_BASE.checked_add(j as u8) {
            Some(x) => x,
            None => return l,
        };
        let slot = (STAT_CELLS + 1 + j) as u8;
        let s = Sample {
            slot,
            op,
            ok: false,
            code,
            value: 0,
        };
        match l.register(s) {
            Ok(()) => {}
            Err(_) => return l,
        }
        j += 1;
    }
    l
}

/// 注入一维错的台账（证明复核真的在逐格看，而不是只看格数）。
pub fn build_ledger_broken(which: u8) -> Ledger {
    let mut l = build_ledger();
    let (cell, value) = match which {
        0 => match (STAT_CELLS, VALS.get(STAT_CELLS)) {
            (c, Some(v)) => (c, v + 1),
            _ => return l,
        },
        1 => match (0usize, VALS.get(0)) {
            (c, Some(v)) => (c, v + 7),
            _ => return l,
        },
        2 => match STAT_CELLS.checked_add(1) {
            Some(c) => (c, 1u32),
            None => return l,
        },
        3 => match STAT_CELLS.checked_add(3) {
            Some(c) => (c, 9u32),
            None => return l,
        },
        _ => return l,
    };
    match l.poke(cell, value) {
        Ok(()) => {}
        Err(_) => {}
    }
    l
}

// ---------------------------------------------------------------------------
// 十三条判据谓词（全部私有，只在 run_unx11668_checks 里汇总）
// ---------------------------------------------------------------------------

/// 01 名称与段基址：常量层自洽，任何一条规模关系破了都算红。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d1f0 {
        return false;
    }
    if ANCHOR_F11667_SEG.checked_add(ANCHOR_F11667_GAP) != Some(SEG_BASE) {
        return false;
    }
    if ANCHOR_F11669_SEG != SEG_BASE + ANCHOR_F11669_GAP {
        return false;
    }
    if CORPUS != SAMPLES || CORPUS != ACCEPT + REJECT {
        return false;
    }
    if TIER_COUNT != Tier::all().len() {
        return false;
    }
    if STAGE_COUNT != STAGE_LAST_RANK || Stage::all().len() as u32 != STAGE_COUNT {
        return false;
    }
    true
}

/// 02 十三码互异：全部落在段内、两两不同、六个缺陷码与判序一一对应。
fn codes_ok() -> bool {
    if ALL_CODES.len() != 13 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_CODES.len() {
        let c = match ALL_CODES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if c <= SEG_BASE || c > SEG_BASE + 0x0d {
            return false;
        }
        let mut j = i + 1usize;
        while j < ALL_CODES.len() {
            if let Some(d) = ALL_CODES.get(j) {
                if *d == c {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let faults = REJECT_FAULTS;
    let mut k = 0usize;
    while k < REJECT {
        let f = match faults.get(k) {
            Some(x) => *x,
            None => return false,
        };
        let want = match REJECT_CODES.get(k) {
            Some(x) => *x,
            None => return false,
        };
        if f.code() != want {
            return false;
        }
        if f.order() != k {
            return false;
        }
        k += 1;
    }
    AH_ESLOT != AH_ESEAL && AH_EBODY != AH_ECHAN
}

/// 03 usage 表十格齐：五 applet × 两线，首行正文都非空，尾注位与线一致。
fn table_ok() -> bool {
    if USAGE_TABLE.len() != USAGE_CELLS {
        return false;
    }
    if USAGE_CELLS != APPLET_JUDGED * 2 {
        return false;
    }
    let mut i = 0usize;
    while i < APPLET_JUDGED {
        let a = match Applet::at(i) {
            Some(x) => x,
            None => return false,
        };
        let mut l = 0usize;
        while l < 2 {
            let line = if l == 0 {
                UsageLine::Gnu
            } else {
                UsageLine::Busybox
            };
            let cell = match usage_cell(a, line) {
                Some(c) => c,
                None => return false,
            };
            if cell.first.is_empty() || cell.body.is_empty() {
                return false;
            }
            if !starts_with(cell.first, USAGE_PREFIX) {
                return false;
            }
            if cell.first.as_bytes().len() as u32 > FIRST_BYTE_CAP {
                return false;
            }
            if cell.trailer != line.has_trailer() {
                return false;
            }
            if cell.applet != a || cell.line != line {
                return false;
            }
            l += 1;
        }
        i += 1;
    }
    usage_cell(Applet::Unknown, UsageLine::Gnu).is_none()
}

/// 前缀比（逐字节，空 needle 不算命中）。
fn starts_with(hay: &str, needle: &str) -> bool {
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

/// 子串存在（逐字节，空 needle 立即为真）。
fn contains(hay: &str, needle: &str) -> bool {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        return true;
    }
    if n.len() > h.len() {
        return false;
    }
    let mut i = 0usize;
    while i + n.len() <= h.len() {
        let mut j = 0usize;
        let mut hit = true;
        while j < n.len() {
            let (Some(a), Some(b)) = (h.get(i + j), n.get(j)) else {
                hit = false;
                break;
            };
            if a != b {
                hit = false;
                break;
            }
            j += 1;
        }
        if hit {
            return true;
        }
        i += 1;
    }
    false
}

/// 04 首行形态分家：coreutils 用 `[OPTION]...` 占位，busybox 铺满短项位图。
fn firstline_ok() -> bool {
    if usage_first(Applet::Ls, UsageLine::Gnu) != "Usage: ls [OPTION]... [FILE]..." {
        return false;
    }
    if usage_first(Applet::Wc, UsageLine::Busybox) != "Usage: wc [-clmwL] [FILE]..." {
        return false;
    }
    let mut i = 0usize;
    while i < APPLET_JUDGED {
        let a = match Applet::at(i) {
            Some(x) => x,
            None => return false,
        };
        let gnu = usage_first(a, UsageLine::Gnu);
        let bb = usage_first(a, UsageLine::Busybox);
        if !contains(gnu, GNU_PLACEHOLDER) {
            return false;
        }
        if contains(bb, GNU_PLACEHOLDER) {
            return false;
        }
        // `ls` / `wc` 的 busybox 首行把短项位图整段铺开，`[OPTIONS]...` 这种
        // 八股占位在它们的首行里不存在；`cp` / `head` / `tail` 用 `[OPTIONS]`。
        if matches!(a, Applet::Ls | Applet::Wc) && !contains(bb, "[-") {
            return false;
        }
        if matches!(a, Applet::Cp | Applet::Head | Applet::Tail) && !contains(bb, "OPTIONS") {
            return false;
        }
        if gnu == bb {
            return false;
        }
        i += 1;
    }
    true
}

/// 05 帮助走 stdout：两族长篇都定型在 stdout，且行数大于一。
fn channel_ok() -> bool {
    let mut i = 0usize;
    while i < APPLET_JUDGED {
        let a = match Applet::at(i) {
            Some(x) => x,
            None => return false,
        };
        let s = Shape::blank().with_applet(a);
        let o = define(&s);
        if !o.ok() || o.code != 0 {
            return false;
        }
        if o.value < 2 {
            return false;
        }
        if !matches!(o.note, Note::HelpVerbatim) {
            return false;
        }
        if s.channel != Channel::Stdout {
            return false;
        }
        i += 1;
    }
    // 通道翻到 stderr：必须判死且不给正文成功档。
    let mut bad = Shape::blank();
    bad.channel = Channel::Stderr;
    let ob = define(&bad);
    if ob.ok() || ob.consistent {
        return false;
    }
    if !matches!(ob.note, Note::HelpOnStderr) {
        return false;
    }
    ob.stage == Stage::Channel
}

/// 06 码档对齐：帮助成功 0、busybox 不支持 `--usage` 1、未名 127。
fn code_ok() -> bool {
    let ok = define(&Shape::blank());
    if ok.code != 0 || !ok.ok() {
        return false;
    }
    let mut bb = Shape::blank().with_line(UsageLine::Busybox);
    bb.entry = Entry::MultiCall;
    bb.banner_seen = true;
    bb.trailer_seen = false;
    if define(&bb).code != 0 {
        return false;
    }
    let unsupported = Shape::blank()
        .with_line(UsageLine::Busybox)
        .with_form(HelpForm::LongUsage);
    let ou = define(&unsupported);
    if ou.code != 1 || ou.ok() {
        return false;
    }
    let unknown = Shape::blank().with_applet(Applet::Unknown);
    let on = define(&unknown);
    if on.code != 127 || on.ok() {
        return false;
    }
    if !contains(msg_not_our_applet(Applet::Unknown).as_str(), "applet not found") {
        return false;
    }
    on.stage == Stage::Dispatch && on.default
}

/// 07 形式辨析：`--help` 长篇、`--usage` 单行、`-h` 不是帮助。
fn form_ok() -> bool {
    let long = define(&Shape::blank());
    let short = define(&Shape::blank().with_form(HelpForm::LongUsage));
    let h = define(&Shape::blank().with_form(HelpForm::ShortH));
    if long.value <= 1 || short.value != 1 {
        return false;
    }
    if !matches!(h.note, Note::ShortHNotHelp) || !h.ok() || h.value != 0 {
        return false;
    }
    if h.stage != Stage::Form || h.default {
        return false;
    }
    // `-h` 在两族都是 applet 自己的开关：换线不改变"无正文"。
    let mut hb = Shape::blank().with_line(UsageLine::Busybox);
    hb.form = HelpForm::ShortH;
    hb.trailer_seen = false;
    let hbo = define(&hb);
    if hbo.value != 0 || hbo.default {
        return false;
    }
    // `--usage` 的行数恒为 1，不论是哪个 applet。
    let mut i = 0usize;
    while i < APPLET_JUDGED {
        let a = match Applet::at(i) {
            Some(x) => x,
            None => return false,
        };
        let mut s = Shape::blank().with_applet(a);
        s.form = HelpForm::LongUsage;
        s.trailer_seen = false;
        let o = define(&s);
        if o.value != 1 || !matches!(o.note, Note::UsageOneLine) {
            return false;
        }
        i += 1;
    }
    HelpForm::all().len() == FORM_COUNT
}

/// 08 语料十四槽互校：现场 ←→ 申报 ←→ 期望表三方一致，且死格只坏一维。
fn corpus_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return false,
        };
        let a = c.actual();
        let want_stage = match WANT_STAGE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want_code = match WANT_CODE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want_value = match WANT_VALUE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want_note = match WANT_NOTE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want_default = match WANT_DEFAULT.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want_success = match WANT_SUCCESS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if a.stage != want_stage
            || a.code as u32 != want_code
            || a.value != want_value
            || a.note != want_note
            || a.default != want_default
            || a.ok() != want_success
        {
            return false;
        }
        if a.value > BODY_LINE_CAP {
            return false;
        }
        // 死格：申报只许在它自己那一维上偏离现场。
        if i >= ACCEPT {
            let ndx = match WANT_NDX.get(i) {
                Some(x) => *x,
                None => return false,
            };
            let k = &c.claim;
            let mut offs = 0usize;
            if a.ok() != k.success() {
                offs += 1;
            }
            if a.stage != k.stage {
                offs += 1;
            }
            if a.code != k.code {
                offs += 1;
            }
            if a.value != k.value {
                offs += 1;
            }
            if a.note != k.note {
                offs += 1;
            }
            if a.default != k.default {
                offs += 1;
            }
            if offs != 1 {
                return false;
            }
            if fault_ndx(c.fault()) != ndx {
                return false;
            }
        }
        i += 1;
    }
    // 标签必须挂本卡号且互不相同。
    let mut t = 0usize;
    while t < CORPUS {
        let a = match WANT_TAGS.get(t) {
            Some(x) => *x,
            None => return false,
        };
        if !starts_with(a, "f11668-") {
            return false;
        }
        let mut u = t + 1usize;
        while u < CORPUS {
            if let Some(b) = WANT_TAGS.get(u) {
                if *b == a {
                    return false;
                }
            }
            u += 1;
        }
        t += 1;
    }
    WANT_STAGE.len() == CORPUS
        && WANT_CODE.len() == CORPUS
        && WANT_VALUE.len() == CORPUS
        && WANT_NOTE.len() == CORPUS
        && WANT_DEFAULT.len() == CORPUS
        && WANT_SUCCESS.len() == CORPUS
        && WANT_NDX.len() == CORPUS
}

/// 09 判序六维互斥：Estate 先于 Stage，Stage 先于 Code，逐级都不许串。
fn judge_ok() -> bool {
    let mut good = Case::blank();
    good.shape = Shape::blank();
    good.claim = Claim {
        success: true,
        stage: Stage::Exit,
        code: 0,
        value: define(&Shape::blank()).value,
        note: Note::HelpVerbatim,
        default: false,
    };
    if judge(&good) != Fault::Ok {
        return false;
    }
    // 同时坏 Estate 与 Stage：必须报 Estate。
    let mut both = good;
    both.claim.success = false;
    both.claim.stage = Stage::Form;
    if judge(&both) != Fault::Estate {
        return false;
    }
    // 同时坏 Stage 与 Code：必须报 Stage。
    let mut sc = good;
    sc.claim.stage = Stage::Body;
    sc.claim.code = 1;
    if judge(&sc) != Fault::Stage {
        return false;
    }
    // 同时坏 Code 与 Value：必须报 Code。
    let mut cv = good;
    cv.claim.code = 2;
    cv.claim.value = 0;
    if judge(&cv) != Fault::Code {
        return false;
    }
    // 同时坏 Value 与 Note：必须报 Value。
    let mut vn = good;
    vn.claim.value = 0;
    vn.claim.note = Note::HelpBbBare;
    if judge(&vn) != Fault::Value {
        return false;
    }
    // 同时坏 Note 与 Default：必须报 Note。
    let mut nd = good;
    nd.claim.note = Note::HelpBbBanner;
    nd.claim.default = true;
    if judge(&nd) != Fault::Note {
        return false;
    }
    // 只剩兜底坏：报 Default。
    let mut dd = good;
    dd.claim.default = true;
    if judge(&dd) != Fault::Default {
        return false;
    }
    // 判序表本身：六维两两互异且位次固定。
    let mut k = 0usize;
    while k < REJECT {
        let f = match REJECT_FAULTS.get(k) {
            Some(x) => *x,
            None => return false,
        };
        if f.order() != k {
            return false;
        }
        let mut j = k + 1usize;
        while j < REJECT {
            if let Some(g) = REJECT_FAULTS.get(j) {
                if g.code() == f.code() || *g == f {
                    return false;
                }
            }
            j += 1;
        }
        k += 1;
    }
    if ndx_fault(7).is_some() || ndx_fault(6) != Some(Fault::Ok) {
        return false;
    }
    fault_ndx(Fault::Estate) == 0 && fault_short(Fault::Default) == "EDEFAUL"
}

/// 10 四档矩阵覆盖：四档都有落点，行数齐、分母平，逐行正例数钉死。
fn matrix_ok() -> bool {
    let m = Matrix::build();
    if m.slots() != CORPUS * TIER_COUNT {
        return false;
    }
    if m.goods() != CORPUS {
        return false;
    }
    if !m.covered() {
        return false;
    }
    if m.red_total() + m.goods() != m.slots() {
        return false;
    }
    let verbatim = match m.row(Tier::Verbatim) {
        Some(x) => x,
        None => return false,
    };
    if verbatim.good != 5 || verbatim.total != CORPUS {
        return false;
    }
    let bannered = match m.row(Tier::Bannered) {
        Some(x) => x,
        None => return false,
    };
    if bannered.good != 2 || !bannered.tier.has_text() {
        return false;
    }
    let nobody = match m.row(Tier::NoBody) {
        Some(x) => x,
        None => return false,
    };
    if nobody.good != 6 || nobody.tier.has_text() {
        return false;
    }
    let refused = match m.row(Tier::Refused) {
        Some(x) => x,
        None => return false,
    };
    if refused.good != 1 || refused.tier.has_text() {
        return false;
    }
    if m.render().len() == 0 {
        return false;
    }
    // 归档函数本身：混认现场必须进拒收档，绝不许被当成正文交付。
    let mixed = match SHAPES.get(ACCEPT) {
        Some(x) => *x,
        None => return false,
    };
    if Tier::of(&define(&mixed)) != Tier::Refused {
        return false;
    }
    // 无正文现场必须进 NoBody，带 banner 的多调用现场必须进 Bannered。
    let no_body = match SHAPES.get(ACCEPT - 1) {
        Some(x) => *x,
        None => return false,
    };
    if Tier::of(&define(&no_body)) != Tier::NoBody {
        return false;
    }
    let bannered_shape = match SHAPES.get(ACCEPT - 4) {
        Some(x) => *x,
        None => return false,
    };
    if Tier::of(&define(&bannered_shape)) != Tier::Bannered {
        return false;
    }
    report_agrees_with_matrix()
}

/// 11 全绿渲染逐字：渲染文本一字不差，且空报告的形态绝不与全绿混淆。
fn render_ok() -> bool {
    let rep = settle(None);
    if !rep.green() {
        return false;
    }
    if rep.render().as_str() != RENDER_OK {
        return false;
    }
    let empty = empty_report();
    if empty.green() || empty.render().as_str() == RENDER_OK {
        return false;
    }
    if empty.rate() != 0 {
        return false;
    }
    let bad = match run_with(Some((CORPUS - 1, Fault::Value))) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if bad.green() || bad.rate() != RED_RATE {
        return false;
    }
    red_rate_calibrated()
}

/// 12 阶段闸门不倒退：正序通过、倒退判死、封账后再走判死。
fn gate_ok() -> bool {
    let h = match Gate::full_pass() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if h == 0 {
        return false;
    }
    let mut g = Gate::new();
    if g.close().is_ok() {
        return false;
    }
    let mut i = 0usize;
    let table = Stage::all();
    while i < table.len() {
        let s = match table.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if g.step(s).is_err() {
            return false;
        }
        i += 1;
    }
    if g.steps() as u32 != STAGE_COUNT {
        return false;
    }
    // 倒退：Exit → Form 必须给阶段码，且不改当前档。
    if g.step(Stage::Form) != Err(AH_ESTAGE) {
        return false;
    }
    if g.stage() != Some(Stage::Exit) {
        return false;
    }
    if g.refusals() != 1 {
        return false;
    }
    // 封账后再走、再封账：都是状态码。
    if g.close().is_err() {
        return false;
    }
    if g.step(Stage::Exit) != Err(AH_ESTATE) {
        return false;
    }
    if g.close() != Err(AH_ESTATE) {
        return false;
    }
    if !g.is_closed() || g.root() == 0 {
        return false;
    }
    // 空闸门封账：没有落过档，必须给阶段码。
    let mut e = Gate::new();
    if e.close() != Err(AH_ESTAGE) {
        return false;
    }
    let walked = match Gate::walk(&[Stage::Dispatch, Stage::Form]) {
        Ok(x) => x,
        Err(_) => return false,
    };
    walked.steps() == 2
}

/// 13 账本不平：格数、成败配比、逐格复核、冻结重放与槽位纪律。
fn ledger_ok() -> bool {
    let mut l = build_ledger();
    if l.len() != SAMPLES || l.is_empty() {
        return false;
    }
    if l.audit() != Ok(()) {
        return false;
    }
    if l.ok_count() != STAT_CELLS + 1 || l.err_count() != REJECT {
        return false;
    }
    if l.first_mismatch().is_some() {
        return false;
    }
    // 槽位纪律：跳号必须拒收且不入账。
    let mut skip = Ledger::new();
    let stray = Sample {
        slot: 3,
        op: OP_DISPATCH,
        ok: true,
        code: 0,
        value: 1,
    };
    if skip.register(stray) != Err(AH_ESLOT) {
        return false;
    }
    if skip.rejects() != 1 || skip.len() != 0 {
        return false;
    }
    // 冻结前不许重放；冻结后不许登记与改动。
    if l.replay().is_ok() {
        return false;
    }
    let h = match l.freeze() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if h == 0 || !l.is_frozen() || l.root() != h {
        return false;
    }
    if l.replay() != Ok(h) {
        return false;
    }
    if l.freeze() != Err(AH_ESTATE) {
        return false;
    }
    if l.register(stray) != Err(AH_ESTATE) {
        return false;
    }
    if l.poke(0, 1) != Err(AH_ESTATE) {
        return false;
    }
    // 注入一维错的账必须被复核逮住。
    let b0 = build_ledger_broken(0);
    if b0.audit() != Err(AH_EVALUE) {
        return false;
    }
    let b1 = build_ledger_broken(1);
    if b1.audit() != Err(AH_EVALUE) {
        return false;
    }
    let mut b2 = build_ledger_broken(2);
    if b2.audit() != Err(AH_EVALUE) {
        return false;
    }
    if b2.freeze().is_ok() {
        return false;
    }
    // 越界取值一律 `None`，绝不给默认格。
    if l.sample(SAMPLES).is_some() || l.sample(0).is_none() {
        return false;
    }
    let fresh = Ledger::new();
    fresh.replay().is_err() && fresh.is_empty()
}

/// 申报表第一格的标签：跨槽复用时的缺省 tag。
pub fn head_tag() -> &'static str {
    match WANT_TAGS.get(0) {
        Some(x) => *x,
        None => "f11668-ls-help-verbatim",
    }
}

/// 统计表第一格的值：台账注入用的缺省值。
pub fn head_val() -> u32 {
    match VALS.get(0) {
        Some(x) => *x,
        None => 0,
    }
}

/// 汇总：十三条判据，一条不多一条不少。
pub fn run_unx11668_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11668_applet_help_usage");
    s.add("AH01-名称与段基址", name_ok(), "static");
    s.add("AH02-十三码互异", codes_ok(), "static");
    s.add("AH03-usage表十格齐", table_ok(), "static");
    s.add("AH04-首行形态分家", firstline_ok(), "static");
    s.add("AH05-帮助走stdout", channel_ok(), "static");
    s.add("AH06-码档0与127对齐", code_ok(), "static");
    s.add("AH07-形式辨析三档", form_ok(), "static");
    s.add("AH08-语料十四槽互校", corpus_ok(), "static");
    s.add("AH09-判序六维互斥", judge_ok(), "static");
    s.add("AH10-四档矩阵覆盖", matrix_ok(), "static");
    s.add("AH11-全绿渲染逐字", render_ok(), "static");
    s.add("AH12-阶段闸门不倒退", gate_ok(), "static");
    s.add(
        "AH13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let audited = l.audit() == Ok(());
            let frozen = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            let four = inject_four();
            let mut all_four = true;
            let mut i = 0usize;
            while i < four.len() {
                if let Some(x) = four.get(i) {
                    if !*x {
                        all_four = false;
                    }
                }
                i += 1;
            }
            audited && frozen && ledger_ok() && all_four
        },
        "static",
    );
    s
}

// ---------------------------------------------------------------------------
// 单元测试：每条只钉一个可复算的事实
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 取一槽的实际结论；缺槽直接失败，绝不拿默认值冒充现场。
    fn out(i: usize) -> Outcome {
        case_at(i).expect("slot must exist").actual()
    }

    /// 多调用入口的 busybox 形状（banner 在前，无尾注）。
    fn bb_multicall() -> Shape {
        Shape {
            line: UsageLine::Busybox,
            entry: Entry::MultiCall,
            banner_seen: true,
            trailer_seen: false,
            ..Shape::blank()
        }
    }

    /// 符号链接入口的 busybox 形状（同正文，无 banner）。
    fn bb_symlink() -> Shape {
        Shape {
            line: UsageLine::Busybox,
            entry: Entry::Symlink,
            banner_seen: false,
            trailer_seen: false,
            ..Shape::blank()
        }
    }

    #[test]
    fn t01_seg_base_is_pinned() {
        assert_eq!(SEG_BASE, 0x6d1f0);
        assert_eq!(ANCHOR_F11667_SEG + ANCHOR_F11667_GAP, SEG_BASE);
        assert_eq!(ANCHOR_F11669_SEG, SEG_BASE + ANCHOR_F11669_GAP);
        assert_eq!(ANCHOR_F11674_SEG, 0x6d270);
        assert_eq!(ANCHOR_F11678_SEG, 0x6d2d0);
        assert!(ANCHOR_F11669_SEG > SEG_BASE);
        assert!(ANCHOR_F11674_SEG > ANCHOR_F11669_SEG);
    }

    #[test]
    fn t02_scale_relations_hold() {
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, CORPUS);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(ACCEPT + REJECT, CORPUS);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(STAGE_COUNT, 7);
        assert_eq!(STAGE_LAST_RANK, STAGE_COUNT);
        assert_eq!(APPLET_COUNT, 6);
        assert_eq!(APPLET_JUDGED, 5);
        assert_eq!(USAGE_CELLS, APPLET_JUDGED * 2);
        assert_eq!(LINE_COUNT, 3);
        assert_eq!(FORM_COUNT, 4);
        assert_eq!(STAT_CELLS, 7);
        assert_eq!(VALS.len(), ACCEPT);
    }

    #[test]
    fn t03_code_offsets_are_exact() {
        assert_eq!(AH_ESTATE, SEG_BASE + 0x01);
        assert_eq!(AH_ECODE, SEG_BASE + 0x02);
        assert_eq!(AH_ESTAGE, SEG_BASE + 0x03);
        assert_eq!(AH_EVALUE, SEG_BASE + 0x04);
        assert_eq!(AH_ENOTE, SEG_BASE + 0x05);
        assert_eq!(AH_EDEFAULT, SEG_BASE + 0x06);
        assert_eq!(AH_EBODY, SEG_BASE + 0x07);
        assert_eq!(AH_ECHAN, SEG_BASE + 0x08);
        assert_eq!(AH_ESLOT, SEG_BASE + 0x09);
        assert_eq!(AH_ESEAL, SEG_BASE + 0x0a);
        assert_eq!(AH_ENOBS, SEG_BASE + 0x0b);
        assert_eq!(AH_EMATRIX, SEG_BASE + 0x0c);
        assert_eq!(AH_EBAL, SEG_BASE + 0x0d);
    }

    #[test]
    fn t04_thirteen_codes_are_distinct_in_segment() {
        assert_eq!(ALL_CODES.len(), 13);
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            let c = match ALL_CODES.get(i) {
                Some(x) => *x,
                None => break,
            };
            assert!(c > SEG_BASE);
            assert!(c < ANCHOR_F11669_SEG);
            let mut j = i + 1usize;
            while j < ALL_CODES.len() {
                if let Some(d) = ALL_CODES.get(j) {
                    assert_ne!(c, *d);
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn t05_usage_table_has_ten_cells() {
        assert_eq!(USAGE_TABLE.len(), USAGE_CELLS);
        let mut cells = 0usize;
        let mut i = 0usize;
        while i < APPLET_JUDGED {
            let a = Applet::at(i).expect("applet in range");
            assert!(a.judged());
            let g = usage_cell(a, UsageLine::Gnu).expect("gnu cell");
            let b = usage_cell(a, UsageLine::Busybox).expect("busybox cell");
            assert!(!g.first.is_empty());
            assert!(!b.first.is_empty());
            assert!(!g.body.is_empty());
            assert!(!b.body.is_empty());
            assert!(g.trailer);
            assert!(!b.trailer);
            cells += 2;
            i += 1;
        }
        assert_eq!(cells, USAGE_CELLS);
        assert!(usage_cell(Applet::Unknown, UsageLine::Gnu).is_none());
        assert!(usage_cell(Applet::Unknown, UsageLine::Busybox).is_none());
    }

    #[test]
    fn t06_first_lines_split_by_family() {
        let mut i = 0usize;
        while i < APPLET_JUDGED {
            let a = Applet::at(i).expect("applet in range");
            let g = usage_first(a, UsageLine::Gnu);
            let b = usage_first(a, UsageLine::Busybox);
            assert!(g.starts_with(USAGE_PREFIX));
            assert!(b.starts_with(USAGE_PREFIX));
            assert!(g.contains(GNU_PLACEHOLDER));
            assert!(!b.contains(GNU_PLACEHOLDER));
            assert_ne!(g, b);
            i += 1;
        }
        assert_eq!(usage_first(Applet::Unknown, UsageLine::Gnu), "");
        assert_eq!(usage_first(Applet::Ls, UsageLine::Mixed), "");
    }

    #[test]
    fn t07_bodies_are_tab_indented_on_busybox() {
        let mut i = 0usize;
        while i < APPLET_JUDGED {
            let a = Applet::at(i).expect("applet in range");
            assert!(usage_body(a, UsageLine::Gnu).len() > 40);
            assert!(usage_body(a, UsageLine::Busybox).len() > 40);
            assert!(usage_body(a, UsageLine::Busybox).contains('\t'));
            assert!(lines_of(usage_body(a, UsageLine::Gnu)) >= 6);
            i += 1;
        }
        assert_eq!(usage_body(Applet::Unknown, UsageLine::Gnu), "");
    }

    #[test]
    fn t08_banner_text_is_verbatim() {
        assert!(BANNER_TEXT.starts_with(BB_VERSION));
        assert!(BANNER_TEXT.ends_with("\n\n"));
        assert_eq!(banner_text(), BANNER_TEXT);
        assert_eq!(lines_of(BANNER_TEXT), 2);
        assert!(BB_VERSION.contains("multi-call binary."));
    }

    #[test]
    fn t09_trailer_names_the_applet() {
        let mut i = 0usize;
        while i < APPLET_JUDGED {
            let a = Applet::at(i).expect("applet in range");
            let t = trailer_text(a);
            assert!(t.starts_with(TRAILER_HEAD));
            assert_eq!(lines_of(t.as_str()), 3);
            assert!(t.contains(a.name()));
            assert!(t.contains("invocation'"));
            i += 1;
        }
        assert_ne!(trailer_text(Applet::Ls), trailer_text(Applet::Cp));
    }

    #[test]
    fn t10_usage_oneline_is_single_line() {
        let mut i = 0usize;
        while i < APPLET_JUDGED {
            let a = Applet::at(i).expect("applet in range");
            let o = usage_oneline(a);
            assert!(o.starts_with(USAGE_PREFIX));
            assert!(!o.contains('\n'));
            assert_eq!(lines_of(o), 0);
            assert!(o.contains(a.name()));
            i += 1;
        }
        assert_eq!(usage_oneline(Applet::Unknown), "Usage: ");
    }

    #[test]
    fn t11_applet_table_is_closed() {
        assert_eq!(Applet::all().len(), APPLET_COUNT);
        assert_eq!(Applet::at(0), Some(Applet::Ls));
        assert_eq!(Applet::at(5), Some(Applet::Unknown));
        assert_eq!(Applet::at(6), None);
        let mut i = 0usize;
        while i < APPLET_JUDGED {
            let a = Applet::at(i).expect("applet in range");
            assert!(a.judged());
            assert_eq!(a.index(), i);
            i += 1;
        }
        assert!(!Applet::Unknown.judged());
        assert_eq!(Applet::Unknown.name(), "notmyapplet");
        assert_eq!(Applet::Ls.name(), "ls");
    }

    #[test]
    fn t12_usage_line_family_traits() {
        assert_eq!(UsageLine::all().len(), LINE_COUNT);
        assert!(UsageLine::Gnu.produces_text());
        assert!(UsageLine::Gnu.has_usage_flag());
        assert!(UsageLine::Gnu.has_trailer());
        assert!(UsageLine::Gnu.uses_placeholder());
        assert_eq!(UsageLine::Gnu.help_exit(), EXIT_OK);
        assert!(UsageLine::Busybox.produces_text());
        assert!(!UsageLine::Busybox.has_usage_flag());
        assert!(!UsageLine::Busybox.has_trailer());
        assert!(!UsageLine::Busybox.uses_placeholder());
        assert!(!UsageLine::Mixed.produces_text());
        assert_eq!(UsageLine::Mixed.help_exit(), EXIT_FAIL);
        assert_eq!(UsageLine::Busybox.index(), 1);
    }

    #[test]
    fn t13_entry_banner_rule() {
        assert_eq!(Entry::all().len(), 2);
        assert!(Entry::MultiCall.has_banner());
        assert!(!Entry::Symlink.has_banner());
        assert_eq!(Entry::MultiCall.name(), "multicall");
        assert_eq!(Entry::Symlink.index(), 1);
    }

    #[test]
    fn t14_form_traits_pin_short_h() {
        assert_eq!(HelpForm::all().len(), FORM_COUNT);
        assert!(HelpForm::LongHelp.wants_text());
        assert!(HelpForm::LongHelp.is_help());
        assert!(HelpForm::LongUsage.wants_text());
        assert!(!HelpForm::LongUsage.is_help());
        assert!(!HelpForm::ShortH.wants_text());
        assert!(!HelpForm::ShortH.is_help());
        assert!(!HelpForm::Operand.wants_text());
        assert!(!HelpForm::Operand.is_help());
        assert_eq!(HelpForm::ShortH.name(), "-h");
    }

    #[test]
    fn t15_channel_bytes_rule() {
        assert_eq!(Channel::all().len(), 3);
        assert!(Channel::Stdout.carries_bytes());
        assert!(Channel::Stderr.carries_bytes());
        assert!(!Channel::Nowhere.carries_bytes());
        assert_eq!(Channel::Stdout.name(), "stdout");
        assert_eq!(Channel::Nowhere.index(), 2);
    }

    #[test]
    fn t16_stage_ranks_are_monotone() {
        let table = Stage::all();
        assert_eq!(table.len() as u32, STAGE_COUNT);
        let mut i = 0usize;
        while i < table.len() {
            let s = match table.get(i) {
                Some(x) => *x,
                None => break,
            };
            assert_eq!(s.rank(), i as u32 + 1);
            assert_eq!(Stage::from_rank(s.rank()), Some(s));
            if i < 3 {
                assert!(s.before_text());
            } else {
                assert!(!s.before_text());
            }
            assert_eq!(s.settles_code(), s == Stage::Exit);
            i += 1;
        }
        assert_eq!(Stage::from_rank(0), None);
        assert_eq!(Stage::from_rank(8), None);
        assert_eq!(Stage::Exit.rank(), STAGE_LAST_RANK);
    }

    #[test]
    fn t17_note_classes_are_disjoint() {
        assert_eq!(Note::all().len(), Note::COUNT);
        assert_eq!(Note::COUNT, 12);
        let mut defaulted = 0usize;
        let mut bodies = 0usize;
        let mut runs = 0usize;
        let mut errs = 0usize;
        let mut gnu = 0usize;
        let mut bb = 0usize;
        let mut i = 0usize;
        while i < Note::COUNT {
            let n = match Note::all().get(i) {
                Some(x) => *x,
                None => break,
            };
            if n.defaulted() {
                defaulted += 1;
            }
            if n.has_body() {
                bodies += 1;
            }
            if n.runs_applet() {
                runs += 1;
            }
            if n.speaks_error() {
                errs += 1;
            }
            if n.is_gnu() {
                gnu += 1;
            }
            if n.is_busybox() {
                bb += 1;
            }
            assert_eq!(n.index(), i);
            i += 1;
        }
        assert_eq!(defaulted, 2);
        assert_eq!(bodies, 4);
        assert_eq!(runs, 2);
        assert_eq!(errs, 5);
        assert_eq!(gnu, 2);
        assert_eq!(bb, 3);
        assert!(Note::NotOurApplet.defaulted());
        assert!(Note::SilentHelp.defaulted());
        assert!(!Note::ShortHNotHelp.defaulted());
        assert!(!Note::UsageUnsupported.defaulted());
        assert!(Note::HelpVerbatim.needs_trailer());
        assert!(!Note::HelpBbBanner.needs_trailer());
    }

    #[test]
    fn t18_note_names_are_unique() {
        let mut i = 0usize;
        while i < Note::COUNT {
            let a = match Note::all().get(i) {
                Some(x) => *x,
                None => break,
            };
            assert!(!a.name().is_empty());
            let mut j = i + 1usize;
            while j < Note::COUNT {
                if let Some(b) = Note::all().get(j) {
                    assert_ne!(a.name(), b.name());
                }
                j += 1;
            }
            i += 1;
        }
        assert_eq!(Note::HelpBbBare.name(), "help-bb-bare");
    }

    #[test]
    fn t19_tier_classification_is_total() {
        let verbatim = Outcome::make(Stage::Exit, 0, 13, Note::HelpVerbatim, false, true);
        let bannered = Outcome::make(Stage::Exit, 0, 10, Note::HelpBbBanner, false, true);
        let nobody = Outcome::make(Stage::Form, 0, 0, Note::ShortHNotHelp, false, true);
        let refused = Outcome::make(Stage::Line, 1, 0, Note::MixedNoText, false, false);
        assert_eq!(Tier::of(&verbatim), Tier::Verbatim);
        assert_eq!(Tier::of(&bannered), Tier::Bannered);
        assert_eq!(Tier::of(&nobody), Tier::NoBody);
        assert_eq!(Tier::of(&refused), Tier::Refused);
        assert_eq!(Tier::all().len(), TIER_COUNT);
        assert!(Tier::Verbatim.has_text());
        assert!(Tier::Bannered.has_text());
        assert!(!Tier::NoBody.has_text());
        assert!(!Tier::Refused.has_text());
        assert_eq!(Tier::all().get(0).map(|t| t.name()), Some("verbatim"));
    }

    #[test]
    fn t20_fault_codes_and_order() {
        assert_eq!(Fault::Ok.code(), 0);
        assert_eq!(Fault::Estate.code(), AH_ESTATE);
        assert_eq!(Fault::Stage.code(), AH_ESTAGE);
        assert_eq!(Fault::Code.code(), AH_ECODE);
        assert_eq!(Fault::Value.code(), AH_EVALUE);
        assert_eq!(Fault::Note.code(), AH_ENOTE);
        assert_eq!(Fault::Default.code(), AH_EDEFAULT);
        assert_eq!(Fault::Ok.order(), 6);
        assert_eq!(Fault::Estate.order(), 0);
        assert_eq!(Fault::Default.order(), 5);
        assert_eq!(fault_short(Fault::Default), "EDEFAUL");
        assert_eq!(Fault::Note.name(), "note");
    }

    #[test]
    fn t21_reject_tables_align() {
        assert_eq!(REJECT_FAULTS.len(), REJECT);
        assert_eq!(REJECT_CODES.len(), REJECT);
        let mut i = 0usize;
        while i < REJECT {
            let f = match REJECT_FAULTS.get(i) {
                Some(x) => *x,
                None => break,
            };
            let c = match REJECT_CODES.get(i) {
                Some(x) => *x,
                None => break,
            };
            assert_eq!(f.code(), c);
            assert_eq!(f.order(), i);
            assert_eq!(ndx_fault(i), Some(f));
            i += 1;
        }
        assert_eq!(ndx_fault(6), Some(Fault::Ok));
        assert_eq!(ndx_fault(7), None);
    }

    #[test]
    fn t22_define_blank_is_gnu_ls_help() {
        let o = define(&Shape::blank());
        assert_eq!(o.stage, Stage::Exit);
        assert_eq!(o.code, 0);
        assert_eq!(o.value, 13);
        assert_eq!(o.note, Note::HelpVerbatim);
        assert!(!o.default);
        assert!(o.consistent);
        assert!(o.ok());
        assert!(o.help_ok());
        assert_eq!(o.exit(), EXIT_OK);
        assert!(!o.anchored());
        assert!(!o.refused());
        assert_eq!(Tier::of(&o), Tier::Verbatim);
    }

    #[test]
    fn t23_define_busybox_multicall_counts_banner() {
        let o = define(&bb_multicall());
        assert_eq!(o.stage, Stage::Exit);
        assert_eq!(o.code, 0);
        assert_eq!(o.value, 10);
        assert_eq!(o.note, Note::HelpBbBanner);
        assert!(!o.default);
        assert!(o.ok());
        assert_eq!(Tier::of(&o), Tier::Bannered);
    }

    #[test]
    fn t24_define_short_h_is_not_help() {
        let o = define(&Shape::blank().with_form(HelpForm::ShortH));
        assert_eq!(o.stage, Stage::Form);
        assert_eq!(o.code, 0);
        assert_eq!(o.value, 0);
        assert_eq!(o.note, Note::ShortHNotHelp);
        assert!(!o.default);
        assert!(o.ok());
        assert!(!o.help_ok());
        assert_eq!(Tier::of(&o), Tier::NoBody);
        assert!(o.note.runs_applet());
    }

    #[test]
    fn t25_define_operand_runs_applet() {
        let o = define(&Shape::blank().with_form(HelpForm::Operand));
        assert_eq!(o.stage, Stage::Form);
        assert_eq!(o.code, 0);
        assert_eq!(o.value, 0);
        assert_eq!(o.note, Note::OperandRun);
        assert!(!o.default);
        assert_eq!(Tier::of(&o), Tier::NoBody);
    }

    #[test]
    fn t26_define_unknown_applet_goes_127() {
        let s = Shape::blank().with_applet(Applet::Unknown);
        let o = define(&s);
        assert_eq!(o.stage, Stage::Dispatch);
        assert_eq!(o.code, EXIT_NOTFOUND as i32);
        assert_eq!(o.value, 0);
        assert_eq!(o.note, Note::NotOurApplet);
        assert!(o.default);
        assert!(!o.ok());
        assert_eq!(Tier::of(&o), Tier::NoBody);
        assert_eq!(msg_not_our_applet(Applet::Unknown), "notmyapplet: applet not found");
    }

    #[test]
    fn t27_define_mixed_line_is_refused() {
        let s = Shape {
            line: UsageLine::Mixed,
            trailer_seen: false,
            ..Shape::blank()
        };
        let o = define(&s);
        assert_eq!(o.stage, Stage::Line);
        assert_eq!(o.code, EXIT_FAIL as i32);
        assert_eq!(o.value, 0);
        assert_eq!(o.note, Note::MixedNoText);
        assert!(!o.consistent);
        assert!(o.refused());
        assert_eq!(Tier::of(&o), Tier::Refused);
    }

    #[test]
    fn t28_define_busybox_usage_unsupported() {
        let s = Shape {
            form: HelpForm::LongUsage,
            ..bb_symlink()
        };
        let o = define(&s);
        assert_eq!(o.stage, Stage::Line);
        assert_eq!(o.code, EXIT_FAIL as i32);
        assert_eq!(o.value, 0);
        assert_eq!(o.note, Note::UsageUnsupported);
        assert!(!o.default);
        assert_eq!(Tier::of(&o), Tier::NoBody);
        assert!(msg_usage_unsupported(Applet::Ls).contains("unrecognized option: usage"));
    }

    #[test]
    fn t29_define_truncated_body_is_refused() {
        let s = Shape {
            truncated: true,
            ..Shape::blank()
        };
        let o = define(&s);
        assert_eq!(o.stage, Stage::Body);
        assert_eq!(o.value, 1);
        assert_eq!(o.note, Note::TruncatedBody);
        assert!(o.refused());
        assert!(!o.ok());
        assert_eq!(Tier::of(&o), Tier::Refused);
    }

    #[test]
    fn t30_define_help_on_stderr_is_refused() {
        let s = Shape {
            channel: Channel::Stderr,
            ..Shape::blank()
        };
        let o = define(&s);
        assert_eq!(o.stage, Stage::Channel);
        assert_eq!(o.code, EXIT_FAIL as i32);
        assert_eq!(o.value, 13);
        assert_eq!(o.note, Note::HelpOnStderr);
        assert!(o.refused());
        assert_eq!(Tier::of(&o), Tier::Refused);
    }

    #[test]
    fn t31_render_help_verbatim_carries_trailer() {
        let t = render_help(&Shape::blank());
        assert_eq!(lines_of(t.as_str()), 13);
        assert!(t.contains("Usage: ls [OPTION]... [FILE]..."));
        assert!(t.starts_with("Usage: ls "));
        assert!(t.contains(TRAILER_HEAD));
        assert!(t.contains("info '(coreutils) ls invocation'"));
        assert!(!t.contains(BB_VERSION));
    }

    #[test]
    fn t32_render_help_busybox_puts_banner_first() {
        let t = render_help(&bb_multicall());
        assert!(t.starts_with(BB_VERSION));
        assert!(t.contains("Usage: ls [-Z]"));
        assert!(!t.contains(TRAILER_HEAD));
        assert_eq!(lines_of(t.as_str()), 10);
    }

    #[test]
    fn t33_render_help_symlink_has_no_banner() {
        let s = bb_symlink();
        let t = render_help(&s);
        assert!(!t.starts_with(BB_VERSION));
        assert!(t.starts_with("Usage: ls [-Z]"));
        let o = define(&s);
        assert_eq!(o.note, Note::HelpBbBare);
        assert_eq!(o.value, 8);
        assert_eq!(Tier::of(&o), Tier::Verbatim);
    }

    #[test]
    fn t34_help_seals_differ_by_family() {
        let a = help_seal(Applet::Ls, UsageLine::Gnu, Entry::Symlink, HelpForm::LongHelp);
        let b = help_seal(Applet::Ls, UsageLine::Busybox, Entry::Symlink, HelpForm::LongHelp);
        let c = help_seal(Applet::Ls, UsageLine::Gnu, Entry::Symlink, HelpForm::LongHelp);
        assert_eq!(a, c);
        assert_ne!(a, b);
        assert_ne!(help_seal(Applet::Head, UsageLine::Gnu, Entry::Symlink, HelpForm::LongHelp), a);
        assert_ne!(help_seal(Applet::Ls, UsageLine::Gnu, Entry::Symlink, HelpForm::LongUsage), a);
    }

    #[test]
    fn t35_judge_reads_a_green_claim_green() {
        let c = case_at(0).expect("slot 0");
        assert_eq!(judge(&c), Fault::Ok);
        assert!(c.ok());
    }

    #[test]
    fn t36_judge_reports_estate_before_stage() {
        let mut c = case_at(0).expect("slot 0");
        c.claim.success = false;
        c.claim.stage = Stage::Form;
        assert_eq!(judge(&c), Fault::Estate);
    }

    #[test]
    fn t37_judge_reports_stage_before_code() {
        let mut c = case_at(0).expect("slot 0");
        c.claim.stage = Stage::Body;
        c.claim.code = 1;
        assert_eq!(judge(&c), Fault::Stage);
    }

    #[test]
    fn t38_judge_reports_code_before_value() {
        let mut c = case_at(0).expect("slot 0");
        c.claim.code = 2;
        c.claim.value = 3;
        assert_eq!(judge(&c), Fault::Code);
    }

    #[test]
    fn t39_judge_reports_value_before_note() {
        let mut c = case_at(0).expect("slot 0");
        c.claim.value = 3;
        c.claim.note = Note::OperandRun;
        assert_eq!(judge(&c), Fault::Value);
    }

    #[test]
    fn t40_judge_reports_note_before_default() {
        let mut c = case_at(0).expect("slot 0");
        c.claim.note = Note::OperandRun;
        c.claim.default = true;
        assert_eq!(judge(&c), Fault::Note);
    }

    #[test]
    fn t41_judge_reads_a_false_default_claim_red() {
        let mut c = case_at(0).expect("slot 0");
        c.claim.default = true;
        assert_eq!(judge(&c), Fault::Default);
        // 现场真落兜底时，怎么申报都不许当结论。
        let mut d = case_at(13).expect("slot 13");
        d.claim.default = true;
        assert_eq!(judge(&d), Fault::Default);
    }

    #[test]
    fn t42_corpus_is_eight_live_six_dead() {
        let all = cases();
        assert_eq!(all.len(), CORPUS);
        let mut i = 0usize;
        while i < CORPUS {
            let c = match all.get(i) {
                Some(x) => *x,
                None => break,
            };
            if i < ACCEPT {
                assert_eq!(c.fault(), Fault::Ok);
            } else {
                assert_ne!(c.fault(), Fault::Ok);
                assert_eq!(Some(c.fault()), expected(i));
            }
            i += 1;
        }
        assert!(case_at(CORPUS).is_none());
    }

    #[test]
    fn t43_tags_are_prefixed_and_unique() {
        assert_eq!(WANT_TAGS.len(), CORPUS);
        let mut i = 0usize;
        while i < CORPUS {
            let a = match WANT_TAGS.get(i) {
                Some(x) => *x,
                None => break,
            };
            assert!(a.starts_with("f11668-"));
            let mut j = i + 1usize;
            while j < CORPUS {
                if let Some(b) = WANT_TAGS.get(j) {
                    assert_ne!(a, *b);
                }
                j += 1;
            }
            i += 1;
        }
        assert_eq!(head_tag(), "f11668-ls-help-verbatim");
    }

    #[test]
    fn t44_expected_mapping_is_pinned() {
        assert_eq!(expected(0), Some(Fault::Ok));
        assert_eq!(expected(7), Some(Fault::Ok));
        assert_eq!(expected(8), Some(Fault::Estate));
        assert_eq!(expected(9), Some(Fault::Stage));
        assert_eq!(expected(10), Some(Fault::Code));
        assert_eq!(expected(11), Some(Fault::Value));
        assert_eq!(expected(12), Some(Fault::Note));
        assert_eq!(expected(13), Some(Fault::Default));
        assert_eq!(expected(14), None);
        assert_eq!(fault_ndx(Fault::Estate), 0);
        assert_eq!(fault_ndx(Fault::Ok), 6);
    }

    #[test]
    fn t45_cover_bounds_are_checked() {
        assert_eq!(cover(0), Ok(()));
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(CORPUS + 1), Err(AH_ESLOT));
        assert_eq!(cover(usize::MAX), Err(AH_ESLOT));
    }

    #[test]
    fn t46_matrix_covers_four_tiers() {
        let m = Matrix::build();
        assert_eq!(m.slots(), CORPUS * TIER_COUNT);
        assert_eq!(m.goods(), CORPUS);
        assert_eq!(m.red_total(), CORPUS * TIER_COUNT - CORPUS);
        assert_eq!(m.perfect_rows(), 0);
        assert!(m.covered());
        assert_eq!(m.row(Tier::Verbatim).map(|r| r.good), Some(5));
        assert_eq!(m.row(Tier::Bannered).map(|r| r.good), Some(2));
        assert_eq!(m.row(Tier::NoBody).map(|r| r.good), Some(6));
        assert_eq!(m.row(Tier::Refused).map(|r| r.good), Some(1));
        let r = m.row(Tier::Refused).expect("refused row");
        assert_eq!(r.total, CORPUS);
        assert_eq!(r.red(), CORPUS - 1);
        assert_eq!(r.render(), "refused=1/14");
        assert!(m.render().starts_with("matrix "));
        assert!(m.render().contains("verbatim=5/14"));
    }

    #[test]
    fn t47_report_render_is_byte_pinned() {
        let rep = run().expect("all green");
        assert_eq!(rep.render(), RENDER_OK);
        assert!(RENDER_OK.ends_with("rate=1000permille\n"));
        assert_eq!(rep.total, CORPUS);
        assert_eq!(rep.accept, ACCEPT);
        assert_eq!(rep.reject, REJECT);
        assert_eq!(rep.verbatim, 5);
        assert_eq!(rep.bannered, 2);
        assert_eq!(rep.nobody, 6);
        assert_eq!(rep.refused, 1);
        assert_eq!(rep.matched, CORPUS);
    }

    #[test]
    fn t48_report_rate_and_gate() {
        let rep = run().expect("all green");
        assert_eq!(rep.rate(), RATE_FULL);
        assert_eq!(rep.rate(), 1000);
        assert!(rep.green());
        assert_eq!(rep.reds(), 0);
        assert_eq!(rep.gate(), Ok(()));
        assert_eq!(rep.tier_cells(), CORPUS as u32);
        let empty = empty_report();
        assert_eq!(empty.rate(), 0);
        assert_eq!(empty.gate(), Err(AH_ENOBS));
        assert!(!empty.green());
    }

    #[test]
    fn t49_run_with_rejects_out_of_range_slot() {
        assert_eq!(run_with(Some((CORPUS, Fault::Value))), Err(AH_ESLOT));
        let rep = run_with(Some((0, Fault::Ok))).expect("no-op injection");
        assert_eq!(rep.matched, CORPUS);
        let dropped = run_with(Some((2, Fault::Value))).expect("injected report");
        assert_eq!(dropped.matched, CORPUS - 1);
        assert_eq!(dropped.rate(), RED_RATE);
        // 反例槽注入"它本来就该报的那一维"不算红。
        let keep = run_with(Some((8, Fault::Estate))).expect("same fault");
        assert_eq!(keep.matched, CORPUS);
    }

    #[test]
    fn t50_red_rate_is_calibrated() {
        assert_eq!(RED_RATE, 929);
        assert!(red_rate_calibrated());
        let bad = run_with(Some((11, Fault::Note))).expect("injected report");
        assert_eq!(bad.rate(), RED_RATE);
        assert_eq!((13u32 * RATE_FULL + (bad.total as u32) / 2) / (bad.total as u32), RED_RATE);
    }

    #[test]
    fn t51_four_routes_agree() {
        assert!(report_agrees_with_matrix());
        let four = inject_four();
        assert_eq!(four.len(), 4);
        let mut i = 0usize;
        while i < four.len() {
            assert_eq!(four.get(i), Some(&true));
            i += 1;
        }
    }

    #[test]
    fn t52_gate_walks_all_stages_in_order() {
        let root = Gate::full_pass().expect("monotone walk");
        assert_ne!(root, 0);
        let mut g = Gate::new();
        assert_eq!(g.close(), Err(AH_ESTAGE));
        assert_eq!(g.refusals(), 0);
        let table = Stage::all();
        let mut i = 0usize;
        while i < table.len() {
            let s = match table.get(i) {
                Some(x) => *x,
                None => break,
            };
            assert_eq!(g.step(s), Ok(()));
            i += 1;
        }
        assert_eq!(g.steps(), STAGE_COUNT);
        assert_eq!(g.stage(), Some(Stage::Exit));
        let h = g.close().expect("close after full walk");
        assert_ne!(h, 0);
        assert!(g.is_closed());
        assert!(g.render().contains("closed=true"));
    }

    #[test]
    fn t53_gate_refuses_regression() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Line), Ok(()));
        assert_eq!(g.step(Stage::Form), Err(AH_ESTAGE));
        assert_eq!(g.refusals(), 1);
        assert_eq!(g.steps(), 1);
        assert_eq!(g.stage(), Some(Stage::Line));
        // 同档重复走不算倒退（幂等前进）。
        assert_eq!(g.step(Stage::Line), Ok(()));
        assert_eq!(g.refusals(), 1);
    }

    #[test]
    fn t54_gate_refuses_after_close() {
        let mut g = Gate::walk(&Stage::all()).expect("walk");
        let h = g.close().expect("close");
        assert_ne!(h, 0);
        assert_eq!(g.step(Stage::Exit), Err(AH_ESTATE));
        assert_eq!(g.close(), Err(AH_ESTATE));
        assert_eq!(g.refusals(), 2);
        assert!(g.is_closed());
    }

    #[test]
    fn t55_gate_walk_reports_first_offence() {
        let g = Gate::walk(&[Stage::Dispatch, Stage::Exit]).expect("ascending");
        assert_eq!(g.steps(), 2);
        assert_eq!(g.stage(), Some(Stage::Exit));
        assert_eq!(Gate::walk(&[Stage::Exit, Stage::Dispatch]), Err(AH_ESTAGE));
        assert_eq!(Gate::walk(&[Stage::Form, Stage::Body, Stage::Form]), Err(AH_ESTAGE));
        assert!(Gate::new().render().starts_with("gate stage=none"));
        assert_eq!(Gate::new().root(), FNV_OFF);
    }

    #[test]
    fn t56_ledger_has_fourteen_cells() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(!l.is_empty());
        assert_eq!(l.ok_count(), STAT_CELLS + 1);
        assert_eq!(l.err_count(), REJECT);
        assert_eq!(l.rejects(), 0);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.first_mismatch(), None);
        assert_eq!(l.root(), 0);
        assert!(!l.is_frozen());
        assert!(l.sample(0).is_some());
        assert!(l.sample(SAMPLES).is_none());
        let last = l.sample(SAMPLES - 1).expect("last cell");
        assert_eq!(last.code, AH_EDEFAULT);
    }

    #[test]
    fn t57_ledger_refuses_misplaced_slots() {
        let mut l = Ledger::new();
        let stray = Sample {
            slot: 1,
            op: OP_FORM,
            ok: true,
            code: 0,
            value: 0,
        };
        assert_eq!(l.register(stray), Err(AH_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert!(l.is_empty());
        let first = Sample {
            slot: 0,
            op: OP_DISPATCH,
            ok: true,
            code: 0,
            value: 1,
        };
        assert_eq!(l.register(first), Ok(()));
        assert_eq!(l.register(first), Err(AH_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn t58_ledger_freezes_and_replays() {
        let mut l = build_ledger();
        let h = l.freeze().expect("freeze");
        assert_ne!(h, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), h);
        assert_eq!(l.replay(), Ok(h));
        assert_eq!(l.freeze(), Err(AH_ESTATE));
        let mut broken = build_ledger_broken(0);
        assert!(broken.freeze().is_err());
        assert!(!broken.is_frozen());
    }

    #[test]
    fn t59_ledger_poke_is_bounded() {
        let mut l = build_ledger();
        assert_eq!(l.poke(0, 1), Ok(()));
        assert_eq!(l.audit(), Err(AH_EVALUE));
        assert_eq!(l.first_mismatch(), Some((0, "stat-value")));
        assert_eq!(l.poke(SAMPLES, 1), Err(AH_ESLOT));
        let mut frozen = build_ledger();
        assert!(frozen.freeze().is_ok());
        assert_eq!(frozen.poke(0, 1), Err(AH_ESTATE));
    }

    #[test]
    fn t60_broken_ledgers_are_caught() {
        let mut w = 0u8;
        while w < 4u8 {
            let l = build_ledger_broken(w);
            assert_eq!(l.len(), SAMPLES);
            assert!(l.audit().is_err());
            assert!(l.first_mismatch().is_some());
            w += 1;
        }
        assert_eq!(build_ledger_broken(0).audit(), Err(AH_EVALUE));
        assert_eq!(build_ledger_broken(1).audit(), Err(AH_EVALUE));
        assert_eq!(build_ledger_broken(2).audit(), Err(AH_EVALUE));
        assert_eq!(build_ledger_broken(3).audit(), Err(AH_EVALUE));
    }

    #[test]
    fn t61_ledger_render_is_stable() {
        let l = build_ledger();
        assert_eq!(l.render(), "ledger len=14 ok=8 err=6 rejects=0 frozen=false");
        let mut f = l;
        assert!(f.freeze().is_ok());
        assert_eq!(f.render(), "ledger len=14 ok=8 err=6 rejects=0 frozen=true");
        let one = Sample {
            slot: 0,
            op: OP_DISPATCH,
            ok: true,
            code: 0,
            value: STAGE_COUNT,
        };
        assert!(one.render().starts_with("slot 0 op 01 true code 00000 value 7"));
    }

    #[test]
    fn t62_vals_pin_the_red_cell() {
        assert_eq!(VALS.get(0), Some(&STAGE_COUNT));
        assert_eq!(VALS.get(1), Some(&(APPLET_JUDGED as u32)));
        assert_eq!(VALS.get(2), Some(&(USAGE_CELLS as u32)));
        assert_eq!(VALS.get(3), Some(&(CORPUS as u32)));
        assert_eq!(VALS.get(4), Some(&(ACCEPT as u32)));
        assert_eq!(VALS.get(5), Some(&(REJECT as u32)));
        assert_eq!(VALS.get(6), Some(&RATE_FULL));
        assert_eq!(VALS.get(7), Some(&929));
        assert_eq!(head_val(), STAGE_COUNT);
        assert_eq!(STAT_OPS.len(), STAT_CELLS);
        assert_eq!(OPID_STAT_RED, 0x08);
        assert_eq!(DEFECT_OP_BASE, 0x21);
    }

    #[test]
    fn t63_checks_are_thirteen_and_green() {
        let s = run_unx11668_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
        assert_eq!(s.get(0).map(|c| c.name), Some("AH01-名称与段基址"));
        assert_eq!(s.get(12).map(|c| c.name), Some("AH13-冻结重放四注入"));
        assert_eq!(s.get(13), None);
        let mut i = 0usize;
        while i < s.len() {
            if let Some(c) = s.get(i) {
                assert!(c.passed);
                assert_eq!(c.detail, "static");
            }
            i += 1;
        }
    }

    #[test]
    fn t64_text_helpers_are_exact() {
        assert_eq!(seg_text(SEG_BASE), "0x06d1f0");
        assert_eq!(u32_text(0), "0");
        assert_eq!(u32_text(929), "929");
        assert_eq!(dec_text(4294967295), "4294967295");
        assert_eq!(hex_text(0x0d, 2), "0d");
        assert_eq!(hex_text(AH_EBAL - SEG_BASE, 2), "0d");
        assert_eq!(i32_text(-127), "-127");
        assert_eq!(i32_text(127), "127");
        assert_eq!(bool_text(true), "true");
        assert_eq!(bool_text(false), "false");
        assert_eq!(permille_text(929), "929permille");
        assert_eq!(lines_of("a\nb\n"), 2);
        assert_eq!(bytes_of("abc"), 3);
    }

    #[test]
    fn t65_shape_validity_rules() {
        assert!(Shape::blank().valid());
        assert!(bb_multicall().valid());
        assert!(bb_symlink().valid());
        assert!(!Shape::blank().with_entry(Entry::MultiCall).valid());
        assert!(!Shape {
            banner_seen: true,
            ..Shape::blank()
        }
        .valid());
        assert!(!Shape {
            line: UsageLine::Busybox,
            entry: Entry::MultiCall,
            banner_seen: true,
            trailer_seen: true,
            ..Shape::blank()
        }
        .valid());
        assert!(!Shape {
            form: HelpForm::LongUsage,
            operand_after: true,
            ..Shape::blank()
        }
        .valid());
        assert!(!Shape {
            form: HelpForm::ShortH,
            truncated: true,
            ..Shape::blank()
        }
        .valid());
        assert!(!Shape {
            line: UsageLine::Mixed,
            banner_seen: true,
            trailer_seen: false,
            ..Shape::blank()
        }
        .valid());
        assert!(!Shape {
            exit_declared: 200,
            ..Shape::blank()
        }
        .valid());
        assert!(!Shape {
            lang_c: false,
            ..Shape::blank()
        }
        .valid());
        assert!(Shape {
            form: HelpForm::LongUsage,
            ..Shape::blank()
        }
        .valid());
    }

    #[test]
    fn t66_outcome_helpers_follow_the_code() {
        let green = Outcome::make(Stage::Exit, 0, 13, Note::HelpVerbatim, false, true);
        assert_eq!(green.exit(), EXIT_OK);
        assert!(green.help_ok());
        let red = Outcome::make(Stage::Dispatch, EXIT_NOTFOUND, 0, Note::NotOurApplet, true, true);
        assert_eq!(red.exit(), EXIT_NOTFOUND);
        assert!(!red.ok());
        assert!(red.anchored());
        let negative = Outcome {
            stage: Stage::Exit,
            code: -1,
            value: 0,
            note: Note::SilentHelp,
            default: true,
            consistent: true,
        };
        assert_eq!(negative.exit(), EXIT_USAGE);
        assert!(!negative.help_ok());
        assert_eq!(Outcome::blank().stage, Stage::Exit);
        assert!(Outcome::blank().default);
    }

    #[test]
    fn t67_seal_is_sensitive_to_every_dimension() {
        let base = Shape::blank().seal();
        assert_eq!(base, Shape::blank().seal());
        assert_ne!(base, Shape::blank().with_applet(Applet::Cp).seal());
        assert_ne!(base, bb_symlink().seal());
        assert_ne!(base, Shape::blank().with_form(HelpForm::ShortH).seal());
        assert_ne!(
            base,
            Shape {
                channel: Channel::Stderr,
                ..Shape::blank()
            }
            .seal()
        );
        let o = define(&Shape::blank());
        assert_eq!(o.seal(), define(&Shape::blank()).seal());
        assert_ne!(o.seal(), define(&bb_multicall()).seal());
        let c = case_at(0).expect("slot 0");
        assert_eq!(c.seal(), case_at(0).expect("slot 0").seal());
        assert_ne!(c.seal(), case_at(1).expect("slot 1").seal());
        assert_eq!(c.tag(), "f11668-ls-help-verbatim");
    }

    #[test]
    fn t68_define_banner_mismatch_refuses() {
        // 声称看到 banner 但现场没有：形状本身不自相矛盾，是**现场对不上形状**，
        // 因此落拒收档靠的是 banner 复核，不是 `valid()`。
        let lie = Shape {
            banner_seen: true,
            ..bb_symlink()
        };
        assert!(lie.valid());
        let o = define(&lie);
        assert!(o.refused());
        assert_eq!(o.stage, Stage::Exit);
        assert_eq!(o.value, 8);
        assert!(!o.ok());
        assert_eq!(Tier::of(&o), Tier::Refused);
    }

    #[test]
    fn t69_corpus_slots_differ_in_one_dimension() {
        let mut i = ACCEPT;
        while i < CORPUS {
            let c = case_at(i).expect("reject slot");
            let a = c.actual();
            let k = &c.claim;
            let mut offs = 0usize;
            if a.ok() != k.success() {
                offs += 1;
            }
            if a.stage != k.stage {
                offs += 1;
            }
            if a.code != k.code {
                offs += 1;
            }
            if a.value != k.value {
                offs += 1;
            }
            if a.note != k.note {
                offs += 1;
            }
            if a.default != k.default {
                offs += 1;
            }
            assert_eq!(offs, 1);
            assert_eq!(WANT_NDX.get(i), Some(&c.fault().order()));
            i += 1;
        }
        let mut j = 0usize;
        while j < ACCEPT {
            let c = case_at(j).expect("live slot");
            assert!(c.ok());
            assert_eq!(c.fault(), Fault::Ok);
            j += 1;
        }
    }

    #[test]
    fn t70_want_tables_match_the_live_corpus() {
        let mut i = 0usize;
        while i < CORPUS {
            let a = out(i);
            assert_eq!(WANT_STAGE.get(i), Some(&a.stage));
            assert_eq!(WANT_CODE.get(i), Some(&(a.code as u32)));
            assert_eq!(WANT_VALUE.get(i), Some(&a.value));
            assert_eq!(WANT_NOTE.get(i), Some(&a.note));
            assert_eq!(WANT_DEFAULT.get(i), Some(&a.default));
            assert_eq!(WANT_SUCCESS.get(i), Some(&a.ok()));
            i += 1;
        }
        assert_eq!(WANT_STAGE.len(), CORPUS);
        assert_eq!(WANT_SUCCESS.len(), CORPUS);
    }
}

