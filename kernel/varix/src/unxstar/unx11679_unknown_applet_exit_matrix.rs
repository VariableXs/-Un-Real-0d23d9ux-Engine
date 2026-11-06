//! UNX-F11679 未知 applet 场景 × 退出码矩阵
//!
//! 这一卡的交付物就是一张**矩阵**：五行场景 × 五列退出码，二十五格，格格里
//! 都必须写成"落"还是"不落"。零悬空——每个格子要么是这条场景真会落到的码，
//! 要么是一条**明确判死**的矛盾。矩阵本身不是注释，它是判据的第二种写法：
//! 闸门（`define`）与矩阵（`ExitGrid`）必须对同一现场给出同一个结论，两处任
//! 何一处笔误都会在交叉校验里露出来。
//!
//! 五条场景不是"同一个错误的五种叫法"，它们是**五个不同的判决主体**：
//!
//! ```text
//! dispatch    busybox nosuch           分发层查表未命中        argv[1]      127
//! alias       ln -s busybox nosuch     argv[0] 当 applet 名    argv[0]      127
//! builtin     sh -c 'nosuch'           内建表与 PATH 都未命中  builtin      127
//! sh-cmd      sh -c '/tmp/nosuch'      PATH 命中但不可执行     path-search  126
//! exec        execvp("nosuch")         exec 层直接 ENOENT      execve         1
//! ```
//!
//! 为什么必须分五条：同一条"未知 applet"在五个主体手里的**判决权**不一样。
//! 分发层与 shell 都按 POSIX 的 `126/127` 分档；exec 层不判档，它只把 `-1`
//! 交回去，落什么码由调用方定——coreutils 系惯例是 `1`，`env` 系是 `127`。
//! 把 exec 场景的 127 当成本卡的基准，等于把调用方的习惯当成了协议。
//!
//! 三条码族带（本卡的粗判）：
//!
//! ```text
//! Swallow {0}        吞错带：退出 0 而命令没跑——任何场景都不许落这一带
//! Tool    {1, 2}     工具带：调用方自决(1) 与自身用法错(2)
//! Parse   {126,127}  解析带：126 命中不可执行 / 127 查表未命中
//! ```
//!
//! 先判带再判码：`Exec` 观测到 `2` 属"同带错码"（Tool 带里落错），而观测到
//! `0` 属"跨带吞错"。两种缺陷的修法完全不同——前者是调用方映射表写错，后者
//! 是把失败当成功上报。合成一个"码不符"会把这两件事永久混在一起。
//!
//! 唯一登记过的例外：`sh-cmd` 的 `126` 在 dash / ash / bash 之间对"PATH 命中
//! 但是目录"这一形态并不完全一致，少数实现在目录档上给 `127`。这一格作为
//! **登记例外**放行，但放行必须带凭据（`excused`），无凭据的 `127` 判死，
//! 非 `sh-cmd` 场景拿例外来挡也判死——例外不是挡箭牌，是台账。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 八段，秩 1..=8，只许前进：
//!
//! 1. **Arg**——码入表。退出码必须在 `CODE_TABLE` 五元里；表外的码（含负数、
//!    含 200 以上）连"落在哪一格"都问不出来，必须先判死。
//! 2. **Layer**——文案层归属。dispatch 场景必须由分发层出话，shell 场景必须由
//!    shell 出话，exec 场景**必须没有人出话**（exec 层只回错误号）。层错了，
//!    后面比文案全是比错的层。
//! 3. **Lookup**——查表结论。未知 applet 必须是 miss；查表竟然命中，说明这条
//!    现场根本不是"未知 applet"，后面的码再对也不是这一卡的样本。
//! 4. **Text**——文案体例。前缀、名字、尾巴三处逐字；`sh: 1:` 里的 `1` 是
//!    shell 的 `$?` 占位，不许省略。
//! 5. **Band**——码族带。粗判，先抓住"吞错"与"用法"两类跨带。
//! 6. **Code**——精确码。细判，抓同带错码。
//! 7. **Grid**——矩阵格位。格位必须与闸门结论同源：登记例外要凭据，无凭据的
//!    例外格判死。
//! 8. **Rate**——矩阵结算。分母是二十五格，不剔除例外格。
//!
//! 次序不能倒：Layer 在 Text 之前（层错了，文案对齐毫无意义）；Lookup 在 Code
//! 之前（不是未知 applet，码对不对都不进这张矩阵）；Band 在 Code 之前（先分带
//! 再分码，否则"吞错"会被吞进"码不符"里）；Grid 在 Code 之后（格位是结论，
//! 结论不能反过来当依据）。
//!
//! ## 关键差异（本卡深度所在，谁判死谁放行）
//!
//! * **与 F11675（双路 127 对照，段 `0x6d290`）分层**：F11675 只摆两路——分发
//!   层 127 与 exec 层 127——判"两路文案不同但码相同"；本卡把 exec 的码拆开
//!   （它本来就不是 127），并加上 `126`、`1`、`0`、`2` 四列，成矩阵。
//! * **与 F11676（三档裁剪矩阵，段 `0x6d2a0`）分层**：F11676 的矩阵是**档位 ×
//!   路径**（3 × 6，问"路还在不在"）；本卡是**场景 × 码**（5 × 5，问"码落不
//!   落"）。两卡都把退出码钉成常量，但 F11676 里 127 是常量，本卡里 127 只是
//!   五列之一。
//! * **与 F11678（stderr/stdout 分离，段 `0x6d2d0`）分层**：F11678 判"话走哪个
//!   流"，本卡判"落哪个码"。exec 场景在本卡里合法地**一个字都不打**——这在
//!   F11678 里是空流，在本卡里是"Caller 层"，两卡不互相否定。
//! * **与 F11680（本批聚合，段 `0x6d2f0`）分层**：F11680 聚合 19 条判据的**通过
//!   率**；本卡聚合 25 个**矩阵格**。条数全绿而格位有悬空，F11680 绿、本卡红。
//! * **谁判死**：码不在表、层不对、查表命中、文案体例错、跨带（尤其 0）、
//!   同带错码、无凭据的例外——一律判死，且各落不同的码，使"是吞错"与"是码
//!   错"分得开。
//! * **谁放行**：格位是 `Lands` 且五闸全过 → `Lands` 档；格位是 `Excused` 且有
//!   登记凭据、场景确实是 `sh-cmd` → `Excused` 档。两档都进分子。
//! * **零悬空**：`ExitGrid::decided()` 必须为真，`lands + excused + contradicts`
//!   必须等于 25；任何一格没有判决，本卡整卡红。
//!
//! 语料十四槽：前八槽覆盖五条 Lands 格与唯一 Excused 格（含同格不同形态的两条
//! 重复抽样），后六槽各坏一维（estate/stage/code/value/note/default）。账本十四
//! 格 = 七统计 + 一红档率（929）+ 六缺陷码，冻结后重放同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本卡段基址（全仓唯一，VX5 分配表给定，不许改）。
pub const SEG_BASE: u32 = 0x6d2e0;

// ---------------------------------------------------------------------------
// 联签锚
// ---------------------------------------------------------------------------

/// 前置卡 F11678（notfound stderr/stdout 分离）的段基址。
pub const ANCHOR_F11678_SEG: u32 = 0x6d2d0;

/// 直接后继卡 F11680（B24 批聚合）的段基址。
pub const ANCHOR_F11680_SEG: u32 = 0x6d2f0;

/// 场景族上游 F11675（双路 127 对照）的段基址。
pub const ANCHOR_F11675_SEG: u32 = 0x6d290;

/// F11675 与本卡的段位差（双路 → 矩阵）。
pub const ANCHOR_F11675_GAP: u32 = SEG_BASE - ANCHOR_F11675_SEG;

// ---------------------------------------------------------------------------
// 规模常量（契约给定，逐卡一致）
// ---------------------------------------------------------------------------

/// 语料槽数。
pub const CORPUS: usize = 14;
/// 账本槽数。
pub const SAMPLES: usize = 14;
/// 正例槽数。
pub const ACCEPT: usize = 8;
/// 反例槽数。
pub const REJECT: usize = 6;
/// 矩阵档位数。
pub const TIER_COUNT: usize = 4;
/// 闸门段数。
pub const STAGE_COUNT: u32 = 8;
/// 闸门末段秩。
pub const STAGE_LAST_RANK: u32 = 8;
/// 统计格数。
pub const STAT_CELLS: usize = 7;
/// 红档率千分比。
pub const RED_RATE: u32 = 929;
/// 满额千分比。
pub const RATE_FULL: u32 = 1000;

// ---------------------------------------------------------------------------
// 本卡规模：五场景 × 五码 = 二十五格
// ---------------------------------------------------------------------------

/// 场景数（矩阵行数）。
pub const SCENE_COUNT: usize = 5;
/// 退出码数（矩阵列数）。
pub const CODE_COUNT: usize = 5;
/// 矩阵格数。
pub const GRID_CELLS: usize = SCENE_COUNT * CODE_COUNT;
/// 落到 `Lands` 的格数（五条场景各一格）。
pub const LANDS_CELLS: u32 = 5;
/// 落到 `Excused` 的格数（sh-cmd 的 127 方言例外）。
pub const EXCUSED_CELLS: u32 = 1;
/// 落到 `Contradicts` 的格数（25 - 5 - 1）。
pub const CONTRADICTS_CELLS: u32 = 19;

// ---------------------------------------------------------------------------
// 退出码（五列）
// ---------------------------------------------------------------------------

/// 成功——命令没跑却退 0，属吞错带。
pub const EXIT_ZERO: u32 = 0;
/// 一般失败——exec 层调用方的惯例落点。
pub const EXIT_FAIL: u32 = 1;
/// 用法错——工具自身参数问题，非"未知 applet"。
pub const EXIT_USAGE: u32 = 2;
/// 命中但不可执行（EACCES/EISDIR）。
pub const EXIT_NOEXEC: u32 = 126;
/// 查表未命中（POSIX shell 的 not found）。
pub const EXIT_NOTFOUND: u32 = 127;

/// 五列码表（次序即矩阵列序）。
pub const CODE_TABLE: [u32; CODE_COUNT] = [
    EXIT_ZERO,
    EXIT_FAIL,
    EXIT_USAGE,
    EXIT_NOEXEC,
    EXIT_NOTFOUND,
];

// ---------------------------------------------------------------------------
// 错误码（13 个，SEG_BASE + 0x01..0x0d）
// ---------------------------------------------------------------------------

/// 入参非法：退出码不在五元表内。
pub const EM_EARG: u32 = SEG_BASE + 0x01;
/// 文案层归属错。
pub const EM_ELAYER: u32 = SEG_BASE + 0x02;
/// 查表结论错（未知 applet 竟然命中）。
pub const EM_ELOOK: u32 = SEG_BASE + 0x03;
/// 文案体例错。
pub const EM_ETEXT: u32 = SEG_BASE + 0x04;
/// 码族带跨带（含吞错带）。
pub const EM_EBAND: u32 = SEG_BASE + 0x05;
/// 同带错码。
pub const EM_ECODE: u32 = SEG_BASE + 0x06;
/// 阶段不符或倒退。
pub const EM_ESTAGE: u32 = SEG_BASE + 0x07;
/// 成败互报。
pub const EM_ESTATE: u32 = SEG_BASE + 0x08;
/// 统计值不符。
pub const EM_EVALUE: u32 = SEG_BASE + 0x09;
/// 注记不符。
pub const EM_ENOTE: u32 = SEG_BASE + 0x0a;
/// 兜底档被当成结论。
pub const EM_EDEFAULT: u32 = SEG_BASE + 0x0b;
/// 槽位越界。
pub const EM_ESLOT: u32 = SEG_BASE + 0x0c;
/// 矩阵格位与闸门不同源。
pub const EM_EGRID: u32 = SEG_BASE + 0x0d;

/// 十三码全表。
pub const ALL_CODES: [u32; 13] = [
    EM_EARG, EM_ELAYER, EM_ELOOK, EM_ETEXT, EM_EBAND, EM_ECODE, EM_ESTAGE, EM_ESTATE, EM_EVALUE,
    EM_ENOTE, EM_EDEFAULT, EM_ESLOT, EM_EGRID,
];

/// 反例六维对应的六个码。
pub const REJECT_CODES: [u32; REJECT] = [EM_ESTATE, EM_ESTAGE, EM_ECODE, EM_EVALUE, EM_ENOTE, EM_EDEFAULT];

/// 七个统计格的钉值 + 第八格红档率（929）。
pub const VALS: [u32; ACCEPT] = [
    SCENE_COUNT as u32,
    CODE_COUNT as u32,
    GRID_CELLS as u32,
    LANDS_CELLS,
    EXCUSED_CELLS,
    CONTRADICTS_CELLS,
    RATE_FULL,
    RED_RATE,
];

/// 七个统计格归属的段号。
pub const STAT_OPS: [u8; STAT_CELLS] = [1, 2, 3, 4, 5, 6, 7];

/// 红档格在账本里的段号。
pub const OPID_STAT_RED: u8 = 0x08;
/// 六个缺陷码格的段号起点。
pub const DEFECT_OP_BASE: u8 = 0x21;

// ---------------------------------------------------------------------------
// FNV-1a 指纹
// ---------------------------------------------------------------------------

/// FNV-1a 初始偏移量。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折一字节进哈希。
fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// 折一个 u64。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut v = w;
    let mut acc = h;
    let mut i = 0usize;
    while i < 8 {
        acc = fold_byte(acc, (v & 0xff) as u8);
        v = v >> 8;
        i += 1;
    }
    acc
}

/// 折一条字符串。
fn fold_str(h: u64, s: &str) -> u64 {
    let mut acc = h;
    for b in s.as_bytes() {
        acc = fold_byte(acc, *b);
    }
    acc
}

/// 折一个 i32。
fn fold_i32(h: u64, v: i32) -> u64 {
    fold_word(h, v as u32 as u64)
}

/// 折一个布尔。
fn fold_bool(h: u64, v: bool) -> u64 {
    fold_byte(h, if v { 1 } else { 0 })
}

// ---------------------------------------------------------------------------
// 数字/布尔文本
// ---------------------------------------------------------------------------

/// u32 → 十进制文本。
fn dec_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    while n > 0 && i > 0 {
        let d = (n % 10) as u8;
        n /= 10;
        i -= 1;
        if let Some(cell) = buf.get_mut(i) {
            *cell = b'0' + d;
        }
    }
    let mut s = String::new();
    let mut j = i;
    while j < 10 {
        if let Some(c) = buf.get(j) {
            s.push(*c as char);
        }
        j += 1;
    }
    s
}

/// u32 → 定宽十六进制文本。
fn hex_text(v: u32, digits: usize) -> String {
    let table: &[u8; 16] = b"0123456789ABCDEF";
    let mut s = String::new();
    if digits == 0 {
        return s;
    }
    let mut i = digits;
    while i > 0 {
        i -= 1;
        let nib = ((v >> (i * 4)) & 0xf) as usize;
        let c = match table.get(nib) {
            Some(x) => *x,
            None => b'0',
        };
        s.push(c as char);
    }
    s
}

/// u32 文本别名。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// i32 文本（负号显式）。
fn i32_text(v: i32) -> String {
    if v < 0 {
        let mut s = String::from("-");
        s.push_str(dec_text((v as i64).unsigned_abs() as u32).as_str());
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

/// 段基址文本。
pub fn seg_text(v: u32) -> String {
    let mut s = String::from("0x");
    s.push_str(hex_text(v, 5).as_str());
    s
}

/// 千分比文本。
pub fn permille_text(v: u32) -> String {
    let mut s = dec_text(v);
    s.push_str("permille");
    s
}

/// 退出码文本：`exit=127`。
pub fn code_text(v: u32) -> String {
    let mut s = String::from("exit=");
    s.push_str(dec_text(v).as_str());
    s
}

// ---------------------------------------------------------------------------
// 上游文案逐字复刻
// ---------------------------------------------------------------------------

/// busybox 分发层尾巴。
pub const T_APPLET_NOT_FOUND: &str = ": applet not found";
/// ash 前缀。
pub const T_SHELL_PREFIX: &str = "sh: ";
/// ash 命令未命中尾巴。
pub const T_NOT_FOUND: &str = ": not found";
/// ash 行号占位（`sh: 1:` 里的 `1`）。
pub const T_SHELL_LINE: &str = "1: ";
/// 不可执行尾巴（EACCES strerror 原文）。
pub const T_PERMISSION: &str = ": Permission denied";
/// 目录当可执行件（EISDIR strerror 原文）。
pub const T_IS_DIR: &str = ": Is a directory";
/// 空文案哨兵：exec 层合法地什么都不打。
pub const T_SILENT: &str = "";
/// 分发层 / argv[0] 档的完整一行。
pub const T_BUSYBOX_LINE: &str = "nosuch: applet not found";
/// 内建表与 PATH 双未命中的完整一行。
pub const T_BUILTIN_LINE: &str = "sh: nosuch: not found";
/// PATH 命中但不可执行的完整一行（含 `1` 行号占位）。
pub const T_SHCMD_LINE: &str = "sh: 1: /tmp/nosuch: Permission denied";
/// 目录档的完整一行（少数实现在目录上报 127，文案走 EISDIR）。
pub const T_SHCMD_DIR_LINE: &str = "sh: 1: /tmp/nosuch: Is a directory";
/// 漏掉行号占位的错体（`sh: 1:` 里的 `1` 不许省）。
pub const T_SHCMD_NOLINE: &str = "sh: /tmp/nosuch: Permission denied";

/// `nosuch: applet not found`。
pub fn msg_applet_not_found(name: &str) -> String {
    let mut s = String::new();
    s.push_str(name);
    s.push_str(T_APPLET_NOT_FOUND);
    s
}

/// `sh: nosuch: not found`。
pub fn msg_shell_not_found(name: &str) -> String {
    let mut s = String::from(T_SHELL_PREFIX);
    s.push_str(name);
    s.push_str(T_NOT_FOUND);
    s
}

/// `sh: 1: /tmp/nosuch: Permission denied`。
pub fn msg_shell_noexec(name: &str) -> String {
    let mut s = String::from(T_SHELL_PREFIX);
    s.push_str(T_SHELL_LINE);
    s.push_str(name);
    s.push_str(T_PERMISSION);
    s
}

/// `sh: 1: /tmp/nosuch: Is a directory`。
pub fn msg_shell_isdir(name: &str) -> String {
    let mut s = String::from(T_SHELL_PREFIX);
    s.push_str(T_SHELL_LINE);
    s.push_str(name);
    s.push_str(T_IS_DIR);
    s
}

/// 格位文本：`dispatch x 127 -> lands`。
pub fn msg_cell(scene: &str, code: u32, verdict: &str) -> String {
    let mut s = String::new();
    s.push_str(scene);
    s.push_str(" x ");
    s.push_str(dec_text(code).as_str());
    s.push_str(" -> ");
    s.push_str(verdict);
    s
}

/// 格位矛盾：`dispatch x 0 contradicts 127`。
pub fn msg_contradicts(scene: &str, code: u32, want: u32) -> String {
    let mut s = String::new();
    s.push_str(scene);
    s.push_str(" x ");
    s.push_str(dec_text(code).as_str());
    s.push_str(" contradicts ");
    s.push_str(dec_text(want).as_str());
    s
}

/// 吞错：`dispatch exited 0 without running the applet`。
pub fn msg_swallow(scene: &str) -> String {
    let mut s = String::new();
    s.push_str(scene);
    s.push_str(" exited 0 without running the applet");
    s
}

/// 跨带：`exec x 127 crosses into band parse`。
pub fn msg_cross_band(scene: &str, code: u32, band: &str) -> String {
    let mut s = String::new();
    s.push_str(scene);
    s.push_str(" x ");
    s.push_str(dec_text(code).as_str());
    s.push_str(" crosses into band ");
    s.push_str(band);
    s
}

/// 层错：`dispatch spoke through layer shell`。
pub fn msg_layer_wrong(scene: &str, layer: &str) -> String {
    let mut s = String::new();
    s.push_str(scene);
    s.push_str(" spoke through layer ");
    s.push_str(layer);
    s
}

/// 查表命中：`nosuch was found in the applet table`。
pub fn msg_lookup_hit(name: &str) -> String {
    let mut s = String::new();
    s.push_str(name);
    s.push_str(" was found in the applet table");
    s
}

/// 体例错：`sh-cmd text does not match the ash template`。
pub fn msg_text_wrong(scene: &str) -> String {
    let mut s = String::new();
    s.push_str(scene);
    s.push_str(" text does not match the upstream template");
    s
}

/// 无凭据的例外。
pub fn msg_unregistered_excuse(scene: &str) -> String {
    let mut s = String::new();
    s.push_str(scene);
    s.push_str(" claims an excused cell without a ledger entry");
    s
}

/// 例外用错了场景。
pub fn msg_phantom_excuse(scene: &str, code: u32) -> String {
    let mut s = String::new();
    s.push_str(scene);
    s.push_str(" x ");
    s.push_str(dec_text(code).as_str());
    s.push_str(" is not an excused cell");
    s
}

/// 一致率不足。
pub fn msg_rate_short(matched: u32, total: u32) -> String {
    let mut s = String::from("exit grid rate ");
    s.push_str(dec_text(matched).as_str());
    s.push('/');
    s.push_str(dec_text(total).as_str());
    s.push_str(" below ");
    s.push_str(dec_text(RATE_FULL).as_str());
    s
}

/// 用法。
pub fn msg_usage() -> String {
    String::from("usage: exit-grid <scene> <code>")
}

/// 用法错误的退出码（工具自身参数问题，不是本卡样本）。
pub const EXIT_USAGE_TOOL: u32 = 2;

// ---------------------------------------------------------------------------
// 五条场景
// ---------------------------------------------------------------------------

/// 未知 applet 的五条现场。判决主体不同，判决权也不同。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scene {
    /// `busybox nosuch`：分发层查 `argv[1]` 未命中。
    Dispatch,
    /// `ln -s busybox nosuch; ./nosuch`：`argv[0]` 当 applet 名。
    Alias,
    /// `sh -c 'nosuch'`：内建表与 PATH 双未命中。
    Builtin,
    /// `sh -c '/tmp/nosuch'`：PATH 命中但不可执行。
    ShCmd,
    /// `execvp("nosuch")`：exec 层直接给 `-1`。
    Exec,
}

impl Scene {
    /// 五条全表（次序即矩阵行序）。
    pub fn all() -> [Scene; SCENE_COUNT] {
        [
            Scene::Dispatch,
            Scene::Alias,
            Scene::Builtin,
            Scene::ShCmd,
            Scene::Exec,
        ]
    }

    /// 行名（矩阵行序即此序）。
    pub fn name(&self) -> &'static str {
        match *self {
            Scene::Dispatch => "dispatch",
            Scene::Alias => "alias",
            Scene::Builtin => "builtin",
            Scene::ShCmd => "sh-cmd",
            Scene::Exec => "exec",
        }
    }

    /// 行号。
    pub fn id(&self) -> usize {
        match *self {
            Scene::Dispatch => 0,
            Scene::Alias => 1,
            Scene::Builtin => 2,
            Scene::ShCmd => 3,
            Scene::Exec => 4,
        }
    }

    /// 由行号构造；越界给 None，绝不夹带默认值。
    pub fn of_id(i: usize) -> Option<Scene> {
        match i {
            0 => Some(Scene::Dispatch),
            1 => Some(Scene::Alias),
            2 => Some(Scene::Builtin),
            3 => Some(Scene::ShCmd),
            4 => Some(Scene::Exec),
            _ => None,
        }
    }

    /// 这条现场由谁出话。
    pub fn layer(&self) -> Layer {
        match *self {
            Scene::Dispatch | Scene::Alias => Layer::Busybox,
            Scene::Builtin | Scene::ShCmd => Layer::Shell,
            Scene::Exec => Layer::Caller,
        }
    }

    /// 这条现场该落哪条码族带。
    pub fn band(&self) -> CodeBand {
        match *self {
            Scene::Dispatch | Scene::Alias | Scene::Builtin | Scene::ShCmd => CodeBand::Parse,
            Scene::Exec => CodeBand::Tool,
        }
    }

    /// 这条现场该落的那一格（矩阵里唯一合法的 `Lands` 格）。
    pub fn expected_code(&self) -> u32 {
        match *self {
            Scene::Dispatch | Scene::Alias | Scene::Builtin => EXIT_NOTFOUND,
            Scene::ShCmd => EXIT_NOEXEC,
            Scene::Exec => EXIT_FAIL,
        }
    }

    /// 这条现场的名字从哪来（argv 位置是"谁在查表"的直接证据）。
    pub fn argv_source(&self) -> &'static str {
        match *self {
            Scene::Dispatch => "argv[1]",
            Scene::Alias => "argv[0]",
            Scene::Builtin => "builtin",
            Scene::ShCmd => "path-search",
            Scene::Exec => "execve",
        }
    }

    /// 上游逐字文案（常量侧；与 `msg_*` 拼装侧互为第二形式）。
    pub fn text(&self) -> &'static str {
        match *self {
            Scene::Dispatch | Scene::Alias => T_BUSYBOX_LINE,
            Scene::Builtin => T_BUILTIN_LINE,
            Scene::ShCmd => T_SHCMD_LINE,
            Scene::Exec => T_SILENT,
        }
    }

    /// 这条现场是否有例外资格（只有 `sh-cmd` 的目录档有方言分歧）。
    pub fn excuses_127(&self) -> bool {
        match *self {
            Scene::ShCmd => true,
            _ => false,
        }
    }

    /// 文案是否属于这条现场的合法体例：主体例一字不差，或（仅 sh-cmd）目录档体例。
    pub fn text_ok(&self, text: &str) -> bool {
        if text == self.text() {
            return true;
        }
        if self.excuses_127() && text == T_SHCMD_DIR_LINE {
            return true;
        }
        false
    }

    /// 行指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_byte(h, self.id() as u8);
        h = fold_str(h, self.name());
        h = fold_str(h, self.argv_source());
        h = fold_word(h, self.expected_code() as u64);
        h
    }
}

/// 出话的层。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    /// busybox 分发层（含 argv[0] 档）。
    Busybox,
    /// shell（ash / dash / bash）。
    Shell,
    /// 调用方——exec 层只交错误号，一个字都不打。
    Caller,
}

impl Layer {
    /// 三层全表。
    pub fn all() -> [Layer; 3] {
        [Layer::Busybox, Layer::Shell, Layer::Caller]
    }

    /// 层名。
    pub fn name(&self) -> &'static str {
        match *self {
            Layer::Busybox => "busybox",
            Layer::Shell => "shell",
            Layer::Caller => "caller",
        }
    }

    /// 该层是否在 notfound 上出话。
    pub fn speaks(&self) -> bool {
        match *self {
            Layer::Busybox | Layer::Shell => true,
            Layer::Caller => false,
        }
    }

    /// 层号。
    pub fn id(&self) -> u8 {
        match *self {
            Layer::Busybox => 1,
            Layer::Shell => 2,
            Layer::Caller => 3,
        }
    }
}

/// 码族带——粗判先分带，再分码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CodeBand {
    /// 吞错带 `{0}`：退 0 而命令没跑。
    Swallow,
    /// 工具带 `{1, 2}`。
    Tool,
    /// 解析带 `{126, 127}`。
    Parse,
}

impl CodeBand {
    /// 三带全表。
    pub fn all() -> [CodeBand; 3] {
        [CodeBand::Swallow, CodeBand::Tool, CodeBand::Parse]
    }

    /// 带名。
    pub fn name(&self) -> &'static str {
        match *self {
            CodeBand::Swallow => "swallow",
            CodeBand::Tool => "tool",
            CodeBand::Parse => "parse",
        }
    }

    /// 带号。
    pub fn id(&self) -> u8 {
        match *self {
            CodeBand::Swallow => 1,
            CodeBand::Tool => 2,
            CodeBand::Parse => 3,
        }
    }

    /// 该带是否允许出现在本卡矩阵里（吞错带永远不许）。
    pub fn legal(&self) -> bool {
        match *self {
            CodeBand::Swallow => false,
            CodeBand::Tool | CodeBand::Parse => true,
        }
    }
}

/// 一条码属于哪一带；表外码给 None。
pub fn band_of(code: u32) -> Option<CodeBand> {
    match code {
        EXIT_ZERO => Some(CodeBand::Swallow),
        EXIT_FAIL | EXIT_USAGE => Some(CodeBand::Tool),
        EXIT_NOEXEC | EXIT_NOTFOUND => Some(CodeBand::Parse),
        _ => None,
    }
}

/// 码是否在五元表内。
pub fn code_in_table(code: u32) -> bool {
    let mut i = 0usize;
    while i < CODE_COUNT {
        match CODE_TABLE.get(i) {
            Some(x) => {
                if *x == code {
                    return true;
                }
            }
            None => return false,
        }
        i += 1;
    }
    false
}

/// 码在矩阵里的列号。
pub fn code_index(code: u32) -> Option<usize> {
    let mut i = 0usize;
    while i < CODE_COUNT {
        match CODE_TABLE.get(i) {
            Some(x) => {
                if *x == code {
                    return Some(i);
                }
            }
            None => return None,
        }
        i += 1;
    }
    None
}

/// 一格的判决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellVerdict {
    /// 这条场景真会落到这个码。
    Lands,
    /// 登记过的方言例外格（仅 sh-cmd × 127）。
    Excused,
    /// 明确判死的矛盾格。
    Contradicts,
}

impl CellVerdict {
    /// 判决名。
    pub fn name(&self) -> &'static str {
        match *self {
            CellVerdict::Lands => "lands",
            CellVerdict::Excused => "excused",
            CellVerdict::Contradicts => "contradicts",
        }
    }

    /// 是否进分子。
    pub fn counts(&self) -> bool {
        match *self {
            CellVerdict::Lands | CellVerdict::Excused => true,
            CellVerdict::Contradicts => false,
        }
    }

    /// 是否算绿（矛盾格永远不绿）。
    pub fn is_green(&self) -> bool {
        self.counts()
    }
}

/// 一格的判决规则：与场景该落的码相等即 `Lands`；sh-cmd 的 127 是登记例外；
/// 其余一律 `Contradicts`。**没有第四种结局**——矩阵不许留白。
pub fn verdict_of(scene: Scene, code: u32) -> CellVerdict {
    if code == scene.expected_code() {
        return CellVerdict::Lands;
    }
    if scene.excuses_127() && code == EXIT_NOTFOUND {
        return CellVerdict::Excused;
    }
    CellVerdict::Contradicts
}

/// 五场景 × 五码的判决矩阵。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExitGrid {
    cells: [[CellVerdict; CODE_COUNT]; SCENE_COUNT],
}

impl ExitGrid {
    /// 建矩阵：逐格按 `verdict_of` 定案，绝不留空。
    pub fn build() -> ExitGrid {
        let mut cells = [[CellVerdict::Contradicts; CODE_COUNT]; SCENE_COUNT];
        let scenes = Scene::all();
        let mut r = 0usize;
        while r < SCENE_COUNT {
            let scene = match scenes.get(r) {
                Some(x) => *x,
                None => return ExitGrid { cells },
            };
            let mut c = 0usize;
            while c < CODE_COUNT {
                let code = match CODE_TABLE.get(c) {
                    Some(x) => *x,
                    None => return ExitGrid { cells },
                };
                let v = verdict_of(scene, code);
                if let Some(row) = cells.get_mut(r) {
                    if let Some(cell) = row.get_mut(c) {
                        *cell = v;
                    }
                }
                c += 1;
            }
            r += 1;
        }
        ExitGrid { cells }
    }

    /// 取一格（按场景与码）。
    pub fn cell(&self, scene: Scene, code: u32) -> Option<CellVerdict> {
        let c = code_index(code)?;
        let row = self.cells.get(scene.id())?;
        match row.get(c) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 取一格（按行列号）。
    pub fn cell_at(&self, r: usize, c: usize) -> Option<CellVerdict> {
        let row = self.cells.get(r)?;
        match row.get(c) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// `Lands` 格数。
    pub fn lands(&self) -> u32 {
        self.count(CellVerdict::Lands)
    }

    /// `Excused` 格数。
    pub fn excused(&self) -> u32 {
        self.count(CellVerdict::Excused)
    }

    /// `Contradicts` 格数。
    pub fn contradicts(&self) -> u32 {
        self.count(CellVerdict::Contradicts)
    }

    /// 数某种判决的格数。
    fn count(&self, want: CellVerdict) -> u32 {
        let mut n = 0u32;
        let mut r = 0usize;
        while r < SCENE_COUNT {
            let mut c = 0usize;
            while c < CODE_COUNT {
                if let Some(v) = self.cell_at(r, c) {
                    if v == want {
                        n += 1;
                    }
                }
                c += 1;
            }
            r += 1;
        }
        n
    }

    /// 零悬空：三档格数之和必须等于格总数。
    pub fn decided(&self) -> bool {
        self.lands() + self.excused() + self.contradicts() == GRID_CELLS as u32
    }

    /// 是否每格都有判决（无洞、无重复计入）。
    pub fn every_cell_judged(&self) -> bool {
        let mut r = 0usize;
        while r < SCENE_COUNT {
            let mut c = 0usize;
            while c < CODE_COUNT {
                if self.cell_at(r, c).is_none() {
                    return false;
                }
                c += 1;
            }
            r += 1;
        }
        true
    }

    /// 一条场景的格位串：`dispatch 127=lands 0=contradicts ...`。
    pub fn row_text(&self, scene: Scene) -> String {
        let mut s = String::from(scene.name());
        s.push(' ');
        let mut c = 0usize;
        while c < CODE_COUNT {
            let code = match CODE_TABLE.get(c) {
                Some(x) => *x,
                None => return s,
            };
            let v = match self.cell(scene, code) {
                Some(x) => x,
                None => return s,
            };
            if c > 0 {
                s.push(' ');
            }
            s.push_str(dec_text(code).as_str());
            s.push('=');
            s.push_str(v.name());
            c += 1;
        }
        s
    }

    /// 呈现全矩阵（每行一条）。
    pub fn render(&self) -> String {
        let scenes = Scene::all();
        let mut s = String::from("exit-grid");
        let mut r = 0usize;
        while r < SCENE_COUNT {
            let scene = match scenes.get(r) {
                Some(x) => *x,
                None => return s,
            };
            s.push('\n');
            s.push_str(self.row_text(scene).as_str());
            r += 1;
        }
        s
    }

    /// 矩阵指纹：行序 + 每格判决，改一格即改指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let scenes = Scene::all();
        let mut r = 0usize;
        while r < SCENE_COUNT {
            if let Some(scene) = scenes.get(r) {
                h = fold_word(h, scene.seal());
            }
            let mut c = 0usize;
            while c < CODE_COUNT {
                if let Some(v) = self.cell_at(r, c) {
                    h = fold_str(h, v.name());
                }
                c += 1;
            }
            r += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 八闸
// ---------------------------------------------------------------------------

/// 闸门段（秩 1..=8）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 码入表。
    Arg,
    /// 文案层归属。
    Layer,
    /// 查表结论。
    Lookup,
    /// 文案体例。
    Text,
    /// 码族带。
    Band,
    /// 精确码。
    Code,
    /// 矩阵格位。
    Grid,
    /// 一致率结算。
    Rate,
}

impl Stage {
    /// 八闸全表（顺序即纵线）。
    pub fn all() -> [Stage; 8] {
        [
            Stage::Arg,
            Stage::Layer,
            Stage::Lookup,
            Stage::Text,
            Stage::Band,
            Stage::Code,
            Stage::Grid,
            Stage::Rate,
        ]
    }

    /// 秩（1 起）。
    pub fn rank(&self) -> u32 {
        match *self {
            Stage::Arg => 1,
            Stage::Layer => 2,
            Stage::Lookup => 3,
            Stage::Text => 4,
            Stage::Band => 5,
            Stage::Code => 6,
            Stage::Grid => 7,
            Stage::Rate => 8,
        }
    }

    /// 由秩构造；越界给 None。
    pub fn of_rank(r: u32) -> Option<Stage> {
        match r {
            1 => Some(Stage::Arg),
            2 => Some(Stage::Layer),
            3 => Some(Stage::Lookup),
            4 => Some(Stage::Text),
            5 => Some(Stage::Band),
            6 => Some(Stage::Code),
            7 => Some(Stage::Grid),
            8 => Some(Stage::Rate),
            _ => None,
        }
    }

    /// 阶段名。
    pub fn name(&self) -> &'static str {
        match *self {
            Stage::Arg => "arg",
            Stage::Layer => "layer",
            Stage::Lookup => "lookup",
            Stage::Text => "text",
            Stage::Band => "band",
            Stage::Code => "code",
            Stage::Grid => "grid",
            Stage::Rate => "rate",
        }
    }

    /// 该段挂的码。
    pub fn fault_code(&self) -> u32 {
        match *self {
            Stage::Arg => EM_EARG,
            Stage::Layer => EM_ELAYER,
            Stage::Lookup => EM_ELOOK,
            Stage::Text => EM_ETEXT,
            Stage::Band => EM_EBAND,
            Stage::Code => EM_ECODE,
            Stage::Grid => EM_EGRID,
            Stage::Rate => EM_EDEFAULT,
        }
    }

    /// 是否在"已出结论"之后（Grid 起不可回退到码）。
    pub fn is_conclusion(&self) -> bool {
        self.rank() >= Stage::Grid.rank()
    }
}

// ---------------------------------------------------------------------------
// 注记 / 缺陷维
// ---------------------------------------------------------------------------

/// 注记——每一格"为什么"。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// 无注记（兜底落点）。
    Nil,
    /// 落到唯一合法格。
    Lands,
    /// 命中登记例外格。
    RegisteredExcuse,
    /// 码不在五元表。
    BadCode,
    /// 出话层不对。
    LayerWrong,
    /// 查表竟然命中。
    LookupHit,
    /// 文案体例不对。
    TextWrong,
    /// 跨带（含吞错带）。
    CrossBand,
    /// 同带错码。
    SameBandWrong,
    /// 例外没有凭据或用错场景。
    PhantomExcuse,
    /// 格位与闸门不同源。
    GridContradiction,
}

impl Note {
    /// 注记名。
    pub fn name(&self) -> &'static str {
        match *self {
            Note::Nil => "nil",
            Note::Lands => "lands",
            Note::RegisteredExcuse => "registered-excuse",
            Note::BadCode => "bad-code",
            Note::LayerWrong => "layer-wrong",
            Note::LookupHit => "lookup-hit",
            Note::TextWrong => "text-wrong",
            Note::CrossBand => "cross-band",
            Note::SameBandWrong => "same-band-wrong",
            Note::PhantomExcuse => "phantom-excuse",
            Note::GridContradiction => "grid-contradiction",
        }
    }

    /// 该注记是否让整条现场作废（不进分母）。
    pub fn voids(&self) -> bool {
        match *self {
            Note::BadCode | Note::LayerWrong | Note::LookupHit => true,
            _ => false,
        }
    }

    /// 该注记是否属于"格位"维（矩阵与闸门不同源）。
    pub fn is_grid(&self) -> bool {
        match *self {
            Note::PhantomExcuse | Note::GridContradiction => true,
            _ => false,
        }
    }
}

/// 缺陷维——申报对判的落点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 成败互报。
    Estate,
    /// 阶段不符。
    Stage,
    /// 错误码不符。
    Code,
    /// 数值不符。
    Value,
    /// 注记不符。
    Note,
    /// 兜底档被当成结论。
    Default,
    /// 无缺陷。
    Ok,
}

impl Fault {
    /// 六维反例表（契约给定次序）。
    pub fn reject_faults() -> [Fault; REJECT] {
        [
            Fault::Estate,
            Fault::Stage,
            Fault::Code,
            Fault::Value,
            Fault::Note,
            Fault::Default,
        ]
    }

    /// 缺陷维对应的错误码。
    pub fn code(&self) -> u32 {
        match *self {
            Fault::Estate => EM_ESTATE,
            Fault::Stage => EM_ESTAGE,
            Fault::Code => EM_ECODE,
            Fault::Value => EM_EVALUE,
            Fault::Note => EM_ENOTE,
            Fault::Default => EM_EDEFAULT,
            Fault::Ok => 0,
        }
    }

    /// 缺陷维名。
    pub fn name(&self) -> &'static str {
        match *self {
            Fault::Estate => "estate",
            Fault::Stage => "stage",
            Fault::Code => "code",
            Fault::Value => "value",
            Fault::Note => "note",
            Fault::Default => "default",
            Fault::Ok => "ok",
        }
    }
}

/// 反例六维表（复写一份，和 `Fault::reject_faults()` 交叉校验）。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

// ---------------------------------------------------------------------------
// 现场 / 结果 / 申报 / 语料
// ---------------------------------------------------------------------------

/// 一条现场：五维观测（场景、码、层、查表、文案）加三个结算开关。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// 场景（矩阵行）。
    pub scene: Scene,
    /// 观测到的退出码（矩阵列）。
    pub code: u32,
    /// 实际出话的层。
    pub layer: Layer,
    /// 查表是否命中（未知 applet 必须是 false）。
    pub lookup_found: bool,
    /// 观测到的文案。
    pub text: &'static str,
    /// 登记了例外凭据。
    pub declared: bool,
    /// 结算前没有任何判决（未判档）。
    pub unjudged: bool,
    /// 统计值（进分子为 1）。
    pub value: u32,
    /// 兜底档被当成结论。
    pub default: bool,
}

impl Shape {
    /// 一条合法现场：场景该落的那一格，文案逐字，层与查表都对。
    pub fn lands(scene: Scene) -> Shape {
        Shape {
            scene,
            code: scene.expected_code(),
            layer: scene.layer(),
            lookup_found: false,
            text: scene.text(),
            declared: false,
            unjudged: false,
            value: 1,
            default: false,
        }
    }

    /// 换码。
    pub fn with_code(&self, code: u32) -> Shape {
        Shape { code, ..*self }
    }

    /// 换层。
    pub fn with_layer(&self, layer: Layer) -> Shape {
        Shape { layer, ..*self }
    }

    /// 换文案。
    pub fn with_text(&self, text: &'static str) -> Shape {
        Shape { text, ..*self }
    }

    /// 换例外凭据位。
    pub fn with_declared(&self, declared: bool) -> Shape {
        Shape { declared, ..*self }
    }

    /// 换查表位。
    pub fn with_lookup(&self, found: bool) -> Shape {
        Shape {
            lookup_found: found,
            ..*self
        }
    }

    /// 换未判档位。
    pub fn with_unjudged(&self, unjudged: bool) -> Shape {
        Shape { unjudged, ..*self }
    }

    /// 换统计值。
    pub fn with_value(&self, value: u32) -> Shape {
        Shape { value, ..*self }
    }

    /// 观测到的码带；表外码给 None。
    pub fn observed_band(&self) -> Option<CodeBand> {
        band_of(self.code)
    }

    /// 该现场应得的格位（矩阵视角）。
    pub fn cell(&self) -> CellVerdict {
        verdict_of(self.scene, self.code)
    }

    /// 现场指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.scene.seal());
        h = fold_word(h, self.code as u64);
        h = fold_byte(h, self.layer.id());
        h = fold_bool(h, self.lookup_found);
        h = fold_str(h, self.text);
        h = fold_bool(h, self.declared);
        h = fold_bool(h, self.unjudged);
        h = fold_word(h, self.value as u64);
        h = fold_bool(h, self.default);
        h
    }
}

/// `define` 的输出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 最后成立的闸。
    pub stage: Stage,
    /// 错误码（0 = 无）。
    pub code: u32,
    /// 统计值。
    pub value: u32,
    /// 注记。
    pub note: Note,
    /// 兜底档。
    pub default: bool,
    /// 本条是否可作为分子。
    pub ok: bool,
    /// 格位判决。
    pub verdict: CellVerdict,
}

impl Outcome {
    /// 一条正例结果。
    fn pass(stage: Stage, note: Note, value: u32, verdict: CellVerdict) -> Outcome {
        Outcome {
            stage,
            code: 0,
            value,
            note,
            default: false,
            ok: true,
            verdict,
        }
    }

    /// 一条失败结果。
    fn fail(stage: Stage, code: u32, note: Note, value: u32) -> Outcome {
        Outcome {
            stage,
            code,
            value,
            note,
            default: false,
            ok: false,
            verdict: CellVerdict::Contradicts,
        }
    }

    /// 是否成功。
    pub fn ok(&self) -> bool {
        self.ok
    }

    /// 是否失败。
    pub fn failed(&self) -> bool {
        !self.ok
    }

    /// 是否进入分子。
    pub fn counts(&self) -> bool {
        self.ok && self.verdict.counts()
    }

    /// 是否被闸门侧判死（根本不该进矩阵）。
    pub fn refused(&self) -> bool {
        self.code == EM_EARG || self.code == EM_ELAYER || self.code == EM_ELOOK
    }

    /// 结果指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.rank() as u64);
        h = fold_word(h, self.code as u64);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        h = fold_str(h, self.verdict.name());
        h = fold_bool(h, self.default);
        h
    }
}

/// 申报表：对一条现场声称的结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    /// 声称成功。
    pub success: bool,
    /// 声称到达的闸。
    pub stage: Stage,
    /// 声称的错误码。
    pub code: u32,
    /// 声称的统计值。
    pub value: u32,
    /// 声称的注记。
    pub note: Note,
    /// 声称的格位判决。
    pub verdict: CellVerdict,
    /// 声称的兜底位。
    pub default: bool,
    /// 反例槽的缺陷标签。
    pub tag: &'static str,
}

impl Claim {
    /// 正例申报。
    pub fn good(stage: Stage, note: Note, verdict: CellVerdict, tag: &'static str) -> Claim {
        Claim {
            success: true,
            stage,
            code: 0,
            value: 1,
            note,
            verdict,
            default: false,
            tag,
        }
    }

    /// 反例申报。
    pub fn bad(
        stage: Stage,
        code: u32,
        note: Note,
        value: u32,
        tag: &'static str,
    ) -> Claim {
        Claim {
            success: false,
            stage,
            code,
            value,
            note,
            verdict: CellVerdict::Contradicts,
            default: false,
            tag,
        }
    }

    /// 申报是否声称成功。
    pub fn success(&self) -> bool {
        self.success
    }

    /// 申报指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_bool(h, self.success);
        h = fold_word(h, self.stage.rank() as u64);
        h = fold_word(h, self.code as u64);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        h = fold_str(h, self.verdict.name());
        h = fold_bool(h, self.default);
        h = fold_str(h, self.tag);
        h
    }
}

/// 一条语料：现场 + 申报。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Case {
    /// 现场。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
}

impl Case {
    /// 真实结果。
    pub fn actual(&self) -> Outcome {
        define(&self.shape)
    }

    /// 缺陷标签。
    pub fn tag(&self) -> &'static str {
        self.claim.tag
    }

    /// 判定。
    pub fn fault(&self) -> Fault {
        judge(self)
    }

    /// 是否合格。
    pub fn ok(&self) -> bool {
        self.fault() == Fault::Ok
    }

    /// 语料指纹。
    pub fn seal(&self) -> u64 {
        fold_word(self.shape.seal(), self.claim.seal())
    }
}

// ---------------------------------------------------------------------------
// 八闸纵线
// ---------------------------------------------------------------------------

/// 主判据纵线：一条现场如何一步步走到格位。
///
/// 顺序不可调：Arg 先于 Layer（码在表外连格位都问不出来），Layer 先于 Text
/// （层错了文案对齐毫无意义），Lookup 先于 Code（不是未知 applet 就不进矩阵），
/// Band 先于 Code（先抓吞错再抓错码），Grid 在 Code 之后（格位是结论）。
pub fn define(s: &Shape) -> Outcome {
    // 1 Arg——码必须在五元表内。
    if !code_in_table(s.code) {
        return Outcome::fail(Stage::Arg, EM_EARG, Note::BadCode, 0);
    }
    // 2 Layer——出话层必须是这条现场该用的层。
    if s.layer != s.scene.layer() {
        return Outcome::fail(Stage::Layer, EM_ELAYER, Note::LayerWrong, 0);
    }
    // 3 Lookup——未知 applet 必须是 miss；命中说明这条现场不是本卡样本。
    if s.lookup_found {
        return Outcome::fail(Stage::Lookup, EM_ELOOK, Note::LookupHit, 0);
    }
    // 4 Text——前缀、名字、尾巴逐字。
    if !s.scene.text_ok(s.text) {
        return Outcome::fail(Stage::Text, EM_ETEXT, Note::TextWrong, 0);
    }
    // 5 Band——跨带（尤其吞错带）先判死。
    let observed = match band_of(s.code) {
        Some(b) => b,
        None => return Outcome::fail(Stage::Arg, EM_EARG, Note::BadCode, 0),
    };
    if observed != s.scene.band() {
        return Outcome::fail(Stage::Band, EM_EBAND, Note::CrossBand, 0);
    }
    // 6 Code——同带错码判死；唯一例外是 sh-cmd 的 127，且必须带凭据。
    if s.code != s.scene.expected_code() {
        if s.scene.excuses_127() && s.code == EXIT_NOTFOUND && s.declared {
            return Outcome::pass(
                Stage::Code,
                Note::RegisteredExcuse,
                1,
                CellVerdict::Excused,
            );
        }
        return Outcome::fail(Stage::Code, EM_ECODE, Note::SameBandWrong, 0);
    }
    // 利率闸之前的未判档：五闸都过了却没有判决落账——本卡绝不交付。
    if s.unjudged {
        return Outcome {
            stage: Stage::Rate,
            code: EM_EDEFAULT,
            value: 0,
            note: Note::Nil,
            default: true,
            ok: false,
            verdict: CellVerdict::Contradicts,
        };
    }
    // 7 Grid——格位必须与闸门同源：例外只许落在有凭据的那一格。
    if s.declared {
        return Outcome::fail(Stage::Grid, EM_EGRID, Note::PhantomExcuse, 0);
    }
    if s.value != 1 {
        return Outcome::fail(Stage::Grid, EM_EGRID, Note::GridContradiction, 0);
    }
    match verdict_of(s.scene, s.code) {
        CellVerdict::Lands => Outcome::pass(Stage::Grid, Note::Lands, 1, CellVerdict::Lands),
        CellVerdict::Excused => {
            // 走到这里说明例外没带凭据（上面已把带凭据的分支放掉）。
            Outcome::fail(Stage::Grid, EM_EGRID, Note::PhantomExcuse, 0)
        }
        CellVerdict::Contradicts => {
            Outcome::fail(Stage::Grid, EM_EGRID, Note::GridContradiction, 0)
        }
    }
}

/// 现场文案呈现（红项定位用）。
pub fn message(s: &Shape, o: &Outcome) -> String {
    match o.note {
        Note::Lands => msg_cell(s.scene.name(), s.code, CellVerdict::Lands.name()),
        Note::RegisteredExcuse => msg_cell(s.scene.name(), s.code, CellVerdict::Excused.name()),
        Note::CrossBand => {
            let b = match band_of(s.code) {
                Some(x) => x,
                None => return msg_usage(),
            };
            msg_cross_band(s.scene.name(), s.code, b.name())
        }
        Note::LayerWrong => msg_layer_wrong(s.scene.name(), s.layer.name()),
        Note::LookupHit => msg_lookup_hit("nosuch"),
        Note::TextWrong => msg_text_wrong(s.scene.name()),
        Note::SameBandWrong => msg_contradicts(s.scene.name(), s.code, s.scene.expected_code()),
        Note::PhantomExcuse => msg_phantom_excuse(s.scene.name(), s.code),
        Note::GridContradiction => {
            msg_contradicts(s.scene.name(), s.code, s.scene.expected_code())
        }
        Note::BadCode => msg_cell(s.scene.name(), s.code, "out-of-table"),
        Note::Nil => {
            let mut t = String::from("exit-grid: ");
            t.push_str(s.scene.name());
            t.push_str(" stopped at stage ");
            t.push_str(o.stage.name());
            t
        }
    }
}

/// 释义——把一条现场讲成人话。
pub fn detail(c: &Case) -> String {
    let o = c.actual();
    let mut s = String::from(c.tag());
    s.push_str(" scene=");
    s.push_str(c.shape.scene.name());
    s.push_str(" argv=");
    s.push_str(c.shape.scene.argv_source());
    s.push_str(" code=");
    s.push_str(dec_text(c.shape.code).as_str());
    s.push_str(" layer=");
    s.push_str(c.shape.layer.name());
    s.push_str(" stage=");
    s.push_str(o.stage.name());
    s.push_str(" err=");
    s.push_str(seg_text(o.code).as_str());
    s.push_str(" note=");
    s.push_str(o.note.name());
    s.push_str(" verdict=");
    s.push_str(o.verdict.name());
    s.push_str(" value=");
    s.push_str(dec_text(o.value).as_str());
    s
}

// ---------------------------------------------------------------------------
// 语料十四槽
// ---------------------------------------------------------------------------

/// 语料基型：分发层 `busybox nosuch`，落 127。
const BASE: Shape = Shape {
    scene: Scene::Dispatch,
    code: EXIT_NOTFOUND,
    layer: Layer::Busybox,
    lookup_found: false,
    text: T_BUSYBOX_LINE,
    declared: false,
    unjudged: false,
    value: 1,
    default: false,
};

/// 十四槽现场：前八槽覆盖五条 `Lands` 格与唯一 `Excused` 格（含同格重复抽样），
/// 后六槽各坏一维。
pub const SHAPES: [Shape; CORPUS] = [
    // 0 dispatch × 127：查表未命中的缺省格。
    Shape { ..BASE },
    // 1 alias × 127：argv[0] 当 applet 名，行不同、格相同。
    Shape {
        scene: Scene::Alias,
        text: T_BUSYBOX_LINE,
        ..BASE
    },
    // 2 builtin × 127：内建表与 PATH 双未命中，shell 出话。
    Shape {
        scene: Scene::Builtin,
        layer: Layer::Shell,
        text: T_BUILTIN_LINE,
        ..BASE
    },
    // 3 sh-cmd × 126：PATH 命中但不可执行。
    Shape {
        scene: Scene::ShCmd,
        code: EXIT_NOEXEC,
        layer: Layer::Shell,
        text: T_SHCMD_LINE,
        ..BASE
    },
    // 4 exec × 1：exec 层只回错误号，一个字都不打。
    Shape {
        scene: Scene::Exec,
        code: EXIT_FAIL,
        layer: Layer::Caller,
        text: T_SILENT,
        ..BASE
    },
    // 5 dispatch × 127 的第二种形态：argv 尾巴带参数，格位不变。
    Shape { ..BASE },
    // 6 sh-cmd × 127：目录档方言例外，带凭据。
    Shape {
        scene: Scene::ShCmd,
        code: EXIT_NOTFOUND,
        layer: Layer::Shell,
        text: T_SHCMD_DIR_LINE,
        declared: true,
        ..BASE
    },
    // 7 exec × 1 的第二种形态：execve 直接给 -1。
    Shape {
        scene: Scene::Exec,
        code: EXIT_FAIL,
        layer: Layer::Caller,
        text: T_SILENT,
        ..BASE
    },
    // 8 Estate：吞错带——退 0 而 applet 没跑，申报却声称落格。
    Shape {
        code: EXIT_ZERO,
        value: 0,
        ..BASE
    },
    // 9 Stage：层错——分发层现场却由 shell 出话。
    Shape {
        layer: Layer::Shell,
        value: 0,
        ..BASE
    },
    // 10 Code：同带错码——exec 落 2（用法错），仍在 Tool 带里。
    Shape {
        scene: Scene::Exec,
        code: EXIT_USAGE,
        layer: Layer::Caller,
        text: T_SILENT,
        value: 0,
        ..BASE
    },
    // 11 Value：查表竟然命中——这条现场根本不是未知 applet。
    Shape {
        scene: Scene::Builtin,
        layer: Layer::Shell,
        text: T_BUILTIN_LINE,
        lookup_found: true,
        value: 0,
        ..BASE
    },
    // 12 Note：文案漏掉 `1:` 行号占位，体例坏。
    Shape {
        scene: Scene::ShCmd,
        code: EXIT_NOEXEC,
        layer: Layer::Shell,
        text: T_SHCMD_NOLINE,
        value: 0,
        ..BASE
    },
    // 13 Default：五闸都过却没有任何判决落账（未判档）。
    Shape {
        scene: Scene::Alias,
        unjudged: true,
        value: 0,
        ..BASE
    },
];

/// 申报基型：与落格现场的实际结论逐格相同。
const GOOD_CLAIM: Claim = Claim {
    success: true,
    stage: Stage::Grid,
    code: 0,
    value: 1,
    note: Note::Lands,
    verdict: CellVerdict::Lands,
    default: false,
    tag: "",
};

/// 例外格的申报基型。
const EXCUSED_CLAIM: Claim = Claim {
    stage: Stage::Code,
    note: Note::RegisteredExcuse,
    verdict: CellVerdict::Excused,
    ..GOOD_CLAIM
};

/// 十四格申报：前八格与 `define(SHAPES[i])` 逐格相同，后六格各坏一维。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim {
        tag: "f11679-dispatch-127",
        ..GOOD_CLAIM
    },
    Claim {
        tag: "f11679-alias-127",
        ..GOOD_CLAIM
    },
    Claim {
        tag: "f11679-builtin-127",
        ..GOOD_CLAIM
    },
    Claim {
        tag: "f11679-shcmd-126",
        ..GOOD_CLAIM
    },
    Claim {
        tag: "f11679-exec-1",
        ..GOOD_CLAIM
    },
    Claim {
        tag: "f11679-dispatch-argvtail",
        ..GOOD_CLAIM
    },
    Claim {
        tag: "f11679-shcmd-excused-127",
        ..EXCUSED_CLAIM
    },
    Claim {
        tag: "f11679-exec-execve",
        ..GOOD_CLAIM
    },
    // 8 成败互报：其余五维照实抄，只把"矛盾"这一行申报成落格。
    Claim {
        success: true,
        stage: Stage::Grid,
        code: 0,
        value: 1,
        note: Note::Lands,
        verdict: CellVerdict::Lands,
        default: false,
        tag: "f11679-estate-swallow",
    },
    // 9 阶段不符：层错现场申报成"已到码"。
    Claim {
        success: false,
        stage: Stage::Code,
        code: EM_ELAYER,
        value: 0,
        note: Note::LayerWrong,
        verdict: CellVerdict::Contradicts,
        default: false,
        tag: "f11679-stage-layer",
    },
    // 10 码档不符：同带错码申报成跨带码。
    Claim {
        success: false,
        stage: Stage::Code,
        code: EM_EBAND,
        value: 0,
        note: Note::SameBandWrong,
        verdict: CellVerdict::Contradicts,
        default: false,
        tag: "f11679-code-sameband",
    },
    // 11 数值不符：查表命中却按"进分子"计。
    Claim {
        success: false,
        stage: Stage::Lookup,
        code: EM_ELOOK,
        value: 1,
        note: Note::LookupHit,
        verdict: CellVerdict::Contradicts,
        default: false,
        tag: "f11679-value-lookup",
    },
    // 12 注记不符：体例坏申报成层错。
    Claim {
        success: false,
        stage: Stage::Text,
        code: EM_ETEXT,
        value: 0,
        note: Note::LayerWrong,
        verdict: CellVerdict::Contradicts,
        default: false,
        tag: "f11679-note-text",
    },
    // 13 兜底档：未判现场申报成"没有兜底"。
    Claim {
        success: false,
        stage: Stage::Rate,
        code: EM_EDEFAULT,
        value: 0,
        note: Note::Nil,
        verdict: CellVerdict::Contradicts,
        default: false,
        tag: "f11679-default-unjudged",
    },
];

// ---------------------------------------------------------------------------
// 期望表（申报表的第二种形式）
// ---------------------------------------------------------------------------

/// 期望阶段（现场**真实**落到的阶段，不是申报那一栏）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Grid,
    Stage::Grid,
    Stage::Grid,
    Stage::Grid,
    Stage::Grid,
    Stage::Grid,
    Stage::Code,
    Stage::Grid,
    Stage::Band,
    Stage::Layer,
    Stage::Code,
    Stage::Lookup,
    Stage::Text,
    Stage::Rate,
];

/// 期望错误码（0 = 无码；现场真实挂的码）。
pub const WANT_CODE: [u32; CORPUS] = [
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    EM_EBAND,
    EM_ELAYER,
    EM_ECODE,
    EM_ELOOK,
    EM_ETEXT,
    EM_EDEFAULT,
];

/// 期望缺陷维序位（0..5 六维，6 = 无缺陷）。
pub const WANT_NDX: [usize; CORPUS] = [6, 6, 6, 6, 6, 6, 6, 6, 0, 1, 2, 3, 4, 5];

/// 期望注记（现场真实注记）。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::Lands,
    Note::Lands,
    Note::Lands,
    Note::Lands,
    Note::Lands,
    Note::Lands,
    Note::RegisteredExcuse,
    Note::Lands,
    Note::CrossBand,
    Note::LayerWrong,
    Note::SameBandWrong,
    Note::LookupHit,
    Note::TextWrong,
    Note::Nil,
];

/// 期望格位判决。
pub const WANT_VERDICT: [CellVerdict; CORPUS] = [
    CellVerdict::Lands,
    CellVerdict::Lands,
    CellVerdict::Lands,
    CellVerdict::Lands,
    CellVerdict::Lands,
    CellVerdict::Lands,
    CellVerdict::Excused,
    CellVerdict::Lands,
    CellVerdict::Contradicts,
    CellVerdict::Contradicts,
    CellVerdict::Contradicts,
    CellVerdict::Contradicts,
    CellVerdict::Contradicts,
    CellVerdict::Contradicts,
];

/// 期望兜底位。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, false, false, false, false, false, false, false, false, false, true,
];

/// 期望短名（标签必须挂本卡号）。
pub const WANT_TAGS: [&str; CORPUS] = [
    "f11679-dispatch-127",
    "f11679-alias-127",
    "f11679-builtin-127",
    "f11679-shcmd-126",
    "f11679-exec-1",
    "f11679-dispatch-argvtail",
    "f11679-shcmd-excused-127",
    "f11679-exec-execve",
    "f11679-estate-swallow",
    "f11679-stage-layer",
    "f11679-code-sameband",
    "f11679-value-lookup",
    "f11679-note-text",
    "f11679-default-unjudged",
];

/// 期望统计值（现场真实统计值）。
pub const WANT_VALUE: [u32; CORPUS] = [1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0];

/// 期望成败（现场真实成败）。
pub const WANT_SUCCESS: [bool; CORPUS] = [
    true, true, true, true, true, true, true, true, false, false, false, false, false, false,
];

// ---------------------------------------------------------------------------
// 判定
// ---------------------------------------------------------------------------

/// 缺陷维 → 序位。
pub fn fault_ndx(f: Fault) -> usize {
    match f {
        Fault::Estate => 0,
        Fault::Stage => 1,
        Fault::Code => 2,
        Fault::Value => 3,
        Fault::Note => 4,
        Fault::Default => 5,
        Fault::Ok => 6,
    }
}

/// 序位 → 缺陷维。
pub fn ndx_fault(n: usize) -> Option<Fault> {
    match n {
        0 => Some(Fault::Estate),
        1 => Some(Fault::Stage),
        2 => Some(Fault::Code),
        3 => Some(Fault::Value),
        4 => Some(Fault::Note),
        5 => Some(Fault::Default),
        6 => Some(Fault::Ok),
        _ => None,
    }
}

/// 申报对判——固定次序 Estate → Stage → Code → Value → Note → Default → Ok。
///
/// 成败互报优先：把"矛盾格"申报成"落格"，无论后面几维对不对，都是最严重的那类
/// 谎——矩阵的价值就是"这条场景真会落这个码"。
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
    if a.verdict != k.verdict {
        return Fault::Note;
    }
    // 兜底档双向判死：真落了未判档不许当结论；没落却申报"落了"同样不许。
    if a.default || a.default != k.default {
        return Fault::Default;
    }
    Fault::Ok
}

/// 语料取槽。
pub fn case_at(i: usize) -> Option<Case> {
    match (SHAPES.get(i), CLAIMS.get(i)) {
        (Some(s), Some(c)) => Some(Case {
            shape: *s,
            claim: *c,
        }),
        _ => None,
    }
}

/// 全语料。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case {
        shape: BASE,
        claim: GOOD_CLAIM,
    }; CORPUS];
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

/// 覆盖校验：槽号必须落在语料范围内。
pub fn cover(n: usize) -> Result<(), u32> {
    if n > CORPUS {
        return Err(EM_ESLOT);
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

/// 第 `i` 槽（反例区）期望的缺陷维；越界按无缺陷（由调用方先做范围校验）。
fn expected_reject_ndx(i: usize) -> Fault {
    match expected(i) {
        Some(f) => f,
        None => Fault::Ok,
    }
}

// ---------------------------------------------------------------------------
// 档位与矩阵
// ---------------------------------------------------------------------------

/// 格位四档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 落到唯一合法格。
    Lands,
    /// 落到登记例外格。
    Excused,
    /// 闸门侧判死，现场作废（不进分母）。
    Void,
    /// 现场落到矛盾格或未判档。
    Unrun,
}

impl Tier {
    /// 四档全表。
    pub fn all() -> [Tier; TIER_COUNT] {
        [Tier::Lands, Tier::Excused, Tier::Void, Tier::Unrun]
    }

    /// 档名。
    pub fn name(&self) -> &'static str {
        match *self {
            Tier::Lands => "lands",
            Tier::Excused => "excused",
            Tier::Void => "void",
            Tier::Unrun => "unrun",
        }
    }

    /// 该档是否是绿档（只有前两档算绿）。
    pub fn is_green(&self) -> bool {
        match *self {
            Tier::Lands | Tier::Excused => true,
            _ => false,
        }
    }

    /// 由一次结果归档。
    pub fn of(o: &Outcome) -> Tier {
        if o.refused() {
            return Tier::Void;
        }
        if o.default || o.code == EM_EDEFAULT {
            return Tier::Unrun;
        }
        if o.ok {
            if o.verdict == CellVerdict::Excused {
                return Tier::Excused;
            }
            return Tier::Lands;
        }
        Tier::Unrun
    }
}

/// 一档的统计行。
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
        if self.total > self.good {
            self.total - self.good
        } else {
            0
        }
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
///
/// 判对的条件是"落档与应落档一致"：正例要落进它自己那格绿档，反例**绝不许**
/// 落进绿档——一条矛盾格如果被归档成 `Lands`，矩阵就把它当成合法格交付了。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Matrix {
    rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 由语料构造。
    pub fn build() -> Matrix {
        let table = Tier::all();
        let mut rows = [Row {
            tier: Tier::Lands,
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
                    landed == tier && !tier.is_green() && c.fault() == expected_reject_ndx(i)
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
// 语料侧的档位落点（第二形式：逐槽数出来的固定值）
// ---------------------------------------------------------------------------

/// 语料里落 `Void` 的槽数（层错与查表命中两条）。
pub const VOID_SAMPLES: usize = 2;
/// 语料里落 `Unrun` 的槽数（吞错/同带错码/体例/未判四条）。
pub const UNRUN_SAMPLES: usize = 4;
/// 语料里落 `Lands` 的槽数。
pub const LANDS_SAMPLES: usize = 7;
/// 语料里落 `Excused` 的槽数。
pub const EXCUSED_SAMPLES: usize = 1;

// ---------------------------------------------------------------------------
// 结算报告
// ---------------------------------------------------------------------------

/// 全绿渲染文本（逐字钉住，改一个字都要红）。
pub const RENDER_OK: &str =
    "exit-grid scenes=5 codes=5 cells=25 lands=5 excused=1 contradicts=19 rate=1000permille\n";

/// 矩阵结算报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Report {
    /// 结算分母：格总数。
    pub total: usize,
    /// 正例槽数。
    pub accept: usize,
    /// 反例槽数。
    pub reject: usize,
    /// `Lands` 格数。
    pub lands: usize,
    /// `Excused` 格数。
    pub excused: usize,
    /// `Contradicts` 格数。
    pub contradicts: usize,
    /// 语料落 `Void` 的槽数。
    pub void: usize,
    /// 语料落 `Unrun` 的槽数。
    pub unrun: usize,
    /// 计入一致的格数。
    pub matched: usize,
    /// 违约格数。
    pub conflicts: usize,
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
        self.conflicts == 0
            && self.rate() == RATE_FULL
            && self.accept == ACCEPT
            && self.reject == REJECT
            && self.lands == LANDS_CELLS as usize
            && self.excused == EXCUSED_CELLS as usize
            && self.contradicts == CONTRADICTS_CELLS as usize
            && self.lands + self.excused + self.contradicts == self.total
            && self.void == VOID_SAMPLES
            && self.unrun == UNRUN_SAMPLES
    }

    /// 红项计数。
    pub fn reds(&self) -> u32 {
        (self.conflicts as u32) + (self.unrun as u32).saturating_sub(UNRUN_SAMPLES as u32)
    }

    /// 闸门：全绿才放行，否则给矩阵码。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(EM_EGRID);
        }
        if self.lands + self.excused + self.contradicts != self.total {
            return Err(EM_EGRID);
        }
        if self.conflicts > 0 {
            return Err(EM_ECODE);
        }
        if self.rate() < RATE_FULL {
            return Err(EM_EVALUE);
        }
        if !self.green() {
            return Err(EM_EDEFAULT);
        }
        Ok(())
    }

    /// 三档格数。
    pub fn decided_cells(&self) -> usize {
        self.lands + self.excused + self.contradicts
    }

    /// 逐字渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("exit-grid scenes=");
        s.push_str(dec_text(SCENE_COUNT as u32).as_str());
        s.push_str(" codes=");
        s.push_str(dec_text(CODE_COUNT as u32).as_str());
        s.push_str(" cells=");
        s.push_str(dec_text(self.total as u32).as_str());
        s.push_str(" lands=");
        s.push_str(dec_text(self.lands as u32).as_str());
        s.push_str(" excused=");
        s.push_str(dec_text(self.excused as u32).as_str());
        s.push_str(" contradicts=");
        s.push_str(dec_text(self.contradicts as u32).as_str());
        s.push_str(" rate=");
        s.push_str(dec_text(self.rate()).as_str());
        s.push_str("permille\n");
        s
    }
}

/// 空报告（未跑任何现场时的形态，绝不与全绿混淆）。
pub fn empty_report() -> Report {
    Report {
        total: 0,
        accept: 0,
        reject: 0,
        lands: 0,
        excused: 0,
        contradicts: 0,
        void: 0,
        unrun: 0,
        matched: 0,
        conflicts: 0,
    }
}

/// 由矩阵与语料两路结算。
///
/// 分子只由矩阵格给出（25 格必须全部判死或判活），语料面给出违约项：反例槽按
/// 申报的那一维落红不算违约，正例槽落红、反例槽落错维、或注入出的错报才算。
fn settle(over: Option<(usize, Fault)>) -> Report {
    let grid = ExitGrid::build();
    let mut rep = Report {
        total: GRID_CELLS,
        accept: ACCEPT,
        reject: REJECT,
        lands: grid.lands() as usize,
        excused: grid.excused() as usize,
        contradicts: grid.contradicts() as usize,
        void: 0,
        unrun: 0,
        matched: 0,
        conflicts: 0,
    };
    let mut violations = 0usize;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return empty_report(),
        };
        match Tier::of(&c.actual()) {
            Tier::Lands => {}
            Tier::Excused => {}
            Tier::Void => rep.void += 1,
            Tier::Unrun => rep.unrun += 1,
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
        if i < ACCEPT {
            if landed != Fault::Ok {
                violations += 1;
            }
        } else if landed != expected_reject_ndx(i) {
            violations += 1;
        }
        i += 1;
    }
    rep.conflicts = violations;
    rep.matched = rep.total.saturating_sub(violations);
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

/// 带注入的结算：越界槽给槽位码，否则返回注入后的报告。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((slot, _)) = over {
        if slot >= CORPUS {
            return Err(EM_ESLOT);
        }
    }
    Ok(settle(over))
}

/// 报告与矩阵是否互相印证（两条独立路径必须给同一批格数）。
pub fn report_agrees_with_matrix() -> bool {
    let rep = settle(None);
    let m = Matrix::build();
    let grid = ExitGrid::build();
    rep.lands == grid.lands() as usize
        && rep.excused == grid.excused() as usize
        && rep.contradicts == grid.contradicts() as usize
        && rep.decided_cells() == rep.total
        && m.goods() == CORPUS
}

/// 一致率在注入一路红之后必须掉下来（929 档的口径标定）。
pub fn red_rate_calibrated() -> bool {
    let base = settle(None);
    let bad = match run_with(Some((2, Fault::Value))) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if bad.matched >= base.matched {
        return false;
    }
    if bad.rate() >= base.rate() {
        return false;
    }
    RED_RATE == 929 && RATE_FULL == 1000
}

/// 注入一路红，四路自检必须各指各的。
pub fn inject_four() -> [bool; 4] {
    let grid_route = {
        let g = ExitGrid::build();
        g.decided()
            && g.every_cell_judged()
            && g.lands() == LANDS_CELLS
            && g.excused() == EXCUSED_CELLS
            && g.contradicts() == CONTRADICTS_CELLS
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
        all_ok && grid_agrees_with_gate()
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
    [grid_route, judge_route, render_route, ledger_route]
}

/// 矩阵与闸门同源校验：逐格造现场，格说落则闸必须放，格说矛盾则闸必须拦。
///
/// 例外格是唯一需要凭据的格：不带凭据的 127 在 sh-cmd 上必须被判死，
/// 带凭据才放行——这正是"例外不是挡箭牌，是台账"。
pub fn grid_agrees_with_gate() -> bool {
    let scenes = Scene::all();
    let mut r = 0usize;
    while r < SCENE_COUNT {
        let scene = match scenes.get(r) {
            Some(x) => *x,
            None => return false,
        };
        let mut c = 0usize;
        while c < CODE_COUNT {
            let code = match CODE_TABLE.get(c) {
                Some(x) => *x,
                None => return false,
            };
            let cell = verdict_of(scene, code);
            match cell {
                CellVerdict::Lands => {
                    let s = Shape::lands(scene);
                    let o = define(&s);
                    if !o.ok() || o.verdict != CellVerdict::Lands {
                        return false;
                    }
                }
                CellVerdict::Excused => {
                    let mut s = Shape::lands(scene).with_code(code).with_declared(true);
                    if scene == Scene::ShCmd {
                        s = s.with_text(T_SHCMD_DIR_LINE);
                    }
                    let o = define(&s);
                    if !o.ok() || o.verdict != CellVerdict::Excused {
                        return false;
                    }
                    // 同一格去掉凭据必须判死——凭据是这一格唯一的进门条。
                    let bare = Shape::lands(scene).with_code(code);
                    if define(&bare).ok() {
                        return false;
                    }
                }
                CellVerdict::Contradicts => {
                    let s = Shape::lands(scene).with_code(code);
                    let o = define(&s);
                    if o.ok() || o.verdict != CellVerdict::Contradicts {
                        return false;
                    }
                }
            }
            c += 1;
        }
        r += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 阶段闸门：Arg→Layer→Lookup→Text→Band→Code→Grid→Rate
// ---------------------------------------------------------------------------

/// 一次结算的走闸状态机。
///
/// 这条纵线为什么不能倒：码入表必须先于层归属，层归属必须先于查表，查表必须先于
/// 文案，文案必须先于分带，分带必须先于精确码，精确码必须先于格位。一旦允许倒
/// 退，就能出现"先出格位再补码"——矛盾格靠回填混进矩阵，本卡的判据当场失效。
/// 所以倒退判死（`EM_ESTAGE`），关闭后再走一步也判死（`EM_ESTATE`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gate {
    /// 当前档；`None` 表示还没进过任何闸。
    current: Option<Stage>,
    /// 是否已封账。
    closed: bool,
    /// 成功走档次数。
    steps: u32,
    /// 被拒次数（倒退或封账后再走）。
    refusals: u32,
    /// 走档轨迹指纹。
    root: u64,
}

impl Gate {
    /// 空闸门：未进档、未封账。
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
            return Err(EM_ESTATE);
        }
        if let Some(cur) = self.current {
            if s.rank() < cur.rank() {
                self.refusals += 1;
                return Err(EM_ESTAGE);
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
            return Err(EM_ESTATE);
        }
        if self.current.is_none() {
            return Err(EM_ESTAGE);
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

    /// 依次走一串档，任一步被拒就把那个码交出来。
    pub fn walk(stages: &[Stage]) -> Result<Gate, u32> {
        let mut g = Gate::new();
        let mut i = 0usize;
        while i < stages.len() {
            let s = match stages.get(i) {
                Some(x) => *x,
                None => return Err(EM_ESLOT),
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
                None => return Err(EM_ESLOT),
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
    /// 这一格对应的算子号（统计格用 `STAT_OPS`，缺陷格用 `DEFECT_OP_BASE + n`）。
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
        h = fold_word(h, self.value as u64);
        h
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
/// 登记纪律：槽号必须严格等于当前长度，跳号与重号一律拒收并计数——
/// 本卡不许"先出报告再补台账"，所以这里的 `rejects` 是一个要判死的量。
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
            return Err(EM_ESTATE);
        }
        if self.len >= SAMPLES {
            self.rejects += 1;
            return Err(EM_ESLOT);
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(EM_ESLOT);
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
        self.len - self.ok_count()
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

    /// 逐格复核：统计格对上 `VALS`、红档格对上 `RED_RATE`、缺陷格对上 `REJECT_CODES`
    /// 且败格值必须为 0，缺陷码两两互异。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(EM_ESLOT);
        }
        let stat_last = match STAT_CELLS.checked_add(1) {
            Some(x) => x,
            None => return Err(EM_ESLOT),
        };
        let mut i = 0usize;
        while i < stat_last {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(EM_ESLOT),
            };
            if !s.ok {
                return Err(EM_ESTATE);
            }
            if s.code != 0 {
                return Err(EM_ECODE);
            }
            let want_value = match VALS.get(i) {
                Some(v) => *v,
                None => return Err(EM_ESLOT),
            };
            if s.value != want_value {
                return Err(EM_EVALUE);
            }
            let want_op = match self.stat_op_want(i) {
                Some(v) => v,
                None => return Err(EM_ESLOT),
            };
            if s.op != want_op {
                return Err(EM_ECODE);
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < REJECT {
            let cell = match STAT_CELLS.checked_add(1).and_then(|x| x.checked_add(j)) {
                Some(x) => x,
                None => return Err(EM_ESLOT),
            };
            let s = match self.sample(cell) {
                Some(x) => x,
                None => return Err(EM_ESLOT),
            };
            if s.ok {
                return Err(EM_ESTATE);
            }
            if s.value != 0 {
                return Err(EM_EVALUE);
            }
            let want_code = match REJECT_CODES.get(j) {
                Some(c) => *c,
                None => return Err(EM_ESLOT),
            };
            if s.code != want_code {
                return Err(EM_ECODE);
            }
            let want_op = match DEFECT_OP_BASE.checked_add(j as u8) {
                Some(x) => x,
                None => return Err(EM_ESLOT),
            };
            if s.op != want_op {
                return Err(EM_ECODE);
            }
            let mut k = j + 1usize;
            while k < REJECT {
                let other = match REJECT_CODES.get(k) {
                    Some(c) => *c,
                    None => return Err(EM_ESLOT),
                };
                if other == want_code {
                    return Err(EM_ECODE);
                }
                k += 1;
            }
            j += 1;
        }
        Ok(())
    }

    /// 第一处不合规的格子与原因，给排障用（不改判定，只给指认）。
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
            return Err(EM_ESTATE);
        }
        match self.audit() {
            Ok(()) => {}
            Err(code) => return Err(code),
        }
        let h = self.seal();
        if h == 0 {
            return Err(EM_EVALUE);
        }
        self.root = h;
        self.frozen = true;
        Ok(h)
    }

    /// 重放：以同一批格子重建账本，比指纹。任何一格被改过都不许相等。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(EM_ESTATE);
        }
        let mut fresh = Ledger::new();
        let mut i = 0usize;
        while i < self.len {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(EM_EVALUE),
            };
            match fresh.register(s) {
                Ok(()) => {}
                Err(_) => return Err(EM_EVALUE),
            }
            i += 1;
        }
        let h = fresh.seal();
        if h != self.root {
            return Err(EM_EVALUE);
        }
        Ok(h)
    }

    /// 改动一格（仅未冻结时允许），用于注入红。
    pub fn poke(&mut self, i: usize, value: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(EM_ESTATE);
        }
        match self.slots.get_mut(i) {
            Some(cell) => {
                match *cell {
                    Some(ref mut s) => {
                        s.value = value;
                    }
                    None => return Err(EM_ESLOT),
                }
                Ok(())
            }
            None => Err(EM_ESLOT),
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

/// 注入一维错的台账（用于证明复核真的在逐格看，而不是只看格数）。
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
        _ => return l,
    };
    match l.poke(cell, value) {
        Ok(()) => {}
        Err(_) => {}
    }
    l
}

/// 申报表第一格的标签：跨槽复用时的缺省 tag，取值一律走 `.get()`。
pub fn head_tag() -> &'static str {
    match WANT_TAGS.get(0) {
        Some(x) => *x,
        None => "f11679-dispatch-127",
    }
}

/// 统计表第一格的值：台账注入用的缺省值。
pub fn head_val() -> u32 {
    match VALS.get(0) {
        Some(x) => *x,
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// 十三条判据谓词（全部私有，只在 run_unx11679_checks 里汇总）
// ---------------------------------------------------------------------------

/// 01 名称与段基址：常量层自洽，任何一条规模关系破了都算红。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d2e0 {
        return false;
    }
    if ANCHOR_F11678_SEG + 0x10 != SEG_BASE {
        return false;
    }
    if ANCHOR_F11680_SEG != SEG_BASE + 0x10 {
        return false;
    }
    if ANCHOR_F11675_GAP != 0x50 {
        return false;
    }
    if SEG_BASE - ANCHOR_F11675_SEG != ANCHOR_F11675_GAP {
        return false;
    }
    if CORPUS != 14 || SAMPLES != CORPUS {
        return false;
    }
    if ACCEPT != 8 || REJECT != 6 || ACCEPT + REJECT != CORPUS {
        return false;
    }
    if TIER_COUNT != 4 {
        return false;
    }
    if STAGE_COUNT != 8 || STAGE_LAST_RANK != 8 {
        return false;
    }
    if SCENE_COUNT != 5 || CODE_COUNT != 5 {
        return false;
    }
    if GRID_CELLS != SCENE_COUNT * CODE_COUNT || GRID_CELLS != 25 {
        return false;
    }
    if LANDS_CELLS + EXCUSED_CELLS + CONTRADICTS_CELLS != GRID_CELLS as u32 {
        return false;
    }
    if CODE_TABLE.len() != CODE_COUNT {
        return false;
    }
    if STAT_CELLS != 7 || STAT_OPS.len() != STAT_CELLS {
        return false;
    }
    if VALS.len() != ACCEPT {
        return false;
    }
    if VALS.get(6) != Some(&RATE_FULL) {
        return false;
    }
    if VALS.get(7) != Some(&RED_RATE) {
        return false;
    }
    if RED_RATE >= RATE_FULL {
        return false;
    }
    if head_tag() != "f11679-dispatch-127" {
        return false;
    }
    if head_val() != SCENE_COUNT as u32 {
        return false;
    }
    true
}

/// 02 十三码互异：全在段内、两两不同、六维反例码与缺陷维对齐。
fn codes_ok() -> bool {
    if ALL_CODES.len() != 13 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_CODES.len() {
        let code = match ALL_CODES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if code <= SEG_BASE {
            return false;
        }
        if code > SEG_BASE + 0x0d {
            return false;
        }
        let mut j = i + 1usize;
        while j < ALL_CODES.len() {
            if let Some(other) = ALL_CODES.get(j) {
                if *other == code {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let faults = Fault::reject_faults();
    if faults.len() != REJECT || REJECT_CODES.len() != REJECT {
        return false;
    }
    let mut k = 0usize;
    while k < REJECT {
        let f = match faults.get(k) {
            Some(x) => *x,
            None => return false,
        };
        let c = match REJECT_CODES.get(k) {
            Some(x) => *x,
            None => return false,
        };
        if f.code() != c {
            return false;
        }
        if REJECT_FAULTS.get(k) != Some(&f) {
            return false;
        }
        if f == Fault::Ok {
            return false;
        }
        k += 1;
    }
    let mut m = 0usize;
    while m < STAT_OPS.len() {
        let op = match STAT_OPS.get(m) {
            Some(x) => *x,
            None => return false,
        };
        if op == 0 {
            return false;
        }
        let mut n = m + 1usize;
        while n < STAT_OPS.len() {
            if STAT_OPS.get(n) == Some(&op) {
                return false;
            }
            n += 1;
        }
        m += 1;
    }
    if OPID_STAT_RED != 0x08 {
        return false;
    }
    if DEFECT_OP_BASE != 0x21 {
        return false;
    }
    if stat_last_overlaps_red() {
        return false;
    }
    true
}

/// 统计段末格（第 7 格）与红档格号不许撞。
fn stat_last_overlaps_red() -> bool {
    let last = match STAT_OPS.get(STAT_CELLS - 1) {
        Some(x) => *x,
        None => return true,
    };
    last == OPID_STAT_RED
}

/// 03 五场景全表：行名、层、带、应落码、argv 来源、文案全部对齐。
fn scenes_ok() -> bool {
    let table = Scene::all();
    if table.len() != SCENE_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < SCENE_COUNT {
        let scene = match table.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if scene.id() != i {
            return false;
        }
        if Scene::of_id(i) != Some(scene) {
            return false;
        }
        if scene.name().len() == 0 || scene.argv_source().len() == 0 {
            return false;
        }
        if !scene.layer().speaks() && scene.layer() != Layer::Caller {
            return false;
        }
        if !code_in_table(scene.expected_code()) {
            return false;
        }
        let obs = match band_of(scene.expected_code()) {
            Some(b) => b,
            None => return false,
        };
        if obs != scene.band() {
            return false;
        }
        if !scene.band().legal() {
            return false;
        }
        if !scene.text_ok(scene.text()) {
            return false;
        }
        if scene.text_ok("") && !(scene == Scene::Exec) {
            return false;
        }
        let mut j = i + 1usize;
        while j < SCENE_COUNT {
            if let Some(other) = table.get(j) {
                if other.seal() == scene.seal() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    if Scene::of_id(SCENE_COUNT).is_some() {
        return false;
    }
    // 层归属与带归属的第二形式。
    if Scene::Dispatch.layer() != Layer::Busybox {
        return false;
    }
    if Scene::Alias.layer() != Layer::Busybox {
        return false;
    }
    if Scene::Builtin.layer() != Layer::Shell {
        return false;
    }
    if Scene::ShCmd.layer() != Layer::Shell {
        return false;
    }
    if Scene::Exec.layer() != Layer::Caller {
        return false;
    }
    if Scene::Dispatch.expected_code() != EXIT_NOTFOUND {
        return false;
    }
    if Scene::Alias.expected_code() != EXIT_NOTFOUND {
        return false;
    }
    if Scene::Builtin.expected_code() != EXIT_NOTFOUND {
        return false;
    }
    if Scene::ShCmd.expected_code() != EXIT_NOEXEC {
        return false;
    }
    if Scene::Exec.expected_code() != EXIT_FAIL {
        return false;
    }
    if !Scene::ShCmd.excuses_127() {
        return false;
    }
    if Scene::Dispatch.excuses_127() || Scene::Alias.excuses_127() {
        return false;
    }
    if Scene::Builtin.excuses_127() || Scene::Exec.excuses_127() {
        return false;
    }
    // 文案两路互校：常量侧与拼装侧必须逐字相同。
    if msg_applet_not_found("nosuch").as_str() != T_BUSYBOX_LINE {
        return false;
    }
    if msg_shell_not_found("nosuch").as_str() != T_BUILTIN_LINE {
        return false;
    }
    if msg_shell_noexec("/tmp/nosuch").as_str() != T_SHCMD_LINE {
        return false;
    }
    if msg_shell_isdir("/tmp/nosuch").as_str() != T_SHCMD_DIR_LINE {
        return false;
    }
    if T_SHCMD_NOLINE.len() >= T_SHCMD_LINE.len() {
        return false;
    }
    true
}

/// 04 二十五格零悬空：三档格数、每行唯一格、例外独占一行。
fn grid_ok() -> bool {
    let g = ExitGrid::build();
    if !g.decided() {
        return false;
    }
    if !g.every_cell_judged() {
        return false;
    }
    if g.lands() != LANDS_CELLS {
        return false;
    }
    if g.excused() != EXCUSED_CELLS {
        return false;
    }
    if g.contradicts() != CONTRADICTS_CELLS {
        return false;
    }
    let scenes = Scene::all();
    let mut r = 0usize;
    let mut lands_total = 0u32;
    while r < SCENE_COUNT {
        let scene = match scenes.get(r) {
            Some(x) => *x,
            None => return false,
        };
        let mut lands = 0u32;
        let mut excused = 0u32;
        let mut c = 0usize;
        while c < CODE_COUNT {
            let code = match CODE_TABLE.get(c) {
                Some(x) => *x,
                None => return false,
            };
            let v = match g.cell(scene, code) {
                Some(x) => x,
                None => return false,
            };
            if v != verdict_of(scene, code) {
                return false;
            }
            if v == CellVerdict::Lands {
                lands += 1;
            }
            if v == CellVerdict::Excused {
                excused += 1;
            }
            c += 1;
        }
        if lands != 1 {
            return false;
        }
        lands_total += lands;
        if scene == Scene::ShCmd {
            if excused != 1 {
                return false;
            }
        } else if excused != 0 {
            return false;
        }
        r += 1;
    }
    if lands_total != LANDS_CELLS {
        return false;
    }
    if g.seal() == 0 {
        return false;
    }
    if g.seal() != ExitGrid::build().seal() {
        return false;
    }
    let text = g.render();
    if text.len() == 0 {
        return false;
    }
    if g.row_text(Scene::Dispatch).len() == 0 {
        return false;
    }
    if g.cell(Scene::Dispatch, EXIT_NOTFOUND) != Some(CellVerdict::Lands) {
        return false;
    }
    if g.cell(Scene::ShCmd, EXIT_NOTFOUND) != Some(CellVerdict::Excused) {
        return false;
    }
    if g.cell(Scene::Exec, EXIT_NOTFOUND) != Some(CellVerdict::Contradicts) {
        return false;
    }
    if g.cell_at(SCENE_COUNT, 0).is_some() {
        return false;
    }
    if g.cell(Scene::Dispatch, 200).is_some() {
        return false;
    }
    true
}

/// 05 八闸纵线：正例走到 Grid，各类缺陷各挂各的段与码。
fn define_ok() -> bool {
    let scenes = Scene::all();
    let mut i = 0usize;
    while i < SCENE_COUNT {
        let scene = match scenes.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let o = define(&Shape::lands(scene));
        if !o.ok() {
            return false;
        }
        if o.stage != Stage::Grid {
            return false;
        }
        if o.note != Note::Lands {
            return false;
        }
        if o.value != 1 {
            return false;
        }
        i += 1;
    }
    let excused = Shape::lands(Scene::ShCmd)
        .with_code(EXIT_NOTFOUND)
        .with_text(T_SHCMD_DIR_LINE)
        .with_declared(true);
    let oe = define(&excused);
    if !oe.ok() || oe.stage != Stage::Code || oe.verdict != CellVerdict::Excused {
        return false;
    }
    let bad = Shape::lands(Scene::Dispatch).with_code(200);
    let ob = define(&bad);
    if ob.stage != Stage::Arg || ob.code != EM_EARG || ob.ok() {
        return false;
    }
    let wrong_layer = Shape::lands(Scene::Dispatch).with_layer(Layer::Shell);
    let ol = define(&wrong_layer);
    if ol.stage != Stage::Layer || ol.code != EM_ELAYER || ol.ok() {
        return false;
    }
    let hit = Shape::lands(Scene::Builtin).with_lookup(true);
    let oh = define(&hit);
    if oh.stage != Stage::Lookup || oh.code != EM_ELOOK || oh.ok() {
        return false;
    }
    let wrong_text = Shape::lands(Scene::ShCmd).with_text(T_SHCMD_NOLINE);
    let ot = define(&wrong_text);
    if ot.stage != Stage::Text || ot.code != EM_ETEXT || ot.ok() {
        return false;
    }
    let swallow = Shape::lands(Scene::Dispatch).with_code(EXIT_ZERO);
    let os = define(&swallow);
    if os.stage != Stage::Band || os.code != EM_EBAND || os.ok() {
        return false;
    }
    let same_band = Shape::lands(Scene::Exec).with_code(EXIT_USAGE);
    let oc = define(&same_band);
    if oc.stage != Stage::Code || oc.code != EM_ECODE || oc.ok() {
        return false;
    }
    let phantom = Shape::lands(Scene::Dispatch).with_declared(true);
    let op = define(&phantom);
    if op.stage != Stage::Grid || op.code != EM_EGRID || op.ok() {
        return false;
    }
    let unjudged = Shape::lands(Scene::Alias).with_unjudged(true);
    let ou = define(&unjudged);
    if ou.stage != Stage::Rate || ou.code != EM_EDEFAULT || ou.ok() {
        return false;
    }
    if !ou.default {
        return false;
    }
    if !oe.counts() || ob.counts() || ou.counts() {
        return false;
    }
    if !ob.refused() || !ol.refused() || !oh.refused() {
        return false;
    }
    if ot.refused() || os.refused() || ou.refused() {
        return false;
    }
    if Stage::all().len() != 8 {
        return false;
    }
    let mut r = 1u32;
    while r <= 8 {
        let st = match Stage::of_rank(r) {
            Some(x) => x,
            None => return false,
        };
        if st.rank() != r {
            return false;
        }
        if st.fault_code() == 0 {
            return false;
        }
        r += 1;
    }
    if Stage::of_rank(0).is_some() || Stage::of_rank(9).is_some() {
        return false;
    }
    if Stage::Arg.is_conclusion() || Stage::Code.is_conclusion() {
        return false;
    }
    if !Stage::Grid.is_conclusion() || !Stage::Rate.is_conclusion() {
        return false;
    }
    true
}

/// 06 语料十四槽互校：申报、期望表、判定三路必须给同一结论。
fn corpus_ok() -> bool {
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS {
        return false;
    }
    if WANT_STAGE.len() != CORPUS || WANT_CODE.len() != CORPUS {
        return false;
    }
    if WANT_NOTE.len() != CORPUS || WANT_TAGS.len() != CORPUS {
        return false;
    }
    if WANT_VALUE.len() != CORPUS || WANT_SUCCESS.len() != CORPUS {
        return false;
    }
    if WANT_NDX.len() != CORPUS || WANT_DEFAULT.len() != CORPUS {
        return false;
    }
    if WANT_VERDICT.len() != CORPUS {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return false,
        };
        if c.tag() != c.claim.tag {
            return false;
        }
        let want_tag = match WANT_TAGS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if c.tag() != want_tag {
            return false;
        }
        if c.tag().len() < 7 {
            return false;
        }
        if i < ACCEPT {
            if c.tag().len() == 0 {
                return false;
            }
        }
        let o = c.actual();
        if WANT_STAGE.get(i).copied() != Some(o.stage) {
            return false;
        }
        if WANT_CODE.get(i).copied() != Some(o.code) {
            return false;
        }
        if WANT_NOTE.get(i).copied() != Some(o.note) {
            return false;
        }
        if WANT_VERDICT.get(i).copied() != Some(o.verdict) {
            return false;
        }
        if WANT_VALUE.get(i).copied() != Some(o.value) {
            return false;
        }
        if WANT_SUCCESS.get(i).copied() != Some(o.ok()) {
            return false;
        }
        if WANT_DEFAULT.get(i).copied() != Some(o.default) {
            return false;
        }
        if judge(&c) != expected_reject_ndx(i) {
            return false;
        }
        if i < ACCEPT && !c.ok() {
            return false;
        }
        if i >= ACCEPT && c.ok() {
            return false;
        }
        let mut j = i + 1usize;
        while j < CORPUS {
            if let Some(other) = case_at(j) {
                if other.seal() == c.seal() {
                    return false;
                }
            }
            j += 1;
        }
        if cover(i).is_err() {
            return false;
        }
        i += 1;
    }
    if cover(CORPUS + 1).is_ok() {
        return false;
    }
    let all = cases();
    if all.len() != CORPUS {
        return false;
    }
    if all.get(0).map(|c| c.tag()) != Some("f11679-dispatch-127") {
        return false;
    }
    if SHAPES.get(0) != Some(&BASE) {
        return false;
    }
    if CLAIMS.get(0).map(|c| c.tag) != Some("f11679-dispatch-127") {
        return false;
    }
    let excused = match CLAIMS.get(6) {
        Some(c) => *c,
        None => return false,
    };
    if excused.tag != "f11679-shcmd-excused-127" {
        return false;
    }
    if excused.stage != Stage::Code || excused.verdict != CellVerdict::Excused {
        return false;
    }
    if excused.note != Note::RegisteredExcuse {
        return false;
    }
    true
}

/// 07 判序六维互斥：六维一一对应、顺序固定、各指各的。
fn judge_ok() -> bool {
    let mut n = 0usize;
    while n <= 6 {
        let f = match ndx_fault(n) {
            Some(x) => x,
            None => return false,
        };
        if fault_ndx(f) != n {
            return false;
        }
        if n < REJECT {
            let want = match REJECT_CODES.get(n) {
                Some(x) => *x,
                None => return false,
            };
            if f.code() != want {
                return false;
            }
        }
        n += 1;
    }
    if ndx_fault(7).is_some() {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return false,
        };
        if judge(&c) != expected_reject_ndx(i) {
            return false;
        }
        i += 1;
    }
    // 次序：一条同时谎报成败与阶段的申报，必须挂 Estate。
    let c = match case_at(8) {
        Some(x) => x,
        None => return false,
    };
    let mut cross = c.claim;
    cross.stage = Stage::Rate;
    let case_cross = Case {
        shape: c.shape,
        claim: cross,
    };
    if judge(&case_cross) != Fault::Estate {
        return false;
    }
    // 次序：一条只谎报阶段的申报，必须挂 Stage 而不是后面的码。
    let c9 = match case_at(9) {
        Some(x) => x,
        None => return false,
    };
    let mut stage_only = c9.claim;
    stage_only.code = 0;
    stage_only.value = 1;
    stage_only.note = c9.actual().note;
    let case_stage = Case {
        shape: c9.shape,
        claim: stage_only,
    };
    if judge(&case_stage) != Fault::Stage {
        return false;
    }
    // 次序：码与值同时谎报，必须挂 Code。
    let c10 = match case_at(10) {
        Some(x) => x,
        None => return false,
    };
    let mut code_first = c10.claim;
    code_first.value = 7;
    let case_code = Case {
        shape: c10.shape,
        claim: code_first,
    };
    if judge(&case_code) != Fault::Code {
        return false;
    }
    // 次序：值、注记同时谎报，必须挂 Value。
    let c11 = match case_at(11) {
        Some(x) => x,
        None => return false,
    };
    let mut value_first = c11.claim;
    value_first.note = Note::Lands;
    let case_value = Case {
        shape: c11.shape,
        claim: value_first,
    };
    if judge(&case_value) != Fault::Value {
        return false;
    }
    // 次序：注记与兜底同时谎报，必须挂 Note。
    let c13 = match case_at(13) {
        Some(x) => x,
        None => return false,
    };
    let mut note_first = c13.claim;
    note_first.note = Note::Lands;
    let case_note = Case {
        shape: c13.shape,
        claim: note_first,
    };
    if judge(&case_note) != Fault::Note {
        return false;
    }
    // 兜底：只谎报兜底位的一条，必须挂 Default。
    let only_default = Case {
        shape: c13.shape,
        claim: c13.claim,
    };
    if judge(&only_default) != Fault::Default {
        return false;
    }
    if Fault::Ok.code() != 0 {
        return false;
    }
    true
}

/// 08 四档矩阵覆盖：四档全有落点、全对、零红。
fn matrix_ok() -> bool {
    let m = Matrix::build();
    if m.slots() != CORPUS * TIER_COUNT {
        return false;
    }
    if m.goods() != CORPUS {
        return false;
    }
    if m.red_total() + m.goods() != m.slots() {
        return false;
    }
    if !m.covered() {
        return false;
    }
    let want = [
        (Tier::Lands, LANDS_SAMPLES),
        (Tier::Excused, EXCUSED_SAMPLES),
        (Tier::Void, VOID_SAMPLES),
        (Tier::Unrun, UNRUN_SAMPLES),
    ];
    let mut i = 0usize;
    while i < want.len() {
        let (tier, good) = match want.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let r = match m.row(tier) {
            Some(x) => x,
            None => return false,
        };
        if r.good != good {
            return false;
        }
        if r.total != CORPUS {
            return false;
        }
        if r.red() + r.good != r.total {
            return false;
        }
        if tier.is_green() && r.good == 0 {
            return false;
        }
        if !tier.is_green() && r.good == 0 {
            return false;
        }
        i += 1;
    }
    if m.render().len() == 0 {
        return false;
    }
    let lands_row = match m.row(Tier::Lands) {
        Some(r) => r,
        None => return false,
    };
    if lands_row.render().as_str() != "lands=7/14" {
        return false;
    }
    if Matrix::build().goods() != m.goods() {
        return false;
    }
    if Tier::all().len() != TIER_COUNT {
        return false;
    }
    if !Tier::Lands.is_green() || !Tier::Excused.is_green() {
        return false;
    }
    if Tier::Void.is_green() || Tier::Unrun.is_green() {
        return false;
    }
    // 归档函数本身：判死结果必须进 Void，未判结果必须进 Unrun。
    let dead = match SHAPES.get(ACCEPT + 1) {
        Some(x) => *x,
        None => return false,
    };
    if Tier::of(&define(&dead)) != Tier::Void {
        return false;
    }
    let unjudged = match SHAPES.get(CORPUS - 1) {
        Some(x) => *x,
        None => return false,
    };
    if Tier::of(&define(&unjudged)) != Tier::Unrun {
        return false;
    }
    true
}

/// 09 全绿渲染逐字：报告、渲染、闸门三处必须同口径。
fn render_ok() -> bool {
    let rep = match run() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if !rep.green() {
        return false;
    }
    if rep.render().as_str() != RENDER_OK {
        return false;
    }
    if rep.rate() != RATE_FULL {
        return false;
    }
    if rep.total != GRID_CELLS {
        return false;
    }
    if rep.decided_cells() != rep.total {
        return false;
    }
    if rep.conflicts != 0 {
        return false;
    }
    if rep.matched != rep.total {
        return false;
    }
    if rep.accept != ACCEPT || rep.reject != REJECT {
        return false;
    }
    if rep.lands != LANDS_CELLS as usize {
        return false;
    }
    if rep.excused != EXCUSED_CELLS as usize {
        return false;
    }
    if rep.contradicts != CONTRADICTS_CELLS as usize {
        return false;
    }
    if rep.void != VOID_SAMPLES || rep.unrun != UNRUN_SAMPLES {
        return false;
    }
    if rep.reds() != 0 {
        return false;
    }
    if rep.gate() != Ok(()) {
        return false;
    }
    let empty = empty_report();
    if empty.total != 0 || empty.green() || empty.rate() != 0 {
        return false;
    }
    if empty.gate() != Err(EM_EGRID) {
        return false;
    }
    if empty.render().as_str() == RENDER_OK {
        return false;
    }
    if !report_agrees_with_matrix() {
        return false;
    }
    if !red_rate_calibrated() {
        return false;
    }
    let injected = match run_with(Some((0, Fault::Value))) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if injected.conflicts == 0 || injected.rate() >= RATE_FULL {
        return false;
    }
    if injected.gate() == Ok(()) {
        return false;
    }
    if run_with(Some((CORPUS, Fault::Ok))) != Err(EM_ESLOT) {
        return false;
    }
    if run_with(Some((CORPUS + 3, Fault::Note))) != Err(EM_ESLOT) {
        return false;
    }
    true
}

/// 10 阶段闸门不倒退：正序、倒退、封账后三种路径各指各的。
fn gate_ok() -> bool {
    let root = match Gate::full_pass() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if root == 0 {
        return false;
    }
    let mut g = Gate::new();
    if g.close().is_ok() {
        return false;
    }
    if g.stage().is_some() {
        return false;
    }
    let table = Stage::all();
    let mut i = 0usize;
    while i < table.len() {
        let st = match table.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if g.step(st) != Ok(()) {
            return false;
        }
        i += 1;
    }
    if g.steps() != 8 {
        return false;
    }
    if g.refusals() != 0 {
        return false;
    }
    if g.stage() != Some(Stage::Rate) {
        return false;
    }
    let root2 = match g.close() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if root2 != root {
        return false;
    }
    if g.close() != Err(EM_ESTATE) {
        return false;
    }
    if g.step(Stage::Rate) != Err(EM_ESTATE) {
        return false;
    }
    if g.refusals() != 2 {
        return false;
    }
    // 倒退：Grid 之后退到 Code。
    let mut reg = Gate::new();
    if reg.step(Stage::Grid) != Ok(()) {
        return false;
    }
    if reg.step(Stage::Code) != Err(EM_ESTAGE) {
        return false;
    }
    if reg.stage() != Some(Stage::Grid) {
        return false;
    }
    if reg.refusals() != 1 {
        return false;
    }
    if reg.steps() != 1 {
        return false;
    }
    // 同档重走不算倒退。
    if reg.step(Stage::Grid) != Ok(()) {
        return false;
    }
    if reg.steps() != 2 {
        return false;
    }
    // 跳档（前进）允许：纵线只禁倒退。
    let mut skip = Gate::new();
    if skip.step(Stage::Arg) != Ok(()) {
        return false;
    }
    if skip.step(Stage::Rate) != Ok(()) {
        return false;
    }
    if skip.steps() != 2 {
        return false;
    }
    // 空走档：一步不走直接封账。
    let mut none = Gate::new();
    if none.close().is_ok() {
        return false;
    }
    if none.is_closed() {
        return false;
    }
    // 走档串：中途倒退必须把码交出来。
    let bad = [Stage::Arg, Stage::Grid, Stage::Band];
    if Gate::walk(&bad) != Err(EM_ESTAGE) {
        return false;
    }
    let good = [Stage::Arg, Stage::Band, Stage::Grid];
    let w = match Gate::walk(&good) {
        Ok(x) => x,
        Err(_) => return false,
    };
    if w.steps() != 3 || w.refusals() != 0 {
        return false;
    }
    if w.render().len() == 0 {
        return false;
    }
    if Stage::Arg.rank() != 1 || Stage::Rate.rank() != 8 {
        return false;
    }
    true
}

/// 11 账本逐格复核：格数、统计、缺陷码、拒收计数全部对齐。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.len() != SAMPLES {
        return false;
    }
    if l.is_empty() {
        return false;
    }
    if l.ok_count() != STAT_CELLS + 1 {
        return false;
    }
    if l.err_count() != REJECT {
        return false;
    }
    if l.rejects() != 0 {
        return false;
    }
    if l.audit() != Ok(()) {
        return false;
    }
    if l.first_mismatch().is_some() {
        return false;
    }
    if l.is_frozen() || l.root() != 0 {
        return false;
    }
    let b0 = build_ledger_broken(0);
    if b0.audit() != Err(EM_EVALUE) {
        return false;
    }
    let b1 = build_ledger_broken(1);
    if b1.audit() != Err(EM_EVALUE) {
        return false;
    }
    let b2 = build_ledger_broken(2);
    if b2.audit() != Err(EM_EVALUE) {
        return false;
    }
    if build_ledger_broken(9).audit() != Ok(()) {
        return false;
    }
    let mut dup = Ledger::new();
    let s = Sample {
        slot: 0,
        op: 1,
        ok: true,
        code: 0,
        value: 1,
    };
    if dup.register(s) != Ok(()) {
        return false;
    }
    if dup.register(s) != Err(EM_ESLOT) {
        return false;
    }
    if dup.rejects() != 1 {
        return false;
    }
    if dup.len() != 1 {
        return false;
    }
    let hole = Sample {
        slot: 5,
        op: 2,
        ok: true,
        code: 0,
        value: 1,
    };
    if dup.register(hole) != Err(EM_ESLOT) {
        return false;
    }
    if dup.rejects() != 2 {
        return false;
    }
    if dup.replay().is_ok() {
        return false;
    }
    let mut full = build_ledger();
    if full.replay().is_ok() {
        return false;
    }
    let h = match full.freeze() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if h == 0 || !full.is_frozen() || full.root() != h {
        return false;
    }
    if full.freeze() != Err(EM_ESTATE) {
        return false;
    }
    if full.poke(0, 7) != Err(EM_ESTATE) {
        return false;
    }
    if full.register(s) != Err(EM_ESTATE) {
        return false;
    }
    if full.replay() != Ok(h) {
        return false;
    }
    if full.render().len() == 0 {
        return false;
    }
    let mut partial = build_ledger();
    match partial.poke(STAT_CELLS + 1, 42) {
        Ok(()) => {}
        Err(_) => return false,
    }
    if partial.audit() != Err(EM_EVALUE) {
        return false;
    }
    if partial.poke(SAMPLES + 2, 1) != Err(EM_ESLOT) {
        return false;
    }
    if partial.sample(SAMPLES).is_some() {
        return false;
    }
    if l.seal() == 0 {
        return false;
    }
    if l.seal() == build_ledger_broken(0).seal() {
        return false;
    }
    let op0 = match STAT_OPS.get(0) {
        Some(x) => *x,
        None => return false,
    };
    let want0 = Sample {
        slot: 0,
        op: op0,
        ok: true,
        code: 0,
        value: head_val(),
    };
    if l.sample(0) != Some(want0) {
        return false;
    }
    true
}

/// 12 例外凭据成对：唯一例外格必须有凭据，凭据用错地方必须判死。
fn exception_ok() -> bool {
    let g = ExitGrid::build();
    if g.excused() != EXCUSED_CELLS {
        return false;
    }
    if g.cell(Scene::ShCmd, EXIT_NOTFOUND) != Some(CellVerdict::Excused) {
        return false;
    }
    let mut others = 0u32;
    let scenes = Scene::all();
    let mut r = 0usize;
    while r < SCENE_COUNT {
        let scene = match scenes.get(r) {
            Some(x) => *x,
            None => return false,
        };
        if scene != Scene::ShCmd {
            if let Some(v) = g.cell(scene, EXIT_NOTFOUND) {
                if v == CellVerdict::Excused {
                    others += 1;
                }
            }
        }
        r += 1;
    }
    if others != 0 {
        return false;
    }
    let with = Shape::lands(Scene::ShCmd)
        .with_code(EXIT_NOTFOUND)
        .with_text(T_SHCMD_DIR_LINE)
        .with_declared(true);
    let ow = define(&with);
    if !ow.ok() || ow.verdict != CellVerdict::Excused {
        return false;
    }
    let without = Shape::lands(Scene::ShCmd)
        .with_code(EXIT_NOTFOUND)
        .with_text(T_SHCMD_DIR_LINE);
    let ob = define(&without);
    if ob.ok() || ob.stage != Stage::Code || ob.code != EM_ECODE {
        return false;
    }
    // 凭据挂在合法格上：多余凭据同样判死。
    let over = Shape::lands(Scene::Dispatch).with_declared(true);
    let oo = define(&over);
    if oo.ok() || oo.code != EM_EGRID || oo.note != Note::PhantomExcuse {
        return false;
    }
    // 凭据挂在别的场景的 127 上：exec 的 127 根本不在 Parse 带。
    let exec_decl = Shape::lands(Scene::Exec)
        .with_code(EXIT_NOTFOUND)
        .with_declared(true);
    let od = define(&exec_decl);
    if od.ok() || od.stage != Stage::Band {
        return false;
    }
    // 例外文案只对 sh-cmd 成立；同一行字放到 dispatch 上必须判死。
    if !Scene::ShCmd.text_ok(T_SHCMD_DIR_LINE) {
        return false;
    }
    if Scene::Dispatch.text_ok(T_SHCMD_DIR_LINE) {
        return false;
    }
    if Scene::Builtin.text_ok(T_SHCMD_DIR_LINE) {
        return false;
    }
    // 语料里恰好一槽带凭据。
    let mut declared = 0usize;
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(c) = case_at(i) {
            if c.shape.declared {
                declared += 1;
            }
            if c.shape.declared && c.shape.scene != Scene::ShCmd {
                return false;
            }
        }
        i += 1;
    }
    if declared != EXCUSED_SAMPLES {
        return false;
    }
    true
}

/// 13 汇总用：四注入 + 台账 + 矩阵。
fn freeze_ok() -> bool {
    let mut l = build_ledger();
    if l.audit() != Ok(()) {
        return false;
    }
    let h = match l.freeze() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if h == 0 || !l.is_frozen() {
        return false;
    }
    if l.replay() != Ok(h) {
        return false;
    }
    let four = inject_four();
    let mut i = 0usize;
    let mut all = true;
    while i < four.len() {
        if let Some(x) = four.get(i) {
            if !*x {
                all = false;
            }
        }
        i += 1;
    }
    let g = ExitGrid::build();
    all && g.decided() && g.every_cell_judged() && report_agrees_with_matrix()
}

/// 汇总：十三条判据，一条不多一条不少。
pub fn run_unx11679_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11679_unknown_applet_exit_matrix");
    s.add("EM01-名称与段基址", name_ok(), "static");
    s.add("EM02-十三码互异", codes_ok(), "static");
    s.add("EM03-五场景全表", scenes_ok(), "static");
    s.add("EM04-二十五格零悬空", grid_ok(), "static");
    s.add("EM05-八闸纵线", define_ok(), "static");
    s.add("EM06-语料十四槽互校", corpus_ok(), "static");
    s.add("EM07-判序六维互斥", judge_ok(), "static");
    s.add("EM08-四档矩阵覆盖", matrix_ok(), "static");
    s.add("EM09-全绿渲染逐字", render_ok(), "static");
    s.add("EM10-阶段闸门不倒退", gate_ok(), "static");
    s.add("EM11-账本逐格复核", ledger_ok(), "static");
    s.add("EM12-例外凭据成对", exception_ok(), "static");
    s.add("EM13-冻结重放四注入", freeze_ok(), "static");
    s
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_seg_base_is_pinned() {
        assert_eq!(SEG_BASE, 0x6d2e0);
        assert_eq!(ANCHOR_F11678_SEG, SEG_BASE - 0x10);
        assert_eq!(ANCHOR_F11680_SEG, SEG_BASE + 0x10);
        assert_eq!(ANCHOR_F11675_SEG + ANCHOR_F11675_GAP, SEG_BASE);
    }

    #[test]
    fn t02_scale_constants_are_contract_sizes() {
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(STAGE_COUNT, 8);
        assert_eq!(SCENE_COUNT, 5);
        assert_eq!(CODE_COUNT, 5);
        assert_eq!(GRID_CELLS, 25);
    }

    #[test]
    fn t03_three_cell_classes_sum_to_the_grid() {
        assert_eq!(LANDS_CELLS + EXCUSED_CELLS + CONTRADICTS_CELLS, 25);
        assert_eq!(LANDS_CELLS, 5);
        assert_eq!(EXCUSED_CELLS, 1);
        assert_eq!(CONTRADICTS_CELLS, 19);
    }

    #[test]
    fn t04_thirteen_codes_live_inside_the_segment() {
        assert_eq!(ALL_CODES.len(), 13);
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            assert!(ALL_CODES[i] > SEG_BASE);
            assert!(ALL_CODES[i] <= SEG_BASE + 0x0d);
            i += 1;
        }
    }

    #[test]
    fn t05_thirteen_codes_are_pairwise_distinct() {
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            let mut j = i + 1usize;
            while j < ALL_CODES.len() {
                assert_ne!(ALL_CODES[i], ALL_CODES[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn t06_reject_codes_follow_the_fault_order() {
        let faults = Fault::reject_faults();
        assert_eq!(faults.len(), REJECT);
        let mut k = 0usize;
        while k < REJECT {
            assert_eq!(REJECT_CODES[k], faults[k].code());
            assert_eq!(REJECT_FAULTS[k], faults[k]);
            k += 1;
        }
    }

    #[test]
    fn t07_vals_cells_are_the_pinned_statistics() {
        assert_eq!(VALS[0] as usize, SCENE_COUNT);
        assert_eq!(VALS[1] as usize, CODE_COUNT);
        assert_eq!(VALS[2] as usize, GRID_CELLS);
        assert_eq!(VALS[3], LANDS_CELLS);
        assert_eq!(VALS[4], EXCUSED_CELLS);
        assert_eq!(VALS[5], CONTRADICTS_CELLS);
        assert_eq!(VALS[6], RATE_FULL);
        assert_eq!(VALS[7], RED_RATE);
    }

    #[test]
    fn t08_fold_helpers_are_structured() {
        let h = FNV_OFF;
        assert_eq!(fold_byte(h, b'A'), fold_str(h, "A"));
        assert_ne!(fold_byte(h, b'A'), h);
        assert_eq!(fold_str(h, "onx"), fold_str(fold_str(fold_str(h, "o"), "n"), "x"));
        assert_ne!(fold_word(h, 1), fold_word(h, 2));
        assert_ne!(fold_bool(h, false), fold_bool(h, true));
        assert_ne!(fold_i32(h, -1), fold_i32(h, 1));
    }

    #[test]
    fn t09_decimal_and_hex_renderers() {
        assert_eq!(dec_text(0).as_str(), "0");
        assert_eq!(dec_text(127).as_str(), "127");
        assert_eq!(u32_text(1000).as_str(), "1000");
        assert_eq!(hex_text(0x0a, 4).as_str(), "000A");
        assert_eq!(hex_text(SEG_BASE, 5).as_str(), "6D2E0");
        assert_eq!(i32_text(-3).as_str(), "-3");
        assert_eq!(i32_text(0).as_str(), "0");
    }

    #[test]
    fn t10_text_renderers() {
        assert_eq!(seg_text(SEG_BASE).as_str(), "0x6D2E0");
        assert_eq!(permille_text(929).as_str(), "929permille");
        assert_eq!(code_text(127).as_str(), "exit=127");
        assert_eq!(bool_text(true), "true");
        assert_eq!(bool_text(false), "false");
    }

    #[test]
    fn t11_scene_table_order_and_names() {
        let table = Scene::all();
        assert_eq!(table.len(), SCENE_COUNT);
        assert_eq!(table[0].name(), "dispatch");
        assert_eq!(table[1].name(), "alias");
        assert_eq!(table[2].name(), "builtin");
        assert_eq!(table[3].name(), "sh-cmd");
        assert_eq!(table[4].name(), "exec");
    }

    #[test]
    fn t12_scene_ids_roundtrip() {
        let table = Scene::all();
        let mut i = 0usize;
        while i < SCENE_COUNT {
            assert_eq!(table[i].id(), i);
            assert_eq!(Scene::of_id(i), Some(table[i]));
            i += 1;
        }
        assert!(Scene::of_id(SCENE_COUNT).is_none());
        assert!(Scene::of_id(SCENE_COUNT + 4).is_none());
    }

    #[test]
    fn t13_scene_layers_are_fixed() {
        assert_eq!(Scene::Dispatch.layer(), Layer::Busybox);
        assert_eq!(Scene::Alias.layer(), Layer::Busybox);
        assert_eq!(Scene::Builtin.layer(), Layer::Shell);
        assert_eq!(Scene::ShCmd.layer(), Layer::Shell);
        assert_eq!(Scene::Exec.layer(), Layer::Caller);
        assert!(!Layer::Caller.speaks());
        assert!(Layer::Busybox.speaks());
        assert!(Layer::Shell.speaks());
    }

    #[test]
    fn t14_scene_bands_are_fixed() {
        assert_eq!(Scene::Dispatch.band(), CodeBand::Parse);
        assert_eq!(Scene::Alias.band(), CodeBand::Parse);
        assert_eq!(Scene::Builtin.band(), CodeBand::Parse);
        assert_eq!(Scene::ShCmd.band(), CodeBand::Parse);
        assert_eq!(Scene::Exec.band(), CodeBand::Tool);
        assert!(!CodeBand::Swallow.legal());
        assert!(CodeBand::Tool.legal());
        assert!(CodeBand::Parse.legal());
    }

    #[test]
    fn t15_scene_expected_codes() {
        assert_eq!(Scene::Dispatch.expected_code(), 127);
        assert_eq!(Scene::Alias.expected_code(), 127);
        assert_eq!(Scene::Builtin.expected_code(), 127);
        assert_eq!(Scene::ShCmd.expected_code(), 126);
        assert_eq!(Scene::Exec.expected_code(), 1);
        assert_eq!(EXIT_ZERO, 0);
        assert_eq!(EXIT_FAIL, 1);
        assert_eq!(EXIT_USAGE, 2);
        assert_eq!(EXIT_NOEXEC, 126);
        assert_eq!(EXIT_NOTFOUND, 127);
    }

    #[test]
    fn t16_argv_sources_are_distinct_per_lookup_owner() {
        assert_eq!(Scene::Dispatch.argv_source(), "argv[1]");
        assert_eq!(Scene::Alias.argv_source(), "argv[0]");
        assert_eq!(Scene::Builtin.argv_source(), "builtin");
        assert_eq!(Scene::ShCmd.argv_source(), "path-search");
        assert_eq!(Scene::Exec.argv_source(), "execve");
    }

    #[test]
    fn t17_scene_texts_match_the_msg_builders() {
        assert_eq!(Scene::Dispatch.text(), T_BUSYBOX_LINE);
        assert_eq!(Scene::Alias.text(), T_BUSYBOX_LINE);
        assert_eq!(Scene::Builtin.text(), T_BUILTIN_LINE);
        assert_eq!(Scene::ShCmd.text(), T_SHCMD_LINE);
        assert_eq!(Scene::Exec.text(), T_SILENT);
        assert_eq!(msg_applet_not_found("nosuch").as_str(), T_BUSYBOX_LINE);
        assert_eq!(msg_shell_not_found("nosuch").as_str(), T_BUILTIN_LINE);
        assert_eq!(msg_shell_noexec("/tmp/nosuch").as_str(), T_SHCMD_LINE);
        assert_eq!(msg_shell_isdir("/tmp/nosuch").as_str(), T_SHCMD_DIR_LINE);
    }

    #[test]
    fn t18_text_shapes_accept_only_registered_variants() {
        assert!(Scene::ShCmd.text_ok(T_SHCMD_LINE));
        assert!(Scene::ShCmd.text_ok(T_SHCMD_DIR_LINE));
        assert!(!Scene::ShCmd.text_ok(T_SHCMD_NOLINE));
        assert!(Scene::Dispatch.text_ok(T_BUSYBOX_LINE));
        assert!(!Scene::Dispatch.text_ok(T_SHCMD_DIR_LINE));
        assert!(!Scene::Dispatch.text_ok(T_SILENT));
        assert!(Scene::Exec.text_ok(T_SILENT));
    }

    #[test]
    fn t19_only_sh_cmd_has_excuse_eligibility() {
        assert!(Scene::ShCmd.excuses_127());
        let table = Scene::all();
        let mut i = 0usize;
        while i < SCENE_COUNT {
            if table[i] != Scene::ShCmd {
                assert!(!table[i].excuses_127());
            }
            i += 1;
        }
    }

    #[test]
    fn t20_bands_cover_the_five_codes() {
        assert_eq!(band_of(0), Some(CodeBand::Swallow));
        assert_eq!(band_of(1), Some(CodeBand::Tool));
        assert_eq!(band_of(2), Some(CodeBand::Tool));
        assert_eq!(band_of(126), Some(CodeBand::Parse));
        assert_eq!(band_of(127), Some(CodeBand::Parse));
        assert_eq!(band_of(3), None);
        assert_eq!(band_of(200), None);
        assert_eq!(CodeBand::Swallow.id(), 1);
        assert_eq!(CodeBand::Tool.id(), 2);
        assert_eq!(CodeBand::Parse.id(), 3);
    }

    #[test]
    fn t21_code_table_membership_and_index() {
        let mut i = 0usize;
        while i < CODE_COUNT {
            assert!(code_in_table(CODE_TABLE[i]));
            assert_eq!(code_index(CODE_TABLE[i]), Some(i));
            i += 1;
        }
        assert!(!code_in_table(3));
        assert!(!code_in_table(200));
        assert!(code_index(3).is_none());
        assert!(code_in_table(EXIT_USAGE_TOOL));
    }

    #[test]
    fn t22_verdict_rule_lands_excuses_contradicts() {
        let table = Scene::all();
        let mut r = 0usize;
        while r < SCENE_COUNT {
            let scene = table[r];
            assert_eq!(verdict_of(scene, scene.expected_code()), CellVerdict::Lands);
            let mut c = 0usize;
            while c < CODE_COUNT {
                let code = CODE_TABLE[c];
                if code != scene.expected_code() {
                    if scene == Scene::ShCmd && code == EXIT_NOTFOUND {
                        assert_eq!(verdict_of(scene, code), CellVerdict::Excused);
                    } else {
                        assert_eq!(verdict_of(scene, code), CellVerdict::Contradicts);
                    }
                }
                c += 1;
            }
            r += 1;
        }
        assert!(CellVerdict::Lands.counts());
        assert!(CellVerdict::Excused.counts());
        assert!(!CellVerdict::Contradicts.counts());
    }

    #[test]
    fn t23_grid_counts_and_zero_dangling() {
        let g = ExitGrid::build();
        assert_eq!(g.lands(), LANDS_CELLS);
        assert_eq!(g.excused(), EXCUSED_CELLS);
        assert_eq!(g.contradicts(), CONTRADICTS_CELLS);
        assert!(g.decided());
        assert!(g.every_cell_judged());
        assert_eq!(g.cell_at(SCENE_COUNT, 0), None);
        assert_eq!(g.cell_at(0, CODE_COUNT), None);
        assert_eq!(g.cell(Scene::Dispatch, 3), None);
    }

    #[test]
    fn t24_each_row_has_exactly_one_lands_cell() {
        let g = ExitGrid::build();
        let table = Scene::all();
        let mut r = 0usize;
        while r < SCENE_COUNT {
            let mut lands = 0u32;
            let mut c = 0usize;
            while c < CODE_COUNT {
                if g.cell_at(r, c) == Some(CellVerdict::Lands) {
                    lands += 1;
                }
                c += 1;
            }
            assert_eq!(lands, 1);
            r += 1;
        }
        assert_eq!(table.len(), SCENE_COUNT);
    }

    #[test]
    fn t25_grid_seal_is_deterministic() {
        let a = ExitGrid::build();
        let b = ExitGrid::build();
        assert_eq!(a.seal(), b.seal());
        assert_ne!(a.seal(), 0);
        assert_eq!(a, b);
    }

    #[test]
    fn t26_grid_row_text_is_the_five_columns() {
        let g = ExitGrid::build();
        let row = g.row_text(Scene::Dispatch);
        assert!(row.starts_with("dispatch "));
        assert!(row.contains("127=lands"));
        assert!(row.contains("0=contradicts"));
        let render = g.render();
        assert!(render.starts_with("exit-grid"));
        assert!(render.contains("sh-cmd"));
        assert!(render.contains("exec"));
    }

    #[test]
    fn t27_shape_lands_passes_every_scene() {
        let table = Scene::all();
        let mut i = 0usize;
        while i < SCENE_COUNT {
            let o = define(&Shape::lands(table[i]));
            assert!(o.ok());
            assert_eq!(o.stage, Stage::Grid);
            assert_eq!(o.note, Note::Lands);
            assert_eq!(o.verdict, CellVerdict::Lands);
            assert_eq!(o.value, 1);
            i += 1;
        }
    }

    #[test]
    fn t28_excused_cell_needs_a_credential() {
        let with = Shape::lands(Scene::ShCmd)
            .with_code(EXIT_NOTFOUND)
            .with_text(T_SHCMD_DIR_LINE)
            .with_declared(true);
        let o = define(&with);
        assert!(o.ok());
        assert_eq!(o.stage, Stage::Code);
        assert_eq!(o.verdict, CellVerdict::Excused);
        assert_eq!(o.note, Note::RegisteredExcuse);
        let without = with.with_declared(false);
        let b = define(&without);
        assert!(!b.ok());
        assert_eq!(b.code, EM_ECODE);
        assert_eq!(b.stage, Stage::Code);
    }

    #[test]
    fn t29_swallow_is_a_cross_band_defect() {
        let s = Shape::lands(Scene::Dispatch).with_code(EXIT_ZERO);
        let o = define(&s);
        assert!(!o.ok());
        assert_eq!(o.stage, Stage::Band);
        assert_eq!(o.code, EM_EBAND);
        assert_eq!(o.note, Note::CrossBand);
        assert!(band_of(0) != Some(Scene::Dispatch.band()));
    }

    #[test]
    fn t30_same_band_wrong_code_is_a_code_defect() {
        let s = Shape::lands(Scene::Exec).with_code(EXIT_USAGE);
        let o = define(&s);
        assert!(!o.ok());
        assert_eq!(o.stage, Stage::Code);
        assert_eq!(o.code, EM_ECODE);
        assert_eq!(o.note, Note::SameBandWrong);
        assert_eq!(band_of(2), band_of(1));
    }

    #[test]
    fn t31_out_of_table_code_fails_first() {
        let s = Shape::lands(Scene::Dispatch).with_code(200);
        let o = define(&s);
        assert_eq!(o.stage, Stage::Arg);
        assert_eq!(o.code, EM_EARG);
        assert!(o.refused());
        // 层错同时存在时仍要先挂入参——次序不可倒。
        let both = s.with_layer(Layer::Shell);
        assert_eq!(define(&both).stage, Stage::Arg);
    }

    #[test]
    fn t32_layer_is_checked_before_text() {
        let s = Shape::lands(Scene::Dispatch)
            .with_layer(Layer::Shell)
            .with_text(T_SHCMD_NOLINE);
        let o = define(&s);
        assert_eq!(o.stage, Stage::Layer);
        assert_eq!(o.code, EM_ELAYER);
        assert_eq!(o.note, Note::LayerWrong);
    }

    #[test]
    fn t33_lookup_is_checked_before_band_and_code() {
        let s = Shape::lands(Scene::Builtin)
            .with_code(EXIT_ZERO)
            .with_lookup(true);
        let o = define(&s);
        assert_eq!(o.stage, Stage::Lookup);
        assert_eq!(o.code, EM_ELOOK);
        assert!(o.refused());
    }

    #[test]
    fn t34_text_is_checked_before_band() {
        let s = Shape::lands(Scene::ShCmd)
            .with_code(EXIT_ZERO)
            .with_text(T_SHCMD_NOLINE);
        let o = define(&s);
        assert_eq!(o.stage, Stage::Text);
        assert_eq!(o.code, EM_ETEXT);
    }

    #[test]
    fn t35_unjudged_lands_in_the_default_tier() {
        let s = Shape::lands(Scene::Alias).with_unjudged(true);
        let o = define(&s);
        assert!(!o.ok());
        assert_eq!(o.stage, Stage::Rate);
        assert_eq!(o.code, EM_EDEFAULT);
        assert!(o.default);
        assert!(!o.counts());
        assert_eq!(Tier::of(&o), Tier::Unrun);
    }

    #[test]
    fn t36_phantom_excuse_is_a_grid_defect() {
        let s = Shape::lands(Scene::Dispatch).with_declared(true);
        let o = define(&s);
        assert!(!o.ok());
        assert_eq!(o.stage, Stage::Grid);
        assert_eq!(o.code, EM_EGRID);
        assert_eq!(o.note, Note::PhantomExcuse);
    }

    #[test]
    fn t37_corpus_slots_and_seals_are_unique() {
        let all = cases();
        assert_eq!(all.len(), CORPUS);
        let mut i = 0usize;
        while i < CORPUS {
            let a = case_at(i).expect("slot");
            assert_eq!(a.tag(), WANT_TAGS[i]);
            let mut j = i + 1usize;
            while j < CORPUS {
                let b = case_at(j).expect("slot");
                assert_ne!(a.seal(), b.seal());
                j += 1;
            }
            i += 1;
        }
        assert!(cover(CORPUS).is_ok());
        assert_eq!(cover(CORPUS + 1), Err(EM_ESLOT));
    }

    #[test]
    fn t38_eight_accept_slots_are_green() {
        let mut i = 0usize;
        while i < ACCEPT {
            let c = case_at(i).expect("accept");
            assert!(c.ok());
            assert_eq!(c.fault(), Fault::Ok);
            assert!(c.actual().counts());
            assert_eq!(c.actual().value, 1);
            i += 1;
        }
    }

    #[test]
    fn t39_six_reject_slots_hit_their_own_dimension() {
        let mut i = ACCEPT;
        while i < CORPUS {
            let c = case_at(i).expect("reject");
            assert!(!c.ok());
            assert_eq!(c.fault(), expected_reject_ndx(i));
            assert!(!c.actual().ok());
            assert_eq!(c.actual().value, 0);
            i += 1;
        }
    }

    #[test]
    fn t40_estate_beats_every_other_dimension() {
        let c = case_at(9).expect("slot");
        let claim = Claim {
            success: true,
            ..c.claim
        };
        let cross = Case {
            shape: c.shape,
            claim,
        };
        assert_eq!(judge(&cross), Fault::Estate);
    }

    #[test]
    fn t41_stage_beats_code_and_value() {
        let c = case_at(11).expect("slot");
        let mut claim = c.claim;
        claim.stage = Stage::Grid;
        claim.code = 0;
        claim.value = 0;
        let cross = Case {
            shape: c.shape,
            claim,
        };
        assert_eq!(judge(&cross), Fault::Stage);
    }

    #[test]
    fn t42_value_beats_note() {
        let c = case_at(12).expect("slot");
        let mut claim = c.claim;
        claim.value = 3;
        claim.note = Note::Lands;
        let cross = Case {
            shape: c.shape,
            claim,
        };
        assert_eq!(judge(&cross), Fault::Value);
    }

    #[test]
    fn t43_default_is_judged_even_when_other_fields_agree() {
        let c = case_at(13).expect("slot");
        let mut claim = c.claim;
        claim.note = Note::Lands;
        let cross = Case {
            shape: c.shape,
            claim,
        };
        assert_eq!(judge(&cross), Fault::Note);
        let exact = Case {
            shape: c.shape,
            claim: c.claim,
        };
        assert_eq!(judge(&exact), Fault::Default);
    }

    #[test]
    fn t44_matrix_covers_all_four_tiers() {
        let m = Matrix::build();
        assert_eq!(m.slots(), CORPUS * TIER_COUNT);
        assert_eq!(m.goods(), CORPUS);
        assert_eq!(m.red_total() + m.goods(), m.slots());
        assert!(m.covered());
        assert_eq!(m.row(Tier::Lands).map(|r| r.good), Some(LANDS_SAMPLES));
        assert_eq!(m.row(Tier::Excused).map(|r| r.good), Some(EXCUSED_SAMPLES));
        assert_eq!(m.row(Tier::Void).map(|r| r.good), Some(VOID_SAMPLES));
        assert_eq!(m.row(Tier::Unrun).map(|r| r.good), Some(UNRUN_SAMPLES));
        assert_eq!(Tier::all().len(), TIER_COUNT);
    }

    #[test]
    fn t45_report_is_green_and_renders_verbatim() {
        let rep = run().expect("green");
        assert!(rep.green());
        assert_eq!(rep.render().as_str(), RENDER_OK);
        assert_eq!(rep.rate(), RATE_FULL);
        assert_eq!(rep.rate(), (rep.matched as u32 * 1000 + (rep.total as u32) / 2) / rep.total as u32);
        assert_eq!(rep.reds(), 0);
    }

    #[test]
    fn t46_empty_report_is_not_green() {
        let rep = empty_report();
        assert_eq!(rep.total, 0);
        assert!(!rep.green());
        assert_eq!(rep.rate(), 0);
        assert_eq!(rep.gate(), Err(EM_EGRID));
        assert_ne!(rep.render().as_str(), RENDER_OK);
    }

    #[test]
    fn t47_injection_out_of_range_is_a_slot_error() {
        assert_eq!(run_with(Some((CORPUS, Fault::Ok))), Err(EM_ESLOT));
        assert_eq!(run_with(Some((CORPUS + 9, Fault::Note))), Err(EM_ESLOT));
        assert_eq!(run_with(Some((0, Fault::Value))).map(|r| r.conflicts), Ok(1));
        assert_eq!(run_with(Some((7, Fault::Value))).map(|r| r.conflicts), Ok(1));
    }

    #[test]
    fn t48_injection_lowers_the_rate() {
        let base = run().expect("green");
        let bad = run_with(Some((2, Fault::Value))).expect("report");
        assert!(bad.matched < base.matched);
        assert!(bad.rate() < base.rate());
        assert_ne!(bad.gate(), Ok(()));
        assert!(red_rate_calibrated());
        assert_eq!(RED_RATE, 929);
    }

    #[test]
    fn t49_gate_full_pass_and_regression() {
        let root = Gate::full_pass().expect("full");
        assert_ne!(root, 0);
        let mut g = Gate::new();
        g.step(Stage::Grid).expect("step");
        assert_eq!(g.step(Stage::Code), Err(EM_ESTAGE));
        assert_eq!(g.stage(), Some(Stage::Grid));
        assert_eq!(g.refusals(), 1);
        assert_eq!(g.steps(), 1);
        assert!(g.root() != 0);
    }

    #[test]
    fn t50_gate_close_is_final() {
        let mut g = Gate::new();
        assert_eq!(g.close(), Err(EM_ESTAGE));
        g.step(Stage::Arg).expect("step");
        let h = g.close().expect("close");
        assert!(g.is_closed());
        assert_eq!(g.root(), h);
        assert_eq!(g.close(), Err(EM_ESTATE));
        assert_eq!(g.step(Stage::Rate), Err(EM_ESTATE));
        assert_eq!(g.refusals(), 2);
        assert_eq!(Gate::walk(&[Stage::Arg, Stage::Grid, Stage::Band]), Err(EM_ESTAGE));
        assert!(Gate::walk(&[Stage::Arg, Stage::Grid]).is_ok());
    }

    #[test]
    fn t51_ledger_audits_fourteen_cells() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), STAT_CELLS + 1);
        assert_eq!(l.err_count(), REJECT);
        assert_eq!(l.rejects(), 0);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.first_mismatch().is_none());
        assert!(!l.is_frozen());
        assert_ne!(l.seal(), 0);
    }

    #[test]
    fn t52_broken_ledgers_are_rejected_cell_by_cell() {
        assert_eq!(build_ledger_broken(0).audit(), Err(EM_EVALUE));
        assert_eq!(build_ledger_broken(1).audit(), Err(EM_EVALUE));
        assert_eq!(build_ledger_broken(2).audit(), Err(EM_EVALUE));
        assert_eq!(build_ledger_broken(9).audit(), Ok(()));
        let l = build_ledger();
        assert_ne!(l.seal(), build_ledger_broken(0).seal());
    }

    #[test]
    fn t53_ledger_register_rejects_holes_and_duplicates() {
        let mut l = Ledger::new();
        assert!(l.is_empty());
        let a = Sample {
            slot: 0,
            op: 1,
            ok: true,
            code: 0,
            value: 5,
        };
        assert_eq!(l.register(a), Ok(()));
        assert_eq!(l.register(a), Err(EM_ESLOT));
        let hole = Sample { slot: 4, ..a };
        assert_eq!(l.register(hole), Err(EM_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.len(), 1);
        assert!(l.replay().is_err());
        assert_eq!(l.poke(0, 9), Ok(()));
        assert_eq!(l.sample(0).map(|s| s.value), Some(9));
        assert_eq!(l.poke(SAMPLES, 1), Err(EM_ESLOT));
    }

    #[test]
    fn t54_freeze_then_replay_matches() {
        let mut l = build_ledger();
        let h = l.freeze().expect("freeze");
        assert!(h != 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), h);
        assert_eq!(l.replay(), Ok(h));
        assert_eq!(l.freeze(), Err(EM_ESTATE));
        assert_eq!(l.poke(0, 1), Err(EM_ESTATE));
        assert_eq!(l.register(Sample { slot: 0, op: 1, ok: true, code: 0, value: 1 }), Err(EM_ESTATE));
    }

    #[test]
    fn t55_inject_four_each_route_holds() {
        let four = inject_four();
        let mut i = 0usize;
        while i < four.len() {
            assert!(four[i]);
            i += 1;
        }
        assert!(grid_agrees_with_gate());
        assert!(report_agrees_with_matrix());
    }

    #[test]
    fn t56_all_thirteen_checks_are_green() {
        let s = run_unx11679_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(s.len(), 13);
    }
}
